//! The core API end to end over real sockets: first-run setup, sign-in, sessions, API keys, the unix-socket local admin, module gating and config sections.
use nocturne_core::server::{ServerConfig, Switch};
use nocturne_server::module::{ApiRouter, BoxFut, Module, Registry};
use nocturne_server::state::{Paths, Secrets, Shared};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU32, Ordering};

/// A module that counts its starts and answers one route.
#[derive(Default)]
struct Probe {
    starts: AtomicU32,
}

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("/ping", || async { "pong" })
    }
    fn start<'a>(&'a self, _: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
    fn stop<'a>(&'a self, _: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async {})
    }
}

struct Server {
    sh: Shared,
    base: String,
    _dir: tempfile::TempDir,
    probe: std::sync::Arc<Probe>,
}

async fn start() -> Server {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = ServerConfig::default();
    cfg.http.port = 1; // replaced below; validate() refuses 0
    let probe = std::sync::Arc::new(Probe::default());
    let sh = Shared::new(
        Paths::under(d.path()),
        cfg,
        Secrets::memory(),
        Registry::new(vec![probe.clone()]),
    )
    .unwrap();
    let mut c = sh.cfg();
    c.http.port = free_port();
    sh.save_cfg(c).unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(nocturne_server::http::run(sh.clone(), Some(tx)));
    let addr = rx.await.unwrap()[0];
    Server {
        sh,
        base: format!("http://{addr}/api/v1"),
        _dir: d,
        probe,
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn call(method: &str, url: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
    let c = reqwest::Client::new();
    let mut rb = c.request(method.parse().unwrap(), url);
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let code = r.status().as_u16();
    (code, r.json().await.unwrap_or(Value::Null))
}

/// One HTTP/1.1 request over the unix socket (what the CLI does).
fn unix_get(sock: &std::path::Path, path: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(sock).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_login_keys_local_admin_and_gating() {
    let s = start().await;
    let b = &s.base;
    assert_eq!(
        call("GET", &format!("{b}/health"), None, None).await,
        (200, json!({"ok": true}))
    );
    assert_eq!(
        call("GET", &format!("{b}/setup"), None, None).await.1["needed"],
        true
    );
    assert_eq!(call("GET", &format!("{b}/status"), None, None).await.0, 401);
    assert_eq!(
        call(
            "POST",
            &format!("{b}/setup"),
            None,
            Some(json!({"username": "ad", "password": "short"}))
        )
        .await
        .0,
        400
    );
    let (code, v) = call(
        "POST",
        &format!("{b}/setup"),
        None,
        Some(json!({"username": "admin", "password": "correct horse"})),
    )
    .await;
    assert_eq!(code, 200, "{v}");
    let tok = v["token"].as_str().unwrap().to_string();
    assert!(tok.starts_with("nst_") && v["user"]["admin"] == true);
    // Setup only once.
    let again = call(
        "POST",
        &format!("{b}/setup"),
        None,
        Some(json!({"username": "x", "password": "another one"})),
    )
    .await;
    assert_eq!(again.0, 409);

    let (code, st) = call("GET", &format!("{b}/status"), Some(&tok), None).await;
    assert_eq!(code, 200);
    assert_eq!(st["setup_needed"], false);
    assert_eq!(
        st["modules"][0],
        json!({"name": "probe", "enabled": false, "running": false, "error": null})
    );

    // Wrong password: 401 after the delay; right one: a second session.
    let bad = call(
        "POST",
        &format!("{b}/auth/login"),
        None,
        Some(json!({"username": "admin", "password": "nope nope"})),
    )
    .await;
    assert_eq!(bad.0, 401);
    let (_, v) = call(
        "POST",
        &format!("{b}/auth/login"),
        None,
        Some(json!({"username": "ADMIN", "password": "correct horse"})),
    )
    .await;
    let tok2 = v["token"].as_str().unwrap().to_string();
    let (_, sessions) = call("GET", &format!("{b}/auth/sessions"), Some(&tok2), None).await;
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    assert_eq!(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["current"] == true)
            .count(),
        1
    );

    // API keys: by header X-Api-Key, listed without the key, revocable.
    let (_, k) = call(
        "POST",
        &format!("{b}/apikeys"),
        Some(&tok),
        Some(json!({"name": "cli"})),
    )
    .await;
    let key = k["key"].as_str().unwrap().to_string();
    let r = reqwest::Client::new()
        .get(format!("{b}/auth/me"))
        .header("X-Api-Key", &key)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let (_, keys) = call("GET", &format!("{b}/apikeys"), Some(&key), None).await;
    assert!(keys[0].get("key").is_none() && keys[0]["name"] == "cli");
    let id = k["id"].as_str().unwrap();
    assert_eq!(
        call("DELETE", &format!("{b}/apikeys/{id}"), Some(&tok), None)
            .await
            .0,
        200
    );
    assert_eq!(
        call("GET", &format!("{b}/auth/me"), Some(&key), None)
            .await
            .0,
        401
    );

    // Logout revokes only that session.
    assert_eq!(
        call("POST", &format!("{b}/auth/logout"), Some(&tok2), None)
            .await
            .0,
        200
    );
    assert_eq!(
        call("GET", &format!("{b}/auth/me"), Some(&tok2), None)
            .await
            .0,
        401
    );
    assert_eq!(
        call("GET", &format!("{b}/auth/me"), Some(&tok), None)
            .await
            .0,
        200
    );

    // The unix socket: local admin without a credential.
    let sock = s.sh.paths.socket().unwrap();
    let out = tokio::task::spawn_blocking(move || unix_get(&sock, "/api/v1/auth/me"))
        .await
        .unwrap();
    assert!(
        out.starts_with("HTTP/1.1 200") && out.contains("\"local\":true"),
        "{out}"
    );

    // The CLI's client: the local admin creates an API key without any credential.
    let sock = s.sh.paths.socket().unwrap();
    let (code, k) = tokio::task::spawn_blocking(move || {
        nocturne_server::cli::call(
            &sock,
            "POST",
            "/api/v1/apikeys",
            Some(&json!({"name": "from cli"})),
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(code, 200, "{k}");
    assert!(k["key"].as_str().unwrap().starts_with("nsk_"));

    // A module's routes: 503 while off, served once its section is enabled through PUT /config.
    assert_eq!(
        call("GET", &format!("{b}/probe/ping"), Some(&tok), None)
            .await
            .0,
        503
    );
    let mut cfg = s.sh.cfg();
    cfg.set_section("probe", &Switch { enabled: true }).unwrap();
    let (code, saved) = call(
        "PUT",
        &format!("{b}/config"),
        Some(&tok),
        Some(serde_json::to_value(&cfg).unwrap()),
    )
    .await;
    assert_eq!(code, 200, "{saved}");
    assert_eq!(saved["probe"]["enabled"], true);
    let r = reqwest::Client::new()
        .get(format!("{b}/probe/ping"))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap();
    assert_eq!(r.text().await.unwrap(), "pong");
    assert_eq!(s.probe.starts.load(Ordering::SeqCst), 1);
    // Saving the same config again does not restart it; the file holds the section.
    call(
        "PUT",
        &format!("{b}/config"),
        Some(&tok),
        Some(serde_json::to_value(&cfg).unwrap()),
    )
    .await;
    assert_eq!(s.probe.starts.load(Ordering::SeqCst), 1);
    assert!(
        std::fs::read_to_string(&s.sh.paths.config)
            .unwrap()
            .contains("[probe]")
    );

    // Unknown API paths are JSON 404s; other paths get the dashboard (or its placeholder).
    assert_eq!(
        call("GET", &format!("{b}/nothing"), Some(&tok), None)
            .await
            .0,
        404
    );
    let page = reqwest::get(b.replace("/api/v1", "/")).await.unwrap();
    assert_eq!(page.status(), 200);

    // Password change revokes sessions.
    let (code, _) = call(
        "PUT",
        &format!("{b}/auth/password"),
        Some(&tok),
        Some(json!({"current": "correct horse", "new": "battery staple"})),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(
        call("GET", &format!("{b}/auth/me"), Some(&tok), None)
            .await
            .0,
        401
    );
    s.sh.stop();
}
