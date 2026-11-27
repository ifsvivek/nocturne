//! In-process tests: the real server on an ephemeral loopback port, driven over HTTP. No network beyond loopback, no keyring, no servers.
use nocturne_core::gateway::{self as gw, Binding, GatewaySettings, Request, Response};
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::limits::{Buckets, Limits};
use nocturne_gateway::server::{self, Access};
use nocturne_gateway::state::State;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

struct Gw {
    st: State,
    base: String,
    http: reqwest::Client,
    pw: String,
    dir: tempfile::TempDir,
    run: tokio::task::JoinHandle<std::io::Result<()>>,
}

fn paths(dir: &Path) -> Paths {
    Paths {
        data: dir.join("data"),
        runtime: Some(dir.join("run")),
        settings: Some(dir.join("settings.toml")),
        sources: None,
    }
}

async fn start_with(gws: GatewaySettings) -> Gw {
    start_limited(gws, Limits::default()).await
}

async fn start_limited(gws: GatewaySettings, limits: Limits) -> Gw {
    let dir = tempfile::tempdir().unwrap();
    let p = paths(dir.path());
    let cfg = Config {
        gw: gws,
        ..Default::default()
    };
    let st = State::open_with(p, cfg, limits).unwrap();
    let pw = st.users.take_unseen().unwrap();
    let run = tokio::spawn(server::run(st.clone()));
    let addrs = tokio::time::timeout(Duration::from_secs(5), st.wait_listening())
        .await
        .unwrap();
    let base = format!("http://127.0.0.1:{}", addrs[0].port());
    Gw {
        st,
        base,
        http: reqwest::Client::new(),
        pw,
        dir,
        run,
    }
}

async fn start() -> Gw {
    start_with(GatewaySettings {
        port: 0,
        ..Default::default()
    })
    .await
}

const AUTH: &str = r#"MediaBrowser Client="Test", Device="Rig", DeviceId="rig-1", Version="1""#;

impl Gw {
    fn url(&self, p: &str) -> String {
        format!("{}{p}", self.base)
    }

    async fn login_as(&self, device_id: &str) -> String {
        let r = self
            .http
            .post(self.url("/Users/AuthenticateByName"))
            .header("Authorization", format!(r#"MediaBrowser Client="Test", Device="Rig", DeviceId="{device_id}", Version="1""#))
            .json(&json!({"Username": "nocturne", "Pw": self.pw}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let v: Value = r.json().await.unwrap();
        v["AccessToken"].as_str().unwrap().to_string()
    }

    async fn login(&self) -> String {
        self.login_as("rig-1").await
    }

    async fn status_with_token(&self, path: &str, token: &str) -> StatusCode {
        self.http
            .get(self.url(path))
            .header("Authorization", format!(r#"MediaBrowser Token="{token}""#))
            .send()
            .await
            .unwrap()
            .status()
    }

    fn stop(self) {
        self.st.stop();
    }
}

#[tokio::test]
async fn sign_in_then_call_a_protected_route() {
    let g = start().await;
    // Public routes need nothing.
    let v: Value = g
        .http
        .get(g.url("/System/Info/Public"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["ProductName"], "Jellyfin Server");
    assert_eq!(v["Version"], gw::ADVERTISED_VERSION);
    assert_eq!(v["Id"].as_str().unwrap().len(), 32);
    assert_eq!(
        g.http
            .get(g.url("/System/Ping"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        g.http
            .post(g.url("/System/Ping"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let users: Value = g
        .http
        .get(g.url("/Users/Public"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(users, json!([]));

    // Protected without a token, with a wrong one.
    assert_eq!(
        g.http
            .get(g.url("/Users/Me"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(g.status_with_token("/Users/Me", &"0".repeat(64)).await, 401);

    // Wrong password, right password.
    let bad = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .json(&json!({"Username": "nocturne", "Pw": "nope"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 401);
    let r = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .header("X-Emby-Authorization", AUTH)
        .json(&json!({"Username": "Nocturne", "Pw": g.pw}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    let token = v["AccessToken"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 64);
    assert_eq!(v["ServerId"], g.st.users.server_id());
    assert_eq!(v["User"]["Policy"]["IsAdministrator"], false);
    assert_eq!(v["SessionInfo"]["DeviceId"], "rig-1");

    // Every accepted token spelling reaches the protected route.
    for (name, req) in [
        (
            "Authorization",
            g.http
                .get(g.url("/Users/Me"))
                .header("Authorization", format!(r#"MediaBrowser Token="{token}""#)),
        ),
        (
            "X-Emby-Authorization",
            g.http.get(g.url("/Users/Me")).header(
                "X-Emby-Authorization",
                format!(r#"MediaBrowser Token="{token}""#),
            ),
        ),
        (
            "X-Emby-Token",
            g.http
                .get(g.url("/Users/Me"))
                .header("X-Emby-Token", &token),
        ),
        (
            "X-MediaBrowser-Token",
            g.http
                .get(g.url("/Users/Me"))
                .header("X-MediaBrowser-Token", &token),
        ),
        (
            "ApiKey",
            g.http.get(g.url(&format!("/Users/Me?ApiKey={token}"))),
        ),
        (
            "api_key",
            g.http.get(g.url(&format!("/Users/Me?api_key={token}"))),
        ),
    ] {
        let r = req.send().await.unwrap();
        assert_eq!(r.status(), 200, "{name}");
        assert_eq!(r.headers()["referrer-policy"], "no-referrer");
        let me: Value = r.json().await.unwrap();
        assert_eq!(me["Name"], "nocturne");
        assert_eq!(me["Policy"]["IsAdministrator"], false);
    }
    let by_id = g
        .status_with_token(&format!("/Users/{}", g.st.users.user_id()), &token)
        .await;
    assert_eq!(by_id, 200);
    assert_eq!(
        g.status_with_token("/Users/00000000000000000000000000000000", &token)
            .await,
        404
    );

    // The server answered nothing that contains the token's hash or the password.
    let body = g
        .http
        .get(g.url("/System/Info"))
        .header("X-Emby-Token", &token)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!body.contains(&g.pw) && !body.contains(&token));

    // Logout revokes this token only.
    let other = g.login_as("rig-2").await;
    let out = g
        .http
        .post(g.url("/Sessions/Logout"))
        .header("X-Emby-Token", &token)
        .send()
        .await
        .unwrap();
    assert_eq!(out.status(), 204);
    assert_eq!(g.status_with_token("/Users/Me", &token).await, 401);
    assert_eq!(g.status_with_token("/Users/Me", &other).await, 200);
    g.stop();
}

fn fill_params(path: &str) -> String {
    path.split('/')
        .map(|s| match s {
            s if s.starts_with("{*") => "a".to_string(),
            s if s.starts_with('{') => "00000000000000000000000000000000".to_string(),
            s => s.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Every registered route carries an access class (the registry cannot take one without); this proves the class is enforced: no route except the listed public ones answers without a token, for every method.
#[tokio::test]
async fn routes_are_gated() {
    // Hundreds of requests from one address: the rate limits are not what is under test here.
    let wide = Limits {
        ip: Buckets::new(1e9, 1e9),
        token: Buckets::new(1e9, 1e9),
        ..Default::default()
    };
    let g = start_limited(
        GatewaySettings {
            port: 0,
            ..Default::default()
        },
        wide,
    )
    .await;
    let (_, table) = server::build(&g.st);
    assert!(table.len() >= 15, "the table lists the routes");
    let public: Vec<_> = table
        .iter()
        .filter(|r| r.access == Access::Public)
        .map(|r| r.path)
        .collect();
    // The only routes that may answer without a token: what clients call before they can sign in.
    let allowed_public = [
        "/System/Info/Public",
        "/System/Ping",
        "/Branding/Configuration",
        "/Branding/Css",
        "/Branding/Css.css",
        "/Users/Public",
        "/Users/AuthenticateByName",
        "/Users/AuthenticateWithQuickConnect",
        "/QuickConnect/Enabled",
        "/QuickConnect/Initiate",
        "/QuickConnect/Connect",
    ];
    for p in &public {
        assert!(
            allowed_public.contains(p),
            "{p} is Public but not on the pre-login list"
        );
    }
    for r in &table {
        let url = g.url(&fill_params(r.path));
        for method in [
            reqwest::Method::GET,
            reqwest::Method::POST,
            reqwest::Method::DELETE,
        ] {
            let anon = g
                .http
                .request(method.clone(), &url)
                .send()
                .await
                .unwrap()
                .status();
            match r.access {
                Access::Public => assert_ne!(anon, 401, "{method} {} is public", r.path),
                Access::Auth | Access::Remote => {
                    assert_eq!(anon, 401, "{method} {} must need a token", r.path)
                }
            }
            // A fresh token per call: some routes (logout) end the token they are called with.
            let token = g.login().await;
            let with = g
                .http
                .request(method.clone(), &url)
                .header("X-Emby-Token", &token)
                .send()
                .await
                .unwrap()
                .status();
            assert_ne!(with, 401, "{method} {} rejected a good token", r.path);
            match r.access {
                // Remote control is off by default: authenticated but refused.
                Access::Remote => assert_eq!(with, 403, "{method} {}", r.path),
                _ => assert_ne!(with, 403, "{method} {}", r.path),
            }
        }
    }
    g.stop();
}

#[tokio::test]
async fn unknown_routes_are_404_even_with_a_token() {
    let g = start().await;
    let token = g.login().await;
    for p in [
        "/Library/Refresh",
        "/Users",
        "/System/Configuration",
        "/Plugins",
        "/ScheduledTasks",
        "/Devices",
        "/Startup/User",
        "/Items/../Users",
    ] {
        for t in [None, Some(&token)] {
            let mut r = g.http.get(g.url(p));
            if let Some(t) = t {
                r = r.header("X-Emby-Token", t);
            }
            assert_eq!(r.send().await.unwrap().status(), 404, "{p}");
        }
    }
    g.stop();
}

#[tokio::test]
async fn quick_connect_pairs_without_a_password() {
    let g = start().await;
    assert_eq!(
        g.http
            .get(g.url("/QuickConnect/Enabled"))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap(),
        json!(true)
    );
    let q: Value = g
        .http
        .post(g.url("/QuickConnect/Initiate"))
        .header(
            "Authorization",
            r#"MediaBrowser Client="TV", Device="Living room", DeviceId="tv-1", Version="2""#,
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let (secret, code) = (
        q["Secret"].as_str().unwrap().to_string(),
        q["Code"].as_str().unwrap().to_string(),
    );
    assert_eq!((q["Authenticated"].clone(), code.len()), (json!(false), 6));
    // Not approved: no token, and Authorize needs a signed-in caller.
    let try_login = || {
        g.http
            .post(g.url("/Users/AuthenticateWithQuickConnect"))
            .json(&json!({"Secret": secret}))
            .send()
    };
    assert_eq!(try_login().await.unwrap().status(), 401);
    assert_eq!(
        g.http
            .post(g.url(&format!("/QuickConnect/Authorize?code={code}")))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        g.http
            .get(g.url("/QuickConnect/Connect?secret=nope"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    // The pending code shows in the status the app polls.
    let (resp, _) = nocturne_gateway::control::handle(&g.st, Request::Status).await;
    let Response::Status(s) = resp else {
        panic!("status")
    };
    assert_eq!(
        (
            s.pending_codes[0].code.clone(),
            s.pending_codes[0].device.clone()
        ),
        (code.clone(), "Living room".to_string())
    );
    // An already signed-in client approves.
    let admin = g.login().await;
    let ok: Value = g
        .http
        .post(g.url(&format!("/QuickConnect/Authorize?code={code}")))
        .header("X-Emby-Token", &admin)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ok, json!(true));
    let c: Value = g
        .http
        .get(g.url(&format!("/QuickConnect/Connect?secret={secret}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(c["Authenticated"], true);
    let r = try_login().await.unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    let tv = v["AccessToken"].as_str().unwrap();
    assert_eq!(v["SessionInfo"]["DeviceId"], "tv-1");
    assert_eq!(g.status_with_token("/Users/Me", tv).await, 200);
    assert_eq!(
        try_login().await.unwrap().status(),
        401,
        "a secret works once"
    );
    g.stop();
}

#[tokio::test]
async fn login_is_rate_limited_even_for_the_right_password() {
    let g = start().await;
    let attempt = |pw: String| {
        g.http
            .post(g.url("/Users/AuthenticateByName"))
            .json(&json!({"Username": "nocturne", "Pw": pw}))
            .send()
    };
    for _ in 0..5 {
        assert_eq!(attempt("wrong".into()).await.unwrap().status(), 401);
    }
    let r = attempt(g.pw.clone()).await.unwrap();
    assert_eq!(r.status(), 429);
    assert!(r.headers().contains_key("retry-after"));
    g.stop();
}

#[tokio::test]
async fn body_limit_and_bad_json() {
    let g = start().await;
    let big = vec![b'a'; 2 << 20];
    let r = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .body(big)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 413);
    let r = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    g.stop();
}

#[tokio::test]
async fn display_prefs_and_bitrate_test() {
    let g = start().await;
    let t = g.login().await;
    let get = |p: &str| g.http.get(g.url(p)).header("X-Emby-Token", &t).send();
    let d: Value = get("/DisplayPreferences/usersettings?client=emby")
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(d["Id"], "usersettings");
    let r = g
        .http
        .post(g.url("/DisplayPreferences/usersettings?client=emby"))
        .header("X-Emby-Token", &t)
        .json(&json!({"SortBy": "DateCreated", "CustomPrefs": {"a": "b"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    let d: Value = get("/DisplayPreferences/usersettings?client=emby")
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        (d["SortBy"].as_str(), d["CustomPrefs"]["a"].as_str()),
        (Some("DateCreated"), Some("b"))
    );
    let other: Value = get("/DisplayPreferences/usersettings?client=other")
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(other["SortBy"], "SortName");
    let b = get("/Playback/BitrateTest?Size=1000")
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(b.len(), 1000);
    let b = get("/Playback/BitrateTest?Size=999999999")
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(b.len(), 5 << 20);
    g.stop();
}

#[cfg(unix)]
#[tokio::test]
async fn control_socket_drives_the_gateway() {
    use std::os::unix::fs::PermissionsExt;
    let g = start().await;
    let sock = g.dir.path().join("run").join("gateway.sock");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&sock), 0o600);
    assert_eq!(mode(sock.parent().unwrap()), 0o700);
    let ask = |r: Request| {
        let s = sock.clone();
        async move {
            tokio::task::spawn_blocking(move || gw::request_at(&s, &r, Duration::from_secs(5)))
                .await
                .unwrap()
                .unwrap()
        }
    };

    let Response::Status(s) = ask(Request::Status).await else {
        panic!()
    };
    assert_eq!(
        (s.binding, s.devices, s.user_name.as_str()),
        (Binding::Local, 0, "nocturne")
    );
    assert!(
        s.address.starts_with("http://127.0.0.1:") && !s.listening.is_empty() && s.error.is_none()
    );

    let t1 = g.login_as("d1").await;
    let _t2 = g.login_as("d2").await;
    let Response::Tokens(list) = ask(Request::Tokens).await else {
        panic!()
    };
    assert_eq!(list.len(), 2);
    let d1 = list.iter().find(|t| t.device_id == "d1").unwrap();
    assert_eq!(d1.client, "Test");
    assert!(
        !serde_json::to_string(&list).unwrap().contains(&t1),
        "the token is never listed"
    );
    assert_eq!(ask(Request::Revoke(d1.id.clone())).await, Response::Ok);
    assert!(matches!(
        ask(Request::Revoke(d1.id.clone())).await,
        Response::Err(_)
    ));
    assert_eq!(g.status_with_token("/Users/Me", &t1).await, 401);

    // Quick Connect approval from the app.
    let q: Value = g
        .http
        .post(g.url("/QuickConnect/Initiate"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(matches!(
        ask(Request::ApproveQuickConnect("000000".into())).await,
        Response::Err(_)
    ));
    assert_eq!(
        ask(Request::ApproveQuickConnect(
            q["Code"].as_str().unwrap().into()
        ))
        .await,
        Response::Ok
    );

    // Regenerating the password invalidates every token and the old password.
    assert!(
        matches!(ask(Request::RevealPassword).await, Response::Err(_)),
        "already taken in start()"
    );
    let Response::Password(new) = ask(Request::RegeneratePassword).await else {
        panic!()
    };
    assert_ne!(new, g.pw);
    let Response::Tokens(list) = ask(Request::Tokens).await else {
        panic!()
    };
    assert!(list.is_empty());
    let r = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .json(&json!({"Username": "nocturne", "Pw": g.pw}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .json(&json!({"Username": "nocturne", "Pw": new}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // A line that is not a request gets an error, not a crash.
    let s2 = sock.clone();
    let out = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        let mut c = std::os::unix::net::UnixStream::connect(s2).unwrap();
        c.write_all(b"{\"cmd\":\"format_disk\"}\n").unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        out
    })
    .await
    .unwrap();
    assert!(out.contains("bad request"), "{out}");

    assert_eq!(ask(Request::Stop).await, Response::Ok);
    tokio::time::timeout(Duration::from_secs(5), g.run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!sock.exists(), "the socket is removed on stop");
}

#[cfg(unix)]
#[test]
fn a_second_instance_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let p = paths(d.path());
    let first = nocturne_gateway::control::acquire_lock(&p).unwrap();
    assert!(first.is_some());
    let e = nocturne_gateway::control::acquire_lock(&p).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);
    drop(first);
    assert!(
        nocturne_gateway::control::acquire_lock(&p)
            .unwrap()
            .is_some(),
        "free again once the first exits"
    );
}

#[tokio::test]
async fn reload_applies_settings() {
    let g = start().await;
    let t = g.login().await;
    // Remote control off: the session routes would answer 403 (checked for every Remote route in routes_are_gated); on: the gate opens.
    std::fs::write(
        g.dir.path().join("settings.toml"),
        "[gateway]\nport = 0\nremote_control = true\n",
    )
    .unwrap();
    g.st.reload().await.unwrap();
    assert!(g.st.cfg().gw.remote_control);
    let _ = t;
    // A listener setting changes: rebind on a new ephemeral port, the old one stops answering.
    let old = g.st.bound()[0].port();
    std::fs::write(
        g.dir.path().join("settings.toml"),
        "[gateway]\nport = 0\nannounce = false\n",
    )
    .unwrap();
    g.st.reload().await.unwrap();
    let new = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(a) = g.st.bound().first()
                && a.port() != old
            {
                return a.port();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        g.http
            .get(format!("http://127.0.0.1:{new}/System/Ping"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(
        reqwest::get(format!("http://127.0.0.1:{old}/System/Ping"))
            .await
            .is_err()
    );
    // A setting the gateway must refuse leaves the running configuration alone.
    std::fs::write(
        g.dir.path().join("settings.toml"),
        "[gateway]\nbinding = \"remote\"\n",
    )
    .unwrap();
    assert!(g.st.reload().await.is_err());
    assert_eq!(g.st.cfg().gw.binding, Binding::Local);
    g.stop();
}

#[tokio::test]
async fn remote_mode_will_not_start_without_a_certificate() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        gw: GatewaySettings {
            binding: Binding::Remote,
            port: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let st = State::open(paths(dir.path()), cfg).unwrap();
    let e = server::run(st).await.unwrap_err();
    assert!(e.to_string().contains("certificate"), "{e}");
}

/// Remote mode with a real certificate (made with the `openssl` binary; skipped where it is missing): HTTPS works, plain HTTP does not, HSTS is set.
#[tokio::test]
async fn remote_mode_serves_https() {
    let dir = tempfile::tempdir().unwrap();
    let (c, k) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    let made = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
            "-addext",
            "basicConstraints=critical,CA:FALSE",
        ])
        .arg("-keyout")
        .arg(&k)
        .arg("-out")
        .arg(&c)
        .output();
    if !made.is_ok_and(|o| o.status.success()) {
        eprintln!("openssl not available: TLS end-to-end test skipped");
        return;
    }
    let cfg = Config {
        gw: GatewaySettings {
            binding: Binding::Remote,
            port: 0,
            cert_path: Some(c.clone()),
            key_path: Some(k),
            ..Default::default()
        },
        ..Default::default()
    };
    let st = State::open(paths(dir.path()), cfg).unwrap();
    let run = tokio::spawn(server::run(st.clone()));
    let port = tokio::time::timeout(Duration::from_secs(5), st.wait_listening())
        .await
        .unwrap()[0]
        .port();
    let cert = reqwest::Certificate::from_pem(&std::fs::read(&c).unwrap()).unwrap();
    let http = reqwest::Client::builder()
        .add_root_certificate(cert)
        .resolve("localhost", ([127, 0, 0, 1], port).into())
        .build()
        .unwrap();
    let r = http
        .get(format!("https://localhost:{port}/System/Info/Public"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers().contains_key("strict-transport-security"));
    assert!(
        reqwest::get(format!("http://127.0.0.1:{port}/System/Ping"))
            .await
            .is_err(),
        "cleartext on the TLS port fails"
    );
    st.stop();
    let _ = tokio::time::timeout(Duration::from_secs(5), run).await;
}

/// matchit 0.8 accepts a different parameter name at the same position, so modules need not agree on names.
#[tokio::test]
async fn path_parameter_names_may_differ() {
    use axum::routing::get;
    let dir = tempfile::tempdir().unwrap();
    let st = State::open(paths(dir.path()), Config::default()).unwrap();
    let st1 = st.clone();
    let ok = std::panic::catch_unwind(move || {
        server::Routes::new(st1)
            .add("/Users/{userId}", Access::Auth, get(|| async {}))
            .add("/Users/{userId}/Items", Access::Auth, get(|| async {}))
            .add("/Users/Me", Access::Auth, get(|| async {}))
            .finish()
    });
    assert!(ok.is_ok());
    let clash = std::panic::catch_unwind(move || {
        server::Routes::new(st)
            .add("/Users/{userId}", Access::Auth, get(|| async {}))
            .add("/Users/{id}/Items", Access::Auth, get(|| async {}))
            .finish()
    });
    assert!(clash.is_ok());
}
