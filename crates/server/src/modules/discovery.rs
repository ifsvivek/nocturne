//! `discovery`: find media servers on the local network, and (only with `announce` on) let NOCTURNE desktop clients find this one.
//! * Scan (`GET /discovery/servers`): one UDP broadcast of Jellyfin's question ("Who is JellyfinServer?", port 7359) and of ours ("Who is NocturneServer?", [`DiscoveryCfg::port`]), answers collected for two seconds. The gateway answers Jellyfin's question itself while it runs in Local network mode (as in 0.7).
//! * Announce (off by default): answer "Who is NocturneServer?" with this server's id, name and address. Peers outside private ranges are ignored.
//!
//! ponytail: plain UDP broadcast only; mDNS (`_nocturne._tcp`) when a client needs it across subnets.
use crate::api::{ApiErr, ApiResult};
use crate::auth::Caller;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::state::Shared;
use axum::extract::State;
use axum::{Extension, Json};
use nocturne_core::gateway::{Binding, DISCOVERY_PORT, peer_allowed};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::watch;

const NAME: &str = "discovery";
pub const NOCTURNE_PORT: u16 = 7360;
const QUESTION: &str = "Who is NocturneServer?";
const JF_QUESTION: &str = "Who is JellyfinServer?";

/// `[discovery]` in `server.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoveryCfg {
    pub enabled: bool,
    /// Answer NOCTURNE clients' question (off by default: the server stays quiet on the network).
    pub announce: bool,
    pub port: u16,
    /// Shown to clients; empty = the host name.
    pub name: String,
}

impl Default for DiscoveryCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            announce: false,
            port: NOCTURNE_PORT,
            name: String::new(),
        }
    }
}

/// One server that answered a scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    /// `jellyfin` or `nocturne`.
    pub kind: String,
    pub id: String,
    pub name: String,
    pub address: String,
}

/// This server's discovery id: random, made once, kept in the database.
fn server_id(sh: &Shared) -> String {
    const KEY: &str = "discovery:id";
    if let Ok(Some(v)) = sh.db.meta(KEY) {
        return format!("{v:016x}");
    }
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    let v = u64::from_le_bytes(b) | 1;
    let _ = sh.db.set_meta(KEY, v);
    format!("{v:016x}")
}

fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "NOCTURNE".into())
}

/// Our address on the route towards `peer` (a UDP "connect" sends nothing).
fn local_towards(peer: SocketAddr) -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind(if peer.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .ok()?;
    s.connect(peer).ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// The answer to one datagram, or `None` if it is not our question.
pub fn reply_for(datagram: &[u8], id: &str, name: &str, address: &str) -> Option<Vec<u8>> {
    let t = std::str::from_utf8(datagram).ok()?.trim();
    if !t.eq_ignore_ascii_case(QUESTION) {
        return None;
    }
    serde_json::to_vec(&json!({"Id": id, "Name": name, "Address": address, "Product": "NOCTURNE", "Version": env!("CARGO_PKG_VERSION")})).ok()
}

async fn announce(
    sock: UdpSocket,
    id: String,
    name: String,
    http_port: u16,
    mut stop: watch::Receiver<bool>,
) {
    let mut buf = [0u8; 64];
    loop {
        let (n, peer) = tokio::select! {
            _ = stop.changed() => return,
            r = sock.recv_from(&mut buf) => match r { Ok(x) => x, Err(_) => continue },
        };
        if !peer_allowed(Binding::Lan, peer.ip()) {
            continue;
        }
        let Some(host) = local_towards(peer) else {
            continue;
        };
        let address = match host {
            IpAddr::V6(h) => format!("http://[{h}]:{http_port}"),
            IpAddr::V4(h) => format!("http://{h}:{http_port}"),
        };
        if let Some(r) = reply_for(&buf[..n], &id, &name, &address) {
            let _ = sock.send_to(&r, peer).await;
        }
    }
}

/// Ask `targets` both questions and collect the answers for `wait`.
pub async fn scan(targets: &[SocketAddr], wait: Duration) -> Vec<Found> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else {
        return vec![];
    };
    let _ = sock.set_broadcast(true);
    for t in targets {
        let q = if t.port() == DISCOVERY_PORT {
            JF_QUESTION
        } else {
            QUESTION
        };
        let _ = sock.send_to(q.as_bytes(), t).await;
    }
    let mut out: Vec<Found> = vec![];
    let mut buf = [0u8; 2048];
    let end = tokio::time::Instant::now() + wait;
    while let Ok(Ok((n, from))) = tokio::time::timeout_at(end, sock.recv_from(&mut buf)).await {
        let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) else {
            continue;
        };
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let f = Found {
            kind: if v["Product"] == "NOCTURNE" {
                "nocturne"
            } else {
                "jellyfin"
            }
            .into(),
            id: s("Id"),
            name: s("Name"),
            address: s("Address"),
        };
        if !f.address.is_empty()
            && peer_allowed(Binding::Lan, from.ip())
            && !out.iter().any(|o| o.id == f.id && o.address == f.address)
        {
            out.push(f);
        }
    }
    out
}

#[derive(Default)]
struct Run(Mutex<Option<watch::Sender<bool>>>);

#[derive(Default)]
pub struct Discovery;

impl Module for Discovery {
    fn name(&self) -> &'static str {
        NAME
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("/servers", servers)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            let cfg: DiscoveryCfg = sh.cfg().section(NAME);
            if !cfg.announce {
                return Ok(());
            }
            let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, cfg.port))
                .await
                .map_err(|e| format!("cannot answer on UDP {}: {e}", cfg.port))?;
            let name = if cfg.name.trim().is_empty() {
                host_name()
            } else {
                cfg.name.trim().to_string()
            };
            let (tx, rx) = watch::channel(false);
            *sh.ext::<Run>().0.lock().unwrap() = Some(tx);
            tokio::spawn(announce(sock, server_id(sh), name, sh.cfg().http.port, rx));
            Ok(())
        })
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async move {
            if let Some(tx) = sh.ext::<Run>().0.lock().unwrap().take() {
                let _ = tx.send(true);
            }
        })
    }
}

/// Scan the local network now (two seconds).
async fn servers(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
) -> ApiResult<Vec<Found>> {
    if !c.admin() {
        return Err(ApiErr::forbidden("only an administrator scans the network"));
    }
    let cfg: DiscoveryCfg = sh.cfg().section(NAME);
    let b = Ipv4Addr::BROADCAST;
    let mut found = scan(
        &[(b, DISCOVERY_PORT).into(), (b, cfg.port).into()],
        Duration::from_secs(2),
    )
    .await;
    // Not ourselves.
    let me = server_id(&sh);
    found.retain(|f| f.id != me);
    Ok(Json(found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_only_our_question() {
        let r = reply_for(
            b"who is nocturneserver?\n",
            "ab",
            "den",
            "http://192.168.1.5:8100",
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(
            (v["Id"].as_str(), v["Product"].as_str()),
            (Some("ab"), Some("NOCTURNE"))
        );
        assert!(reply_for(b"Who is JellyfinServer?", "a", "b", "c").is_none());
        assert!(reply_for(&[0xff], "a", "b", "c").is_none());
    }

    #[tokio::test]
    async fn a_scan_finds_an_announcing_server_and_a_jellyfin_answer() {
        // Our announcer on a loopback port.
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = sock.local_addr().unwrap().port();
        let (_tx, rx) = watch::channel(false);
        tokio::spawn(announce(sock, "id1".into(), "den".into(), 8100, rx));
        // A stand-in Jellyfin answering its question.
        let jf = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let jf_port = jf.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut b = [0u8; 64];
            let (_, peer) = jf.recv_from(&mut b).await.unwrap();
            let r = json!({"Id": "jf", "Name": "Jelly", "Address": "http://127.0.0.1:8096"})
                .to_string();
            jf.send_to(r.as_bytes(), peer).await.unwrap();
        });
        let found = scan(
            &[
                SocketAddr::from(([127, 0, 0, 1], port)),
                SocketAddr::from(([127, 0, 0, 1], jf_port)),
            ],
            Duration::from_millis(500),
        )
        .await;
        assert!(found.iter().any(|f| f.kind == "nocturne"
            && f.id == "id1"
            && f.address == "http://127.0.0.1:8100"));
        assert!(found.iter().any(|f| f.kind == "jellyfin" && f.id == "jf"));
    }
}
