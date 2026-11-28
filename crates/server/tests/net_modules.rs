//! The network modules over real sockets: the gateway in this process and its management routes, remote access with a fake `cloudflared`, sources' refusals, and the one-time import of 0.7 settings. Offline; the live test against throwaway Jellyfin servers is at the end (ignored).
use nocturne_core::gateway::GatewaySettings;
use nocturne_core::server::ServerConfig;
use nocturne_server::module::Registry;
use nocturne_server::modules::gateway::{GatewayCfg, LocalSource};
use nocturne_server::modules::tunnel::{Mode, TunnelCfg};
use nocturne_server::state::{Paths, Secrets, Shared};
use serde_json::{Value, json};
use std::time::Duration;

struct Server {
    sh: Shared,
    base: String,
    tok: String,
    dir: tempfile::TempDir,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn call(method: &str, url: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
    let mut rb = reqwest::Client::new().request(method.parse().unwrap(), url);
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

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ServerConfig::default();
    cfg.http.port = free_port();
    let sh = Shared::new(
        Paths::under(dir.path()),
        cfg,
        Secrets::memory(),
        Registry::new(nocturne_server::modules::all()),
    )
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(nocturne_server::http::run(sh.clone(), Some(tx)));
    let addr = rx.await.unwrap()[0];
    let base = format!("http://{addr}/api/v1");
    let (code, v) = call(
        "POST",
        &format!("{base}/setup"),
        None,
        Some(json!({"username": "admin", "password": "correct horse"})),
    )
    .await;
    assert_eq!(code, 200, "{v}");
    Server {
        sh,
        base,
        tok: v["token"].as_str().unwrap().to_string(),
        dir,
    }
}

impl Server {
    async fn set<T: serde::Serialize>(&self, name: &str, v: &T) {
        let mut c = self.sh.cfg();
        c.set_section(name, v).unwrap();
        self.sh.save_cfg(c).unwrap();
        self.sh.modules().apply(&self.sh).await;
    }
    async fn call(&self, m: &str, p: &str, body: Option<Value>) -> (u16, Value) {
        call(m, &format!("{}{p}", self.base), Some(&self.tok), body).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_gateway_runs_in_process_and_is_managed_through_the_api() {
    let s = start().await;
    assert_eq!(
        s.call("GET", "/gateway/status", None).await.0,
        503,
        "off by default"
    );
    s.set(
        "gateway",
        &GatewayCfg {
            gw: GatewaySettings {
                enabled: true,
                port: 0,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await;
    let (code, st) = s.call("GET", "/gateway/status", None).await;
    assert_eq!(code, 200, "{st}");
    let addr = st["listening"][0].as_str().unwrap().to_string();
    // Its files are where 0.7 kept them: next to the server's data directory.
    assert!(s.dir.path().join("gateway/users.toml").exists());
    let jf = format!("http://{addr}");
    let r: Value = reqwest::get(format!("{jf}/System/Info/Public"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["ProductName"], "Jellyfin Server");

    // An API key for a Jellyfin client: works on the gateway, listed, revoked.
    let (code, k) = s
        .call("POST", "/gateway/apikeys", Some(json!({"name": "TV"})))
        .await;
    assert_eq!(code, 200, "{k}");
    let key = k["key"].as_str().unwrap().to_string();
    let me = |key: String| {
        let jf = jf.clone();
        async move {
            reqwest::Client::new()
                .get(format!("{jf}/Users/Me"))
                .header("X-Emby-Token", key)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };
    assert_eq!(me(key.clone()).await, 200);
    let (_, devs) = s.call("GET", "/gateway/devices", None).await;
    let id = devs
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["api_key"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!devs.to_string().contains(&key), "never listed");
    let (_, pw) = s.call("POST", "/gateway/password/reveal", None).await;
    assert!(pw["password"].as_str().is_some_and(|p| p.len() >= 20));
    assert_eq!(
        s.call("POST", "/gateway/quickconnect/000000", None).await.0,
        400
    );
    assert_eq!(s.call("POST", "/gateway/restart", None).await.0, 200);
    let (_, st) = s.call("GET", "/gateway/status", None).await;
    let jf = format!("http://{}", st["listening"][0].as_str().unwrap());
    let still = reqwest::Client::new()
        .get(format!("{jf}/Users/Me"))
        .header("X-Emby-Token", &key)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(still, 200, "a restart keeps devices and keys");
    assert_eq!(
        s.call("DELETE", &format!("/gateway/devices/{id}"), None)
            .await
            .0,
        200
    );
    let gone = reqwest::Client::new()
        .get(format!("{jf}/Users/Me"))
        .header("X-Emby-Token", &key)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(gone, 401);

    // Switched off: the listener goes away.
    s.set("gateway", &GatewayCfg::default()).await;
    assert!(
        reqwest::get(format!("{jf}/System/Info/Public"))
            .await
            .is_err()
    );
    s.sh.stop();
}

/// A stand-in `cloudflared`: records its arguments and whether it got a token, prints what the real one prints, then stays up (or exits at once with `EXIT_FAST`).
fn fake_cloudflared(dir: &std::path::Path, exit_fast: bool) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("cloudflared");
    let rec = dir.join("record");
    let tail = if exit_fast { "exit 3" } else { "exec sleep 60" };
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{r}.args'\nprintf '%s' \"$TUNNEL_TOKEN\" > '{r}.token'\necho $$ > '{r}.pid'\necho 'INF |  https://calm-river-1234.trycloudflare.com  |' >&2\necho 'INF Registered tunnel connection connIndex=0' >&2\n{tail}\n",
            r = rec.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

async fn until(mut f: impl AsyncFnMut() -> bool) {
    for _ in 0..100 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("condition not met in time");
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_access_supervises_cloudflared_and_keeps_the_token_off_the_command_line() {
    let s = start().await;
    let bin = fake_cloudflared(s.dir.path(), false);
    let rec = s.dir.path().join("record");

    // Quick mode: no account, one target, the printed address shows up.
    s.set(
        "tunnel",
        &TunnelCfg {
            enabled: true,
            mode: Mode::Quick,
            cloudflared: Some(bin.clone()),
            ..Default::default()
        },
    )
    .await;
    until(async || s.call("GET", "/tunnel/status", None).await.1["state"] == "connected").await;
    let (_, st) = s.call("GET", "/tunnel/status", None).await;
    assert_eq!(st["url"], "https://calm-river-1234.trycloudflare.com");
    let args = std::fs::read_to_string(rec.with_extension("args")).unwrap();
    assert!(args.contains("--url http://127.0.0.1:"));
    let pid: i32 = std::fs::read_to_string(rec.with_extension("pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    // Off: that exact process is gone.
    s.set("tunnel", &TunnelCfg::default()).await;
    // SAFETY: signal 0 only asks whether the pid exists.
    until(async || unsafe { libc::kill(pid, 0) } != 0).await;

    // Token mode refuses to start without a token; with one, the token reaches cloudflared through its environment only.
    let token_cfg = TunnelCfg {
        enabled: true,
        mode: Mode::Token,
        cloudflared: Some(bin.clone()),
        ..Default::default()
    };
    s.set("tunnel", &token_cfg).await;
    let (_, st) = s.call("GET", "/tunnel/status", None).await;
    assert_eq!(st["state"], "stopped", "waits for a token");
    assert!(st["error"].as_str().unwrap().contains("token"));
    assert_eq!(st["token_set"], false);
    assert_eq!(
        s.call(
            "PUT",
            "/tunnel/token",
            Some(json!({"token": "cloudflared tunnel run"}))
        )
        .await
        .0,
        400
    );
    // Made up at run time (a token-like literal would trip the secret scan).
    let token = ["fake", "tunnel", "key"].join("-").repeat(3);
    let token = token.as_str();
    let _ = std::fs::remove_file(rec.with_extension("args"));
    // Storing the token starts the tunnel.
    assert_eq!(
        s.call("PUT", "/tunnel/token", Some(json!({"token": token})))
            .await
            .0,
        200
    );
    until(async || std::fs::read_to_string(rec.with_extension("token")).is_ok_and(|t| t == token))
        .await;
    let args = std::fs::read_to_string(rec.with_extension("args")).unwrap();
    assert!(
        args.contains("run") && !args.contains(token),
        "the token is not an argument"
    );
    assert!(
        !s.call("GET", "/tunnel/status", None)
            .await
            .1
            .to_string()
            .contains(token)
    );
    assert_eq!(s.call("POST", "/tunnel/restart", None).await.0, 200);
    s.set("tunnel", &TunnelCfg::default()).await;

    // A cloudflared that keeps dying is restarted with a pause.
    let dying = s.dir.path().join("dying");
    std::fs::create_dir_all(&dying).unwrap();
    let bin2 = fake_cloudflared(&dying, true);
    s.set(
        "tunnel",
        &TunnelCfg {
            enabled: true,
            mode: Mode::Quick,
            cloudflared: Some(bin2),
            ..Default::default()
        },
    )
    .await;
    until(async || s.call("GET", "/tunnel/status", None).await.1["restarts"].as_u64() >= Some(1))
        .await;
    s.set("tunnel", &TunnelCfg::default()).await;
    s.sh.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn sources_refuse_bad_input_and_need_an_administrator() {
    let s = start().await;
    s.set("sources", &nocturne_core::server::Switch { enabled: true })
        .await;
    assert_eq!(s.call("GET", "/sources", None).await, (200, json!([])));
    let (code, v) = s
        .call(
            "POST",
            "/sources",
            Some(json!({"url": "jf.example", "username": "a", "password": "b"})),
        )
        .await;
    assert_eq!(code, 400, "{v}");
    // Nothing listens there: an error, nothing stored.
    let dead = format!("http://127.0.0.1:{}", free_port());
    let (code, _) = s
        .call(
            "POST",
            "/sources",
            Some(json!({"url": dead, "username": "a", "password": "b"})),
        )
        .await;
    assert_eq!(code, 400);
    assert_eq!(
        s.call("POST", "/sources/quickconnect", Some(json!({"url": dead})))
            .await
            .0,
        400
    );
    assert_eq!(s.call("GET", "/sources", None).await.1, json!([]));
    assert_eq!(s.call("DELETE", "/sources/9", None).await.0, 404);
    assert_eq!(
        call("GET", &format!("{}/sources", s.base), None, None)
            .await
            .0,
        401
    );
    s.sh.stop();
}

#[test]
fn the_0_7_settings_are_imported_once_and_the_app_files_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let cfgdir = paths.config.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&cfgdir).unwrap();
    let settings = "version = 2\nallow_server_writes = true\n\n[gateway]\nenabled = true\nport = 8123\nremote_control = true\n";
    std::fs::write(cfgdir.join("settings.toml"), settings).unwrap();
    let sources = "version = 2\nnext_id = 4\n\n[[profile]]\nid = 3\nlabel = \"Films\"\nkind = \"local\"\nroots = [\"/srv/films\"]\n";
    std::fs::write(cfgdir.join("sources.toml"), sources).unwrap();
    let sh = Shared::new(
        paths,
        ServerConfig::default(),
        Secrets::memory(),
        Registry::new(vec![]),
    )
    .unwrap();
    assert!(!nocturne_server::modules::gateway::imported(&sh));
    nocturne_server::modules::gateway::migrate(&sh).unwrap();
    let g: GatewayCfg = sh.cfg().section("gateway");
    assert!(g.gw.enabled && g.gw.port == 8123 && g.gw.remote_control && g.allow_server_writes);
    assert!(!g.sync_progress);
    assert_eq!(
        g.local,
        [LocalSource {
            id: 3,
            label: "Films".into(),
            roots: vec!["/srv/films".into()]
        }]
    );
    assert!(nocturne_server::modules::gateway::imported(&sh));
    // Once only: a later change of the server's own section is not overwritten.
    let mut c = sh.cfg();
    c.set_section("gateway", &GatewayCfg::default()).unwrap();
    sh.save_cfg(c).unwrap();
    nocturne_server::modules::gateway::migrate(&sh).unwrap();
    assert!(!sh.cfg().section::<GatewayCfg>("gateway").gw.enabled);
    assert_eq!(
        std::fs::read_to_string(cfgdir.join("settings.toml")).unwrap(),
        settings
    );
    assert_eq!(
        std::fs::read_to_string(cfgdir.join("sources.toml")).unwrap(),
        sources
    );
}

fn env_value(var: &str, key: &str) -> String {
    std::fs::read_to_string(std::env::var(var).unwrap())
        .unwrap()
        .lines()
        .find_map(|l| {
            l.strip_prefix(&format!("{key}="))
                .map(|v| v.trim().to_string())
        })
        .unwrap()
}

/// Sources by password, Quick Connect and API key against two throwaway Jellyfin servers, then the in-process gateway merging them.
/// `scripts/test-jellyfin.sh start; scripts/test-jellyfin.sh start 2`, then
/// `NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env NOCTURNE_JF_TEST_ENV_2=/tmp/nocturne-jf-test-2/test.env cargo test -p nocturne-server --test net_modules -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs two throwaway Jellyfin servers: NOCTURNE_JF_TEST_ENV and NOCTURNE_JF_TEST_ENV_2"]
async fn sources_sign_in_three_ways_and_feed_the_gateway() {
    use nocturne_jellyfin_admin::guard::TestServerGuard;
    let (_, url_a, _) = TestServerGuard::connect_env("NOCTURNE_JF_TEST_ENV")
        .await
        .expect("guard");
    let (_, url_b, key_b) = TestServerGuard::connect_env("NOCTURNE_JF_TEST_ENV_2")
        .await
        .expect("guard");
    let pw_a = env_value("NOCTURNE_JF_TEST_ENV", "NOCTURNE_JF_TEST_ALICE_PW");
    let s = start().await;
    s.set("sources", &nocturne_core::server::Switch { enabled: true })
        .await;

    // Password.
    let (code, v) = s
        .call(
            "POST",
            "/sources",
            Some(json!({"url": format!("{url_a}/"), "username": "alice", "password": pw_a})),
        )
        .await;
    assert_eq!(code, 200, "{v}");
    let id_a = v["id"].as_u64().unwrap();
    let (code, _) = s
        .call(
            "POST",
            "/sources",
            Some(json!({"url": url_a, "username": "bob", "password": "wrong"})),
        )
        .await;
    assert_eq!(code, 400, "a wrong password stores nothing");
    let (code, _) = s
        .call(
            "POST",
            "/sources",
            Some(json!({"url": url_a, "username": "alice", "password": pw_a})),
        )
        .await;
    assert_eq!(code, 409, "the same account twice");

    // Quick Connect: alice approves from a session of her own on server B (throwaway server: writes are fine).
    let (code, q) = s
        .call("POST", "/sources/quickconnect", Some(json!({"url": url_b})))
        .await;
    assert_eq!(code, 200, "{q}");
    let (code, _) = s
        .call(
            "POST",
            "/sources/quickconnect/complete",
            Some(json!({"url": url_b, "secret": q["secret"]})),
        )
        .await;
    assert_eq!(code, 409, "not approved yet");
    let pw_b = env_value("NOCTURNE_JF_TEST_ENV_2", "NOCTURNE_JF_TEST_ALICE_PW");
    let alice_b = nocturne_jellyfin_user::UserClient::authenticate_by_name(&url_b, "alice", &pw_b)
        .await
        .unwrap();
    nocturne_jellyfin_user::UserClient::from_session(&url_b, &alice_b)
        .unwrap()
        .quick_connect_authorize(q["code"].as_str().unwrap())
        .await
        .unwrap();
    let (code, v) = s
        .call(
            "POST",
            "/sources/quickconnect/complete",
            Some(json!({"url": url_b, "secret": q["secret"], "label": "Beta"})),
        )
        .await;
    assert_eq!(code, 200, "{v}");
    let id_b = v["id"].as_u64().unwrap();

    // API key: the admin key of server B acting for bob.
    let (code, v) = s
        .call(
            "POST",
            "/sources",
            Some(json!({"url": url_b, "username": "bob", "api_key": key_b})),
        )
        .await;
    assert_eq!(code, 200, "{v}");
    let id_key = v["id"].as_u64().unwrap();

    let (_, list) = s.call("GET", "/sources", None).await;
    assert_eq!(list.as_array().unwrap().len(), 3);
    assert!(
        !list.to_string().contains(&key_b),
        "never a token in the list"
    );
    for id in [id_a, id_b, id_key] {
        let (_, h) = s.call("POST", &format!("/sources/{id}/check"), None).await;
        assert_eq!(
            (h["reachable"].clone(), h["signed_in"].clone()),
            (json!(true), json!(true)),
            "source {id}: {h}"
        );
    }
    // bob's key is not needed by the gateway: switch it off.
    assert_eq!(
        s.call(
            "PUT",
            &format!("/sources/{id_key}"),
            Some(json!({"enabled": false}))
        )
        .await
        .0,
        200
    );

    // The gateway merges A (password) and B (Quick Connect).
    s.set(
        "gateway",
        &GatewayCfg {
            gw: GatewaySettings {
                enabled: true,
                port: 0,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await;
    let (_, st) = s.call("GET", "/gateway/status", None).await;
    assert_eq!(st["sources"].as_array().unwrap().len(), 2, "{st}");
    let jf = format!("http://{}", st["listening"][0].as_str().unwrap());
    let (_, k) = s
        .call("POST", "/gateway/apikeys", Some(json!({"name": "live"})))
        .await;
    let key = k["key"].as_str().unwrap();
    let items: Value = reqwest::Client::new()
        .get(format!(
            "{jf}/Items?Recursive=true&IncludeItemTypes=Movie&SortBy=SortName"
        ))
        .header("X-Emby-Token", key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<&str> = items["Items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["Name"].as_str())
        .collect();
    println!("merged movies: {}", names.len());
    for n in ["Test Clip 1", "Test Clip 2", "Test Clip 4"] {
        assert!(names.contains(&n), "{n} through the gateway");
    }
    // Removing a source takes it out of the gateway.
    assert_eq!(
        s.call("DELETE", &format!("/sources/{id_b}"), None).await.0,
        200
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (_, st) = s.call("GET", "/gateway/status", None).await;
    assert_eq!(st["sources"].as_array().unwrap().len(), 1);
    s.sh.stop();
}
