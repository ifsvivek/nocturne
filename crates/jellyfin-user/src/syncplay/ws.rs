//! The Jellyfin WebSocket: connect with the session token, answer keep-alives, reconnect with
//! backoff, and decode SyncPlay messages into [`Incoming`]. The handshake goes through reqwest
//! (HTTP/1.1 upgrade) so TLS, proxies and certificate handling are exactly those of every other
//! call; tungstenite only does the framing.
use super::model::{Incoming, decode};
use crate::{Error, Result, UserClient};
use futures_util::{SinkExt, StreamExt};
use reqwest::header;
use std::hash::{BuildHasher, Hasher};
use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::time::{Instant, interval, sleep, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::{Message, protocol::Role};

#[derive(Debug, Clone, PartialEq)]
pub enum SocketEvent {
    Connected,
    /// The socket dropped; a reconnect is under way.
    Disconnected,
    Message(Incoming),
}

/// Seconds to wait before reconnect attempt `n` (0-based): 1, 2, 4, 8, then every 15.
pub fn backoff_secs(n: u32) -> u64 {
    (1u64 << n.min(4)).min(15)
}

/// Paths the Jellyfin socket has lived at: `/socket` (10.x and the web client) first.
const PATHS: [&str; 2] = ["socket", "websocket"];

fn random_key() -> String {
    use base64::Engine;
    let s = std::collections::hash_map::RandomState::new();
    let mut bytes = Vec::with_capacity(16);
    for i in 0..2u64 {
        let mut h = s.build_hasher();
        h.write_u64(i);
        bytes.extend_from_slice(&h.finish().to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

type Ws = WebSocketStream<reqwest::Upgraded>;

async fn connect(c: &UserClient) -> Result<Ws> {
    // No total timeout and HTTP/1.1 only: the connection outlives any request deadline.
    let http = reqwest::Client::builder()
        .http1_only()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Http(e.without_url()))?;
    let mut last = Error::Config("no socket path".into());
    for p in PATHS {
        let mut url = c.base.clone();
        url.path_segments_mut()
            .expect("http url has a base")
            .pop_if_empty()
            .push(p);
        let req = http
            .get(url)
            .header(header::AUTHORIZATION, c.auth.clone())
            .header(header::CONNECTION, "Upgrade")
            .header(header::UPGRADE, "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", random_key());
        let resp = match timeout(Duration::from_secs(15), req.send()).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                last = Error::Http(e.without_url());
                continue;
            }
            Err(_) => {
                last = Error::Config("socket handshake timed out".into());
                continue;
            }
        };
        if resp.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            last = Error::Status {
                status: resp.status().as_u16(),
                path: format!("/{p}"),
            };
            continue;
        }
        let up = resp
            .upgrade()
            .await
            .map_err(|e| Error::Http(e.without_url()))?;
        return Ok(WebSocketStream::from_raw_socket(up, Role::Client, None).await);
    }
    Err(last)
}

/// One connected session: returns when the socket is gone or the receiver was dropped.
/// `false`: the receiver is gone, stop for good.
async fn session(mut ws: Ws, tx: &Sender<SocketEvent>) -> bool {
    let mut tick = interval(Duration::from_secs(15));
    let mut last_seen = Instant::now();
    let mut keepalive = Duration::from_secs(30);
    let mut last_keepalive = Instant::now();
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if last_seen.elapsed() > Duration::from_secs(50) {
                    return true; // silence for too long: treat as dead
                }
                if last_keepalive.elapsed() >= keepalive / 2 {
                    last_keepalive = Instant::now();
                    if ws.send(Message::text(r#"{"MessageType":"KeepAlive"}"#)).await.is_err() {
                        return true;
                    }
                }
                if ws.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return true;
                }
            }
            m = ws.next() => {
                let Some(Ok(m)) = m else { return true };
                last_seen = Instant::now();
                match m {
                    Message::Text(t) => match decode(t.as_str()) {
                        Incoming::ForceKeepAlive(s) => {
                            keepalive = Duration::from_secs(s.clamp(10, 300));
                            last_keepalive = Instant::now();
                            let _ = ws.send(Message::text(r#"{"MessageType":"KeepAlive"}"#)).await;
                        }
                        Incoming::Other => {}
                        i => {
                            if tx.send(SocketEvent::Message(i)).is_err() {
                                return false;
                            }
                        }
                    },
                    Message::Close(_) => return true,
                    _ => {}
                }
            }
        }
    }
}

/// Connects and stays connected until the receiver of `tx` is dropped (abort the task to stop
/// earlier). Send errors only ever end the loop; they never panic.
pub async fn run(client: UserClient, tx: Sender<SocketEvent>) {
    let mut tries = 0u32;
    loop {
        match connect(&client).await {
            Ok(ws) => {
                tries = 0;
                if tx.send(SocketEvent::Connected).is_err() {
                    return;
                }
                let alive = session(ws, &tx).await;
                if !alive || tx.send(SocketEvent::Disconnected).is_err() {
                    return;
                }
            }
            Err(Error::Status {
                status: 401 | 403, ..
            }) => {
                // The token is dead; the app's own connection watch handles sign-in.
                tries = tries.max(3);
            }
            Err(_) => {}
        }
        sleep(Duration::from_secs(backoff_secs(tries))).await;
        tries += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        let v: Vec<u64> = (0..7).map(backoff_secs).collect();
        assert_eq!(v, [1, 2, 4, 8, 15, 15, 15]);
    }

    /// In-process server: upgrades every connection, sends `hello` once, forwards what the client
    /// writes, and closes the first connection after `drop_first` text frames were seen.
    async fn serve(
        hello: &'static str,
    ) -> (
        UserClient,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let heads: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let heads2 = heads.clone();
        tokio::spawn(async move {
            let mut n = 0;
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut buf = vec![0u8; 4096];
                let mut got = 0;
                while !buf[..got].windows(4).any(|w| w == b"\r\n\r\n") {
                    let k = s.read(&mut buf[got..]).await.unwrap_or(0);
                    if k == 0 {
                        break;
                    }
                    got += k;
                }
                heads2
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..got]).to_string());
                n += 1;
                let _ = s
                    .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: x\r\n\r\n")
                    .await;
                let mut ws = WebSocketStream::from_raw_socket(s, Role::Server, None).await;
                let tx = tx.clone();
                let first = n == 1;
                tokio::spawn(async move {
                    let _ = ws.send(Message::text(hello)).await;
                    let _ = ws
                        .send(Message::text(
                            r#"{"MessageType":"ForceKeepAlive","Data":20}"#,
                        ))
                        .await;
                    while let Some(Ok(m)) = ws.next().await {
                        if let Message::Text(t) = m {
                            let _ = tx.send(t.as_str().to_string());
                            if first {
                                return; // drop the first connection after the first client frame
                            }
                        }
                    }
                });
            }
        });
        let c = UserClient::from_session(
            &url,
            &crate::Session {
                access_token: "tok".into(),
                user_id: "me".into(),
                server_id: "s".into(),
                user_name: "n".into(),
                is_admin: false,
            },
        )
        .unwrap();
        (c, rx, heads)
    }

    #[tokio::test]
    async fn connects_decodes_answers_keepalive_and_reconnects() {
        let hello = r#"{"MessageType":"SyncPlayGroupUpdate","Data":{"GroupId":"g","Type":"UserJoined","Data":"bob"}}"#;
        let (c, mut from_client, heads) = serve(hello).await;
        let (tx, rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(run(c, tx));
        let next = || async {
            let rx = &rx;
            tokio::time::timeout(Duration::from_secs(8), async {
                loop {
                    if let Ok(e) = rx.try_recv() {
                        return e;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("socket event")
        };
        assert_eq!(next().await, SocketEvent::Connected);
        assert_eq!(
            next().await,
            SocketEvent::Message(Incoming::Group(super::super::GroupUpdate::UserJoined(
                "bob".into()
            )))
        );
        // ForceKeepAlive is answered, never surfaced.
        let frame = tokio::time::timeout(Duration::from_secs(5), from_client.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(frame.contains("KeepAlive"), "{frame}");
        // The server dropped that connection: Disconnected, then a fresh Connected.
        assert_eq!(next().await, SocketEvent::Disconnected);
        assert_eq!(next().await, SocketEvent::Connected);
        let h = heads.lock().unwrap();
        assert_eq!(h.len(), 2);
        let first = h[0].to_lowercase();
        assert!(first.starts_with("get /socket "), "{first}");
        assert!(first.contains("authorization: mediabrowser") && first.contains("token=\"tok\""));
        assert!(first.contains("upgrade: websocket") && first.contains("sec-websocket-key:"));
        task.abort();
    }

    #[test]
    fn keys_are_base64_of_16_bytes_and_differ() {
        let (a, b) = (random_key(), random_key());
        assert_eq!(a.len(), 24);
        assert_ne!(a, b);
    }
}
