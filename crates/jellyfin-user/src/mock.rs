//! Tiny in-process HTTP server for unit tests: replies from a route table and records every
//! request, so tests can assert what a write sent without any real Jellyfin.
use crate::{Session, UserClient};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub type Log = Arc<Mutex<Vec<(String, String, String)>>>;

/// `routes`: (METHOD, path-without-query, status, body). Unlisted requests get 404.
pub async fn serve(routes: Vec<(&'static str, &'static str, u16, String)>) -> (UserClient, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let log2 = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (routes, log) = (routes.clone(), log2.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let (head, body_start) = loop {
                    let mut chunk = [0u8; 4096];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break (String::from_utf8_lossy(&buf[..i]).to_string(), i + 4);
                    }
                };
                let len = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() < body_start + len {
                    let mut chunk = [0u8; 4096];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let mut first = head.lines().next().unwrap_or_default().split(' ');
                let (m, target) = (
                    first.next().unwrap_or_default(),
                    first.next().unwrap_or_default(),
                );
                let body = String::from_utf8_lossy(&buf[body_start..]).to_string();
                log.lock().unwrap().push((m.into(), target.into(), body));
                let path = target.split('?').next().unwrap_or_default();
                let (code, out) = routes
                    .iter()
                    .find(|r| r.0 == m && r.1 == path)
                    .map_or((404, String::new()), |r| (r.2, r.3.clone()));
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    let c = UserClient::from_session(
        &url,
        &Session {
            access_token: "t".into(),
            user_id: "me".into(),
            server_id: "s".into(),
            user_name: "n".into(),
            is_admin: false,
        },
    )
    .unwrap();
    (c, log)
}
