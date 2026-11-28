//! Adversarial checks of the server's authentication over real sockets (docs/dev/SERVER_PLAN.md, "Security review 2026-11-28"). Each test is the proof of one finding: it failed before the fix.
use nocturne_core::server::ServerConfig;
use nocturne_server::module::Registry;
use nocturne_server::state::{Paths, Secrets, Shared};
use serde_json::{Value, json};

struct Server {
    sh: Shared,
    addr: std::net::SocketAddr,
    base: String,
    _dir: tempfile::TempDir,
}

async fn start_with(f: impl FnOnce(&mut ServerConfig)) -> Server {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = ServerConfig::default();
    cfg.http.port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    f(&mut cfg);
    let sh = Shared::new(
        Paths::under(d.path()),
        cfg,
        Secrets::memory(),
        Registry::new(vec![]),
    )
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(nocturne_server::http::run(sh.clone(), Some(tx)));
    let addr = rx.await.unwrap()[0];
    Server {
        sh,
        addr,
        base: format!("http://{addr}/api/v1"),
        _dir: d,
    }
}

async fn start() -> Server {
    start_with(|_| {}).await
}

async fn post(url: &str, headers: &[(&str, &str)], body: Value) -> (u16, Value) {
    let mut rb = reqwest::Client::new().post(url).json(&body);
    for (n, v) in headers {
        rb = rb.header(*n, *v);
    }
    let r = rb.send().await.unwrap();
    let code = r.status().as_u16();
    (code, r.json().await.unwrap_or(Value::Null))
}

/// First-run setup through the unix socket (the CLI's `user add`): the local admin needs no code.
async fn setup_locally(s: &Server) {
    let sock = s.sh.paths.socket().unwrap();
    // The socket is bound just after the TCP address is reported.
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (code, v) = tokio::task::spawn_blocking(move || {
        nocturne_server::cli::call(
            &sock,
            "POST",
            "/api/v1/setup",
            Some(&json!({"username": "admin", "password": "correct horse"})),
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(code, 200, "{v}");
}

/// Finding: anyone who reached the port first (a LAN peer, a visitor of the tunnel's address, another local user, a web page through DNS rebinding) could create the admin account. Setup over TCP now needs the one-time code the server keeps in a 0600 file.
#[tokio::test(flavor = "multi_thread")]
async fn setup_over_tcp_needs_the_code_only_the_owner_can_read() {
    let s = start().await;
    let url = format!("{}/setup", s.base);
    let creds = |code: Option<&str>| {
        let mut v = json!({"username": "mallory", "password": "correct horse"});
        if let Some(c) = code {
            v["code"] = json!(c);
        }
        v
    };
    // As cloudflared delivers it: from loopback, with Cloudflare's headers.
    let tunnel = [
        ("cf-connecting-ip", "203.0.113.9"),
        ("x-forwarded-for", "203.0.113.9"),
    ];
    assert_eq!(post(&url, &tunnel, creds(None)).await.0, 403);
    assert_eq!(post(&url, &[], creds(None)).await.0, 403);
    assert_eq!(post(&url, &[], creds(Some("0000"))).await.0, 403);

    let code = nocturne_server::auth::setup_code(&s.sh.paths).expect("a setup code is waiting");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(nocturne_server::auth::setup_code_path(&s.sh.paths))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "the code file is private");
    }
    let (status, v) = post(&url, &[], creds(Some(&code))).await;
    assert_eq!(status, 200, "{v}");
    assert!(nocturne_server::auth::setup_code(&s.sh.paths).is_none());
    // Done once: the same code does not make a second admin.
    assert_eq!(post(&url, &[], creds(Some(&code))).await.0, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_local_admin_sets_up_without_a_code() {
    let s = start().await;
    setup_locally(&s).await;
    assert!(nocturne_server::auth::setup_code(&s.sh.paths).is_none());
}

/// Finding: failed sign-ins were delayed one at a time only after Argon2 had run, and a right guess was answered at once, so parallel requests guessed as fast as the CPU hashed. Now at most 5 failures per (address, name) in 10 minutes, counted before the password is checked.
#[tokio::test(flavor = "multi_thread")]
async fn parallel_guesses_cannot_outrun_the_login_limit() {
    let s = start().await;
    setup_locally(&s).await;
    let url = format!("{}/auth/login", s.base);
    let tries = (0..12).map(|i| {
        let url = url.clone();
        tokio::spawn(async move {
            post(
                &url,
                &[],
                json!({"username": "admin", "password": format!("guess {i}")}),
            )
            .await
            .0
        })
    });
    let mut codes = vec![];
    for t in tries {
        codes.push(t.await.unwrap());
    }
    let checked = codes.iter().filter(|c| **c == 401).count();
    assert!(checked <= 5, "{checked} passwords were checked: {codes:?}");
    assert!(codes.iter().all(|c| *c == 401 || *c == 429), "{codes:?}");
    // Locked for this name from this address, even with the right password.
    let (code, _) = post(
        &url,
        &[],
        json!({"username": "admin", "password": "correct horse"}),
    )
    .await;
    assert_eq!(code, 429);
}

/// A forged `X-Forwarded-For` does not reset the limit unless the peer is the configured proxy; behind it (cloudflared on loopback with `trusted_proxy = "127.0.0.1"`) every visitor is counted on their own.
#[tokio::test(flavor = "multi_thread")]
async fn forwarded_for_counts_only_from_the_trusted_proxy() {
    let wrong = |ip: &str| {
        (
            [("x-forwarded-for", ip.to_string())],
            json!({"username": "admin", "password": "wrong one"}),
        )
    };
    let s = start().await;
    setup_locally(&s).await;
    let url = format!("{}/auth/login", s.base);
    for i in 0..5 {
        let (h, b) = wrong(&format!("198.51.100.{i}"));
        assert_eq!(post(&url, &[(h[0].0, &h[0].1)], b).await.0, 401);
    }
    let (h, b) = wrong("198.51.100.77");
    assert_eq!(post(&url, &[(h[0].0, &h[0].1)], b).await.0, 429);

    let s = start_with(|c| c.http.trusted_proxy = Some([127, 0, 0, 1].into())).await;
    setup_locally(&s).await;
    let url = format!("{}/auth/login", s.base);
    for _ in 0..5 {
        let (h, b) = wrong("198.51.100.1");
        assert_eq!(post(&url, &[(h[0].0, &h[0].1)], b).await.0, 401);
    }
    let (h, b) = wrong("198.51.100.1");
    assert_eq!(post(&url, &[(h[0].0, &h[0].1)], b).await.0, 429);
    let (h, b) = wrong("198.51.100.2");
    assert_eq!(post(&url, &[(h[0].0, &h[0].1)], b).await.0, 401);
}

/// Spellings of a protected path that could slip past the `/api/` prefix test of the auth layer never reach a handler.
#[tokio::test(flavor = "multi_thread")]
async fn no_spelling_of_a_route_skips_authentication() {
    use std::io::{Read, Write};
    let s = start().await;
    setup_locally(&s).await;
    for p in [
        "/api/v1/config",
        "//api/v1/config",
        "/api//v1/config",
        "/./api/v1/config",
        "/x/../api/v1/config",
        "/%61pi/v1/config",
        "/api/v1/%63onfig",
        "/API/v1/config",
        "/api/v1/config/",
        "/api/v1/config?x=/api/v1/health",
        "/api/v1/apikeys",
        "/api/v1/logs",
    ] {
        let addr = s.addr;
        let out = tokio::task::spawn_blocking(move || {
            let mut c = std::net::TcpStream::connect(addr).unwrap();
            write!(
                c,
                "GET {p} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut out = String::new();
            let _ = c.read_to_string(&mut out);
            out
        })
        .await
        .unwrap();
        let ok = out.starts_with("HTTP/1.1 401")
            || out.starts_with("HTTP/1.1 404")
            || out.starts_with("HTTP/1.1 400")
            || (out.starts_with("HTTP/1.1 200") && out.contains("text/html"));
        assert!(ok, "{p}: {}", out.lines().next().unwrap_or(""));
    }
}

/// Finding: the listener had no connection limit, so one peer could hold every socket and file descriptor. Now 64 per address (256 in all), like the gateway.
#[tokio::test(flavor = "multi_thread")]
async fn one_peer_cannot_hold_every_connection() {
    use std::io::{Read, Write};
    let s = start().await;
    let addr = s.addr;
    let out = tokio::task::spawn_blocking(move || {
        let held: Vec<_> = (0..nocturne_gateway::limits::MAX_CONNECTIONS_PER_IP)
            .map(|_| std::net::TcpStream::connect(addr).unwrap())
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let _ = write!(
            c,
            "GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        );
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        drop(held);
        out
    })
    .await
    .unwrap();
    assert!(!out.starts_with("HTTP/1.1 200"), "{out}");
    // The slots come back when the connections close.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let r = reqwest::get(format!("{}/health", s.base)).await.unwrap();
    assert_eq!(r.status(), 200);
}

/// Finding: `PUT /config` let any admin session choose the programs the server runs (`tunnel.cloudflared`, `storage.gio`), so a stolen session token (through a tunnel, say) became a shell on the machine. Only the local admin (unix socket, or editing `server.toml`) may change them now.
#[tokio::test(flavor = "multi_thread")]
async fn a_remote_session_cannot_choose_a_program_to_run() {
    let s = start().await;
    let code = nocturne_server::auth::setup_code(&s.sh.paths).unwrap();
    let (_, v) = post(
        &format!("{}/setup", s.base),
        &[],
        json!({"username": "admin", "password": "correct horse", "code": code}),
    )
    .await;
    let tok = v["token"].as_str().unwrap().to_string();
    let c = reqwest::Client::new();
    let mut cfg: Value = c
        .get(format!("{}/config", s.base))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for (sec, key) in [("tunnel", "cloudflared"), ("storage", "gio")] {
        let mut bad = cfg.clone();
        bad[sec] = json!({ key: "/bin/sh" });
        let r = c
            .put(format!("{}/config", s.base))
            .bearer_auth(&tok)
            .json(&bad)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403, "{sec}.{key}");
    }
    // Other changes still work over TCP, and the local admin may set the program.
    cfg["requests"]["auto_approve"] = json!(false);
    let r = c
        .put(format!("{}/config", s.base))
        .bearer_auth(&tok)
        .json(&cfg)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    cfg["tunnel"] = json!({"cloudflared": "/usr/bin/cloudflared"});
    let sock = s.sh.paths.socket().unwrap();
    let (code, v) = tokio::task::spawn_blocking(move || {
        nocturne_server::cli::call(&sock, "PUT", "/api/v1/config", Some(&cfg))
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(code, 200, "{v}");
}

/// Finding: `state.redb` (password hashes, token hashes, the library) was created with the umask, readable by other users of the machine wherever the home directory is not private. Now 0600.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_store_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let s = start().await;
    let mode = std::fs::metadata(s.sh.paths.db())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
}

/// Finding: the storage agent client followed redirects, and `X-Api-Key` (unlike `Authorization`) survives a redirect to another host, so an agent address that redirects handed the key to whatever host it named.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_key_never_follows_a_redirect() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let hits = std::sync::Arc::new(AtomicU32::new(0));
    let h = hits.clone();
    let other = axum::Router::new().fallback(move || {
        h.fetch_add(1, Ordering::SeqCst);
        async { "[]" }
    });
    let ol = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let to = format!("http://{}/stolen", ol.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(ol, other).await });
    let agent = axum::Router::new().fallback(move || {
        let to = to.clone();
        async move { axum::response::Redirect::temporary(&to) }
    });
    let al = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", al.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(al, agent).await });
    let r = nocturne_storage::agent::AgentClient::new(&url, "nsa_secret")
        .roots()
        .await;
    assert!(r.is_err(), "{r:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}
