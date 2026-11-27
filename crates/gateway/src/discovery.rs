//! UDP discovery (Jellyfin phone and TV apps): "Who is JellyfinServer?" on port 7359 is answered with a JSON line. Runs only in Local network mode with "Announce" on; skipped if the port is taken.
use crate::state::State;
use nocturne_core::gateway::{Binding, DISCOVERY_PORT, address_url, peer_allowed};
use serde_json::json;
use std::net::{IpAddr, UdpSocket as StdUdp};
use std::time::Instant;
use tokio::net::UdpSocket;

/// The answer to one datagram, or `None` if it is not the discovery question.
pub fn reply_for(datagram: &[u8], address: &str, server_id: &str) -> Option<Vec<u8>> {
    let t = std::str::from_utf8(datagram).ok()?.trim();
    if !t.eq_ignore_ascii_case("who is jellyfinserver?") {
        return None;
    }
    serde_json::to_vec(
        &json!({"Address": address, "Id": server_id, "Name": "NOCTURNE", "EndpointAddress": null}),
    )
    .ok()
}

/// Our address on the route towards `peer` (a UDP "connect" sends nothing).
fn local_towards(peer: IpAddr) -> Option<IpAddr> {
    let bind = if peer.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let s = StdUdp::bind(bind).ok()?;
    s.connect((peer, DISCOVERY_PORT)).ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

pub async fn run(st: State, http_port: u16) {
    let sock = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await {
        Ok(s) => s,
        Err(e) => {
            crate::glog!("discovery skipped: UDP {DISCOVERY_PORT}: {e}");
            return;
        }
    };
    let mut buf = [0u8; 64];
    loop {
        let Ok((n, peer)) = sock.recv_from(&mut buf).await else {
            continue;
        };
        if !peer_allowed(Binding::Lan, peer.ip())
            || !st
                .limits
                .ip
                .allow(&crate::limits::limit_key(peer.ip()), Instant::now())
        {
            continue;
        }
        let Some(host) = local_towards(peer.ip()) else {
            continue;
        };
        let address = address_url(Binding::Lan, &host.to_string(), http_port);
        if let Some(r) = reply_for(&buf[..n], &address, &st.users.server_id()) {
            let _ = sock.send_to(&r, peer).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_only_the_question() {
        let r = reply_for(b"Who is JellyfinServer?", "http://192.168.1.5:8099", "abc").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
        assert_eq!(
            (v["Address"].as_str(), v["Id"].as_str()),
            (Some("http://192.168.1.5:8099"), Some("abc"))
        );
        assert!(reply_for(b"who is jellyfinserver?\n", "a", "b").is_some());
        assert!(reply_for(b"hello", "a", "b").is_none());
        assert!(reply_for(&[0xff, 0xfe], "a", "b").is_none());
        assert!(reply_for(b"", "a", "b").is_none());
    }
}
