//! Security review tests (docs/dev/GATEWAY_PLAN.md section 10): what a hostile client or a hostile upstream can and cannot do. In process, loopback only, a mock server for the upstream. Unix only (local-file and socket checks use unix paths and permissions; the gateway ships on Linux).
#![cfg(unix)]
use axum::Router;
use axum::extract::{Request, State as AxState};
use axum::http::{HeaderValue, StatusCode as Sc};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use nocturne_core::gateway::{self as core_gw, Binding, GatewaySettings};
use nocturne_core::source::SourceId;
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::limits::{Buckets, Limits};
use nocturne_gateway::server;
use nocturne_gateway::state::{Secret, State, UpKind, UpState, Upstream};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;

// ---------------------------------------------------------------- harness

struct Gw {
    st: State,
    base: String,
    addrs: Vec<SocketAddr>,
    http: reqwest::Client,
    pw: String,
    _dir: tempfile::TempDir,
}

async fn start_cfg(gw: GatewaySettings, writes: bool, progress: bool, limits: Limits) -> Gw {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        data: dir.path().join("data"),
        runtime: None,
        settings: Some(dir.path().join("settings.toml")),
        sources: None,
    };
    let cfg = Config {
        gw,
        allow_server_writes: writes,
        sync_progress: progress,
        sync_watched: true,
    };
    let st = State::open_with(paths, cfg, limits).unwrap();
    let pw = st.users.take_unseen().unwrap();
    tokio::spawn(server::run(st.clone()));
    let addrs = tokio::time::timeout(Duration::from_secs(5), st.wait_listening())
        .await
        .unwrap();
    let v4 = addrs.iter().find(|a| a.is_ipv4()).unwrap_or(&addrs[0]);
    let base = format!("http://127.0.0.1:{}", v4.port());
    Gw {
        st,
        base,
        addrs,
        http: reqwest::Client::new(),
        pw,
        _dir: dir,
    }
}

fn wide() -> Limits {
    Limits {
        ip: Buckets::new(1e9, 1e9),
        token: Buckets::new(1e9, 1e9),
        ..Default::default()
    }
}

async fn start(writes: bool, progress: bool) -> Gw {
    let gw = GatewaySettings {
        port: 0,
        remote_control: true,
        ..Default::default()
    };
    start_cfg(gw, writes, progress, wide()).await
}

fn auth(token: &str) -> String {
    format!(r#"MediaBrowser Token="{token}""#)
}

impl Gw {
    fn url(&self, p: &str) -> String {
        format!("{}{p}", self.base)
    }
    async fn login(&self, device: &str) -> String {
        let r = self
            .http
            .post(self.url("/Users/AuthenticateByName"))
            .header("Authorization", format!(r#"MediaBrowser Client="C", Device="{device}", DeviceId="{device}", Version="1""#))
            .json(&json!({"Username": "nocturne", "Pw": self.pw}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.json::<Value>().await.unwrap()["AccessToken"]
            .as_str()
            .unwrap()
            .to_string()
    }
    async fn call(&self, m: reqwest::Method, token: &str, path: &str, body: Option<Value>) -> u16 {
        let mut r = self
            .http
            .request(m, self.url(path))
            .header("Authorization", auth(token));
        if let Some(b) = body {
            r = r.json(&b);
        }
        r.send().await.unwrap().status().as_u16()
    }
}

// ---------------------------------------------------------------- a recording mock upstream

type Rec = (String, String, Vec<(String, String)>);

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<Rec>>>);

impl Seen {
    fn all(&self) -> Vec<Rec> {
        self.0.lock().unwrap().clone()
    }
    fn writes(&self) -> Vec<Rec> {
        self.all().into_iter().filter(|r| r.0 != "GET").collect()
    }
}

#[derive(Clone)]
struct Mock {
    seen: Seen,
    evil: String,
}

async fn mock_any(AxState(m): AxState<Mock>, req: Request) -> axum::response::Response {
    let (uri, method) = (req.uri().to_string(), req.method().to_string());
    let hdrs = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    m.seen
        .0
        .lock()
        .unwrap()
        .push((method.clone(), uri.clone(), hdrs));
    let path = uri.split('?').next().unwrap_or("");
    if path.ends_with("/stream.mkv") {
        let mut r = Sc::FOUND.into_response();
        r.headers_mut().insert(
            "location",
            HeaderValue::from_str(&format!("{}/loot", m.evil)).unwrap(),
        );
        return r;
    }
    if path.ends_with("/stream") {
        let mut r = ([("content-type", "video/mp4")], "abc").into_response();
        for (k, v) in [
            ("set-cookie", "sid=SERVERCOOKIE; Path=/"),
            ("x-server-secret", "SRVHDR"),
            ("access-control-allow-origin", "*"),
        ] {
            r.headers_mut().insert(k, HeaderValue::from_static(v));
        }
        return r;
    }
    if method == "GET" && path == "/Items/realitem0" {
        return axum::Json(json!({
            "Id": "realitem0", "Name": "Demo Movie", "Type": "Movie", "MediaType": "Video",
            "RunTimeTicks": 6_000_000_000i64, "ProductionYear": 2020,
            "Path": "/srv/media/Demo Movie (2020).mkv", "ImageTags": {"Primary": "abc"},
            "MediaSources": [{"Id": "ms0", "Path": "/srv/media/Demo Movie (2020).mkv"}],
            "UserData": {"Played": false, "PlaybackPositionTicks": 0, "PlayCount": 0, "IsFavorite": false},
        }))
        .into_response();
    }
    if method == "GET" && path == "/Sessions" {
        return axum::Json(json!([{
            "Id": "realsess1", "DeviceId": "mpv-1", "Client": "Jellyfin MPV Shim", "DeviceName": "Den",
            "UserId": "acct", "UserName": "real", "SupportsRemoteControl": true,
            "LastActivityDate": "2026-11-27T10:00:00.0000000Z",
        }]))
        .into_response();
    }
    Sc::NO_CONTENT.into_response()
}

/// Returns (server url, what it saw, number of hits on a second "evil" host).
async fn mock() -> (String, Seen, Arc<Mutex<u32>>) {
    let hits = Arc::new(Mutex::new(0u32));
    let h2 = hits.clone();
    let evil_app = Router::new().fallback(move || {
        let h = h2.clone();
        async move {
            *h.lock().unwrap() += 1;
            Sc::OK
        }
    });
    let el = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let evil = format!("http://{}", el.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(el, evil_app).await });
    let seen = Seen::default();
    let app = Router::new().fallback(mock_any).with_state(Mock {
        seen: seen.clone(),
        evil,
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (url, seen, hits)
}

fn upstream(url: String) -> Upstream {
    Upstream {
        id: SourceId(1),
        label: "Server A".into(),
        kind: UpKind::Server {
            url,
            user_id: "acct".into(),
            source_key: "srvA".into(),
            token: Some(Secret::new("UPTOKEN".into())),
        },
        state: UpState::Ready,
    }
}

// ---------------------------------------------------------------- SSRF and header hygiene

#[tokio::test]
async fn nothing_of_the_client_reaches_the_server_and_no_server_header_comes_back() {
    let (url, seen, _) = mock().await;
    let authority = url.trim_start_matches("http://").to_string();
    let g = start(false, false).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev").await;
    let gid = g.st.ids.register_single("srvA", "realitem0").to_string();

    let r = g
        .http
        .get(g.url(&format!("/Videos/{gid}/stream?static=true&ApiKey={t}")))
        .header("Authorization", auth(&t))
        .header("Cookie", "session=CLIENTCOOKIE")
        .header("X-Forwarded-For", "6.6.6.6")
        .header("X-Forwarded-Host", "evil.test")
        .header("X-Forwarded-Proto", "https")
        .header("X-Real-IP", "6.6.6.6")
        .header("X-Emby-Token", &t)
        .header("X-MediaBrowser-Token", &t)
        .header("Host", "evil.test")
        .header("Range", "bytes=0-1")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.status());
    for h in [
        "set-cookie",
        "x-server-secret",
        "access-control-allow-origin",
    ] {
        assert!(
            r.headers().get(h).is_none(),
            "{h} came back from the server"
        );
    }
    let rec = seen.all();
    assert_eq!(rec.len(), 1, "{rec:?}");
    let (_, up_uri, hdrs) = &rec[0];
    assert!(
        !up_uri.contains(&t),
        "the client token reached the server in the URL"
    );
    for (k, v) in hdrs {
        let k = k.to_ascii_lowercase();
        assert!(
            !k.starts_with("x-forwarded")
                && k != "x-real-ip"
                && k != "cookie"
                && !k.starts_with("x-emby")
                && !k.starts_with("x-mediabrowser"),
            "{k} was forwarded"
        );
        assert!(
            !v.contains(&t) || k == "never",
            "the client token reached the server in {k}"
        );
        if k == "host" {
            assert_eq!(
                *v, authority,
                "the server was addressed with the client's Host"
            );
        }
        if k == "authorization" {
            assert!(v.contains(r#"Token="UPTOKEN""#), "the server's own login");
        }
    }
    assert!(
        hdrs.iter().any(|(k, v)| k == "range" && v == "bytes=0-1"),
        "Range passes"
    );
    g.st.stop();
}

#[tokio::test]
async fn a_redirect_from_a_server_is_never_followed() {
    let (url, seen, evil_hits) = mock().await;
    let g = start(false, false).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev").await;
    let gid = g.st.ids.register_single("srvA", "realitem0").to_string();
    let r = g
        .http
        .get(g.url(&format!("/Videos/{gid}/stream.mkv")))
        .header("Authorization", auth(&t))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        502,
        "the gateway answers for the server, not the redirect"
    );
    assert!(r.headers().get("location").is_none());
    assert_eq!(seen.all().len(), 1);
    assert_eq!(
        *evil_hits.lock().unwrap(),
        0,
        "the other host was contacted"
    );
    g.st.stop();
}

#[tokio::test]
async fn crafted_ids_and_paths_never_leave_the_servers_route_templates() {
    let (url, seen, evil_hits) = mock().await;
    let g = start(true, true).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev").await;
    for p in [
        "/Videos/..%2f..%2fSystem%2fConfiguration/stream",
        "/Videos/http:%2f%2f127.0.0.1:1%2fx/stream",
        "/Videos/%2e%2e/stream",
        "/Videos/00000000000000000000000000000000/stream",
        "/Items/%2f%2fevil.test/Images/Primary",
        "/Items/..%5c..%5cetc/PlaybackInfo",
    ] {
        let s = g
            .http
            .get(g.url(p))
            .header("Authorization", auth(&t))
            .send()
            .await
            .unwrap()
            .status();
        assert!(s.is_client_error(), "{p}: {s}");
    }
    assert!(
        seen.all().is_empty(),
        "nothing was asked of the server: {:?}",
        seen.all()
    );
    assert_eq!(*evil_hits.lock().unwrap(), 0);
    g.st.stop();
}

// ---------------------------------------------------------------- write gating

/// Every route that can cause a POST or DELETE to a server, called as a signed-in client.
async fn fire_all_writers(g: &Gw, t: &str, gid: &str, sess: &str) {
    use reqwest::Method as M;
    let ev = json!({"ItemId": gid, "PositionTicks": 100_000_000i64, "IsPaused": false});
    for p in [
        "/Sessions/Playing",
        "/Sessions/Playing/Progress",
        "/Sessions/Playing/Stopped",
        "/Sessions/Playing/Ping",
    ] {
        g.call(M::POST, t, p, Some(ev.clone())).await;
    }
    for base in ["/UserPlayedItems", "/UserFavoriteItems"] {
        for m in [M::POST, M::DELETE] {
            g.call(m, t, &format!("{base}/{gid}"), None).await;
        }
    }
    for base in ["PlayedItems", "FavoriteItems"] {
        for m in [M::POST, M::DELETE] {
            g.call(m, t, &format!("/Users/u/{base}/{gid}"), None).await;
        }
    }
    g.call(
        M::POST,
        t,
        &format!("/UserItems/{gid}/UserData"),
        Some(json!({"Played": true, "IsFavorite": true, "PlaybackPositionTicks": 5})),
    )
    .await;
    g.call(
        M::POST,
        t,
        &format!("/Users/u/Items/{gid}/UserData"),
        Some(json!({"Played": true})),
    )
    .await;
    g.call(M::POST, t, &format!("/Sessions/{sess}/Playing/Pause"), None)
        .await;
    g.call(
        M::POST,
        t,
        &format!("/Sessions/{sess}/Playing?PlayCommand=PlayNow&ItemIds={gid}"),
        None,
    )
    .await;
    g.call(
        M::POST,
        t,
        &format!("/Sessions/{sess}/Command"),
        Some(json!({"Name": "VolumeUp"})),
    )
    .await;
    g.call(M::POST, t, &format!("/Sessions/{sess}/Command/Mute"), None)
        .await;
    g.call(
        M::POST,
        t,
        &format!("/Sessions/{sess}/Message"),
        Some(json!({"Text": "hi"})),
    )
    .await;
}

async fn session_id(g: &Gw, t: &str) -> String {
    let l: Vec<Value> = g
        .http
        .get(g.url("/Sessions"))
        .header("Authorization", auth(t))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    l.iter().find(|s| s["DeviceId"] == "mpv-1").unwrap()["Id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn with_server_changes_off_no_route_forwards_a_post_or_delete() {
    let (url, seen, _) = mock().await;
    // sync_progress on: only the master switch may stop the writes.
    let g = start(false, true).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev").await;
    let gid = g.st.ids.register_single("srvA", "realitem0").to_string();
    let sess = session_id(&g, &t).await;
    fire_all_writers(&g, &t, &gid, &sess).await;
    assert!(
        seen.writes().is_empty(),
        "written with the switch off: {:?}",
        seen.writes()
    );
    g.st.stop();
}

#[tokio::test]
async fn the_same_calls_do_reach_the_server_with_the_switches_on() {
    // The control for the test above: the table really exercises every forwarding path.
    let (url, seen, _) = mock().await;
    let g = start(true, true).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev").await;
    let gid = g.st.ids.register_single("srvA", "realitem0").to_string();
    let sess = session_id(&g, &t).await;
    fire_all_writers(&g, &t, &gid, &sess).await;
    let w = seen.writes();
    let has = |m: &str, p: &str| {
        w.iter().any(|r| {
            r.0 == m
                && r.1
                    .to_ascii_lowercase()
                    .starts_with(&p.to_ascii_lowercase())
        })
    };
    for (m, p) in [
        ("POST", "/Sessions/Playing"),
        ("POST", "/Sessions/Playing/Progress"),
        ("POST", "/Sessions/Playing/Stopped"),
        ("POST", "/UserPlayedItems/"),
        ("DELETE", "/UserPlayedItems/"),
        ("POST", "/UserFavoriteItems/"),
        ("DELETE", "/UserFavoriteItems/"),
        ("POST", "/Sessions/realsess1/Playing/Pause"),
        ("POST", "/Sessions/realsess1/Command"),
    ] {
        assert!(has(m, p), "{m} {p} never reached the server: {w:?}");
    }
    assert!(w.iter().all(|r| {
        r.2.iter()
            .any(|(k, v)| k == "authorization" && v.contains(r#"Token="UPTOKEN""#))
    }));
    g.st.stop();
}

// ---------------------------------------------------------------- login limits behind a spoofed proxy header

#[tokio::test]
async fn spoofed_forwarded_for_does_not_dodge_the_login_limit() {
    let g = start_cfg(
        GatewaySettings {
            port: 0,
            ..Default::default()
        },
        false,
        false,
        Limits::default(),
    )
    .await;
    let attempt = |pw: String, i: usize| {
        g.http
            .post(g.url("/Users/AuthenticateByName"))
            .header("X-Forwarded-For", format!("10.9.8.{i}"))
            .header("X-Real-IP", format!("10.9.7.{i}"))
            .json(&json!({"Username": "nocturne", "Pw": pw}))
            .send()
    };
    for i in 0..5 {
        assert_eq!(attempt("wrong".into(), i).await.unwrap().status(), 401);
    }
    assert_eq!(attempt(g.pw.clone(), 99).await.unwrap().status(), 429);
    g.st.stop();
}

// ---------------------------------------------------------------- WebSocket caps

async fn open_socket(
    g: &Gw,
    token: &str,
) -> Result<WebSocketStream<reqwest::Upgraded>, StatusCode> {
    let r = g
        .http
        .get(g.url("/socket"))
        .header("Authorization", auth(token))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();
    if r.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(r.status());
    }
    Ok(WebSocketStream::from_raw_socket(r.upgrade().await.unwrap(), Role::Client, None).await)
}

#[tokio::test]
async fn a_token_gets_four_sockets_and_oversize_frames_close_the_socket() {
    let g = start(false, false).await;
    let t = g.login("dev").await;
    let mut socks = vec![];
    for _ in 0..4 {
        socks.push(open_socket(&g, &t).await.expect("within the cap"));
    }
    assert_eq!(
        open_socket(&g, &t).await.err(),
        Some(StatusCode::TOO_MANY_REQUESTS)
    );
    // Another device is not affected by the first one's sockets.
    let t2 = g.login("dev2").await;
    assert!(open_socket(&g, &t2).await.is_ok());

    let mut s = socks.remove(0);
    let big = "x".repeat(64 * 1024);
    let _ = s.send(Message::text(big)).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match s.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return true,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await;
    assert_eq!(closed, Ok(true), "a 64 KiB frame ended the socket");
    g.st.stop();
}

// ---------------------------------------------------------------- binding

/// This machine's address on a real interface (the route to a documentation address; nothing is sent).
fn lan_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

async fn get_from(src: IpAddr, to: SocketAddr) -> Result<StatusCode, String> {
    let c = reqwest::Client::builder()
        .local_address(src)
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    c.get(format!("http://{to}/System/Info/Public"))
        .send()
        .await
        .map(|r| r.status())
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn local_mode_does_not_listen_on_the_lan_address() {
    let Some(ip) = lan_ip() else {
        println!("no non-loopback address here: skipped");
        return;
    };
    let g = start_cfg(
        GatewaySettings {
            port: 0,
            binding: Binding::Local,
            ..Default::default()
        },
        false,
        false,
        wide(),
    )
    .await;
    assert!(
        g.addrs.iter().all(|a| a.ip().is_loopback()),
        "{:?}",
        g.addrs
    );
    let port = g.addrs[0].port();
    let r = get_from(ip, SocketAddr::new(ip, port)).await;
    assert!(
        r.is_err(),
        "the loopback-only gateway answered on {ip}: {r:?}"
    );
    assert_eq!(
        get_from(
            "127.0.0.1".parse().unwrap(),
            SocketAddr::new("127.0.0.1".parse().unwrap(), port)
        )
        .await,
        Ok(StatusCode::OK)
    );
    g.st.stop();
}

#[tokio::test]
async fn lan_mode_serves_private_peers_and_refuses_the_rest_at_accept_time() {
    let Some(ip) = lan_ip() else {
        println!("no non-loopback address here: skipped");
        return;
    };
    let g = start_cfg(
        GatewaySettings {
            port: 0,
            binding: Binding::Lan,
            ..Default::default()
        },
        false,
        false,
        wide(),
    )
    .await;
    let port = g.addrs[0].port();
    let r = get_from(ip, SocketAddr::new(ip, port)).await;
    if core_gw::peer_allowed(Binding::Lan, ip) {
        assert_eq!(r, Ok(StatusCode::OK), "a private LAN peer is served");
    } else {
        assert!(
            r.is_err(),
            "a public peer is dropped before any bytes: {r:?}"
        );
    }
    // Public peers, by the same predicate the accept loop uses.
    for p in ["8.8.8.8", "203.0.113.9", "2001:db8::1", "100.128.0.1"] {
        assert!(!server::admit(&g.st.cfg(), p.parse().unwrap()), "{p}");
    }
    for p in [
        "127.0.0.1",
        "192.168.1.5",
        "10.0.0.2",
        "172.16.0.1",
        "169.254.1.1",
        "100.64.0.1",
        "fd00::1",
        "fe80::1",
        "::ffff:192.168.1.5",
    ] {
        assert!(server::admit(&g.st.cfg(), p.parse().unwrap()), "{p}");
    }
    g.st.stop();
}

// ---------------------------------------------------------------- local files

#[tokio::test]
async fn local_files_are_served_only_from_under_a_configured_root() {
    use std::os::unix::fs::symlink;
    let g = start(false, false).await;
    let d = tempfile::tempdir().unwrap();
    let (root, outside) = (d.path().join("root"), d.path().join("outside"));
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(root.join("sub/a.mp4"), b"a").unwrap();
    std::fs::write(outside.join("secret.mp4"), b"s").unwrap();
    symlink(outside.join("secret.mp4"), root.join("link.mp4")).unwrap();
    symlink(&outside, root.join("dir")).unwrap();
    g.st.set_upstreams(vec![Upstream {
        id: SourceId(3),
        label: "Local".into(),
        kind: UpKind::Local {
            roots: vec![root.clone()],
        },
        state: UpState::Ready,
    }]);
    let ok = |s: u32, p: &std::path::Path| {
        nocturne_gateway::stream::under_roots(&g.st, SourceId(s), p).is_some()
    };
    assert!(ok(3, &root.join("sub/a.mp4")));
    assert!(!ok(3, &root.join("link.mp4")), "a symlink out of the root");
    assert!(
        !ok(3, &root.join("dir/secret.mp4")),
        "a symlinked directory out of the root"
    );
    assert!(
        !ok(3, &root.join("sub/../../outside/secret.mp4")),
        "dot-dot out of the root"
    );
    assert!(!ok(3, &outside.join("secret.mp4")));
    assert!(!ok(3, std::path::Path::new("/etc/passwd")));
    assert!(!ok(9, &root.join("sub/a.mp4")), "an unknown source");
    g.st.stop();
}

// ---------------------------------------------------------------- session content

#[tokio::test]
async fn now_playing_shows_the_real_item_without_paths_and_user_data_news_is_pushed() {
    let (url, _, _) = mock().await;
    let g = start(false, false).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev").await;
    let gid = g.st.ids.register_single("srvA", "realitem0").to_string();
    let mut ws = open_socket(&g, &t).await.unwrap();
    let ev = json!({"ItemId": gid, "PositionTicks": 100_000_000i64});
    assert_eq!(
        g.call(reqwest::Method::POST, &t, "/Sessions/Playing", Some(ev))
            .await,
        204
    );
    let text = g
        .http
        .get(g.url("/Sessions"))
        .header("Authorization", auth(&t))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let l: Vec<Value> = serde_json::from_str(&text).unwrap();
    let me = l.iter().find(|s| s["DeviceId"] == "dev").unwrap();
    let np = &me["NowPlayingItem"];
    assert_eq!(np["Name"], "Demo Movie");
    assert_eq!(np["Type"], "Movie");
    assert_eq!(np["Id"], gid.as_str());
    assert_eq!(np["RunTimeTicks"], 6_000_000_000i64);
    assert_eq!(np["ImageTags"]["Primary"], "abc");
    for bad in ["/srv/media", "Path", "MediaSources", "UPTOKEN"] {
        assert!(!text.contains(bad), "{bad} in the session list");
    }
    // A mark pushes UserDataChanged (coalesced, within ~5 s).
    assert_eq!(
        g.call(
            reqwest::Method::POST,
            &t,
            &format!("/UserPlayedItems/{gid}"),
            None
        )
        .await,
        200
    );
    let got = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(Ok(Message::Text(m))) = ws.next().await {
                let v: Value = serde_json::from_str(m.as_str()).unwrap();
                if v["MessageType"] == "UserDataChanged" {
                    return v;
                }
            }
        }
    })
    .await
    .expect("UserDataChanged within 8 s");
    assert_eq!(got["Data"]["UserDataList"][0]["ItemId"], gid.as_str());
    assert_eq!(got["Data"]["UserDataList"][0]["Played"], true);
    g.st.stop();
}
