//! The storage module and the storage agent in process: an agent on a loopback port (ranged reads, create-only writes, moves, key and path-escape refusals), and the module's roots over the API with a fake mount table and a fake `gio` (nothing real is mounted).
use nocturne_core::gateway::Binding;
use nocturne_core::server::{Event, ServerConfig};
use nocturne_server::module::Registry;
use nocturne_server::modules::storage::{
    AgentConfig, AgentRootConfig, StorageConfig, StorageHandle,
};
use nocturne_server::modules::storage_agent::{serve, sha256_hex};
use nocturne_server::state::{Paths, Secrets, Shared};
use nocturne_storage::agent::AgentClient;
use serde_json::{Value, json};
use std::path::Path;

const KEY: &str = "nsa_test-key";

/// An agent exporting `dir/media` as `media`; returns its URL.
async fn agent(dir: &Path) -> String {
    std::fs::create_dir_all(dir.join("media/tv")).unwrap();
    let cfg = AgentConfig {
        binding: Binding::Local,
        port: 0,
        key_sha256: sha256_hex(KEY),
        roots: vec![AgentRootConfig {
            name: "media".into(),
            path: dir.join("media"),
        }],
    };
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { serve(&cfg, l).await });
    url
}

#[tokio::test]
async fn agent_reads_writes_moves_and_refuses() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("secret"), "no").unwrap();
    let url = agent(d.path()).await;
    let c = AgentClient::new(&url, KEY);

    let roots = c.roots().await.unwrap();
    assert_eq!(roots.len(), 1);
    assert!(roots[0].name == "media" && roots[0].writable && roots[0].space.unwrap().total > 0);

    c.write("media", "tv/Show/S01E01.mkv", b"0123456789".to_vec())
        .await
        .unwrap();
    let e = c.stat("media", "tv/Show/S01E01.mkv").await.unwrap();
    assert!(!e.dir && e.size == 10);
    assert!(
        c.write("media", "tv/Show/S01E01.mkv", b"x".to_vec())
            .await
            .unwrap_err()
            .contains("exists")
    );

    let whole = c.read("media", "tv/Show/S01E01.mkv", None).await.unwrap();
    assert_eq!(whole.status(), 200);
    assert_eq!(&whole.bytes().await.unwrap()[..], b"0123456789");
    let part = c
        .read("media", "tv/Show/S01E01.mkv", Some((2, Some(4))))
        .await
        .unwrap();
    assert_eq!(part.status(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 2-4/10");
    assert_eq!(&part.bytes().await.unwrap()[..], b"234");
    let tail = c
        .read("media", "tv/Show/S01E01.mkv", Some((7, None)))
        .await
        .unwrap();
    assert_eq!(&tail.bytes().await.unwrap()[..], b"789");
    assert!(
        c.read("media", "tv/Show/S01E01.mkv", Some((10, None)))
            .await
            .is_err()
    );

    // A streamed file body.
    let src = d.path().join("upload.bin");
    std::fs::write(&src, vec![7u8; 300_000]).unwrap();
    let f = tokio::fs::File::open(&src).await.unwrap();
    c.write("media", "big.bin", f).await.unwrap();
    assert_eq!(
        std::fs::metadata(d.path().join("media/big.bin"))
            .unwrap()
            .len(),
        300_000
    );

    c.rename("media", "tv/Show/S01E01.mkv", "tv/Show/Season 1/S01E01.mkv")
        .await
        .unwrap();
    assert!(d.path().join("media/tv/Show/Season 1/S01E01.mkv").is_file());
    assert!(
        c.rename("media", "big.bin", "tv/Show/Season 1/S01E01.mkv")
            .await
            .is_err()
    );
    let names: Vec<String> = c
        .list("media", "tv/Show")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["Season 1"]);

    // Escapes and unknown roots.
    assert!(c.read("media", "../secret", None).await.is_err());
    assert!(c.write("media", "../evil", b"x".to_vec()).await.is_err());
    assert!(c.rename("media", "big.bin", "../big.bin").await.is_err());
    assert!(c.list("other", "").await.is_err());
    assert!(!d.path().join("evil").exists() && d.path().join("media/big.bin").exists());

    // Wrong key, no key.
    let bad = AgentClient::new(&url, "nsa_wrong");
    assert_eq!(bad.roots().await.unwrap_err(), "the agent refused the key");
    let r = reqwest::get(format!("{url}/agent/v1/roots")).await.unwrap();
    assert_eq!(r.status(), 401);
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn call(method: &str, url: &str, token: &str, body: Option<Value>) -> (u16, Value) {
    let mut rb = reqwest::Client::new()
        .request(method.parse().unwrap(), url)
        .bearer_auth(token);
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let code = r.status().as_u16();
    (code, r.json().await.unwrap_or(Value::Null))
}

fn write_script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn module_roots_health_mount_watch_and_agents() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path();
    let info = t.join("mountinfo");
    let base = "22 1 0:21 / / rw - ext4 /dev/sda1 rw\n";
    let nas = t.join("nas");
    std::fs::create_dir_all(nas.join("Movies")).unwrap();
    let cifs = format!("40 22 0:50 / {} rw - cifs //nas/media rw\n", nas.display());
    std::fs::write(&info, format!("{base}{cifs}")).unwrap();
    let gio = t.join("gio");
    let gio_log = t.join("gio.log");
    write_script(
        &gio,
        &format!(
            "#!/bin/sh\necho \"$@\" > '{0}'\ncat >> '{0}'\n",
            gio_log.display()
        ),
    );

    let mut cfg = ServerConfig::default();
    cfg.http.port = free_port();
    cfg.set_section(
        "storage",
        &StorageConfig {
            enabled: true,
            gio: gio.clone(),
            mountinfo: info.clone(),
            ..StorageConfig::default()
        },
    )
    .unwrap();
    let sh = Shared::new(
        Paths::under(&t.join("server")),
        cfg,
        Secrets::memory(),
        Registry::new(nocturne_server::modules::all()),
    )
    .unwrap();
    let mut events = sh.subscribe();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(nocturne_server::http::run(sh.clone(), Some(tx)));
    let b = format!("http://{}/api/v1", rx.await.unwrap()[0]);
    let (_, v) = call(
        "POST",
        &format!("{b}/setup"),
        "",
        Some(json!({"username": "admin", "password": "correct horse", "code": nocturne_server::auth::setup_code(&sh.paths)})),
    )
    .await;
    let tok = v["token"].as_str().unwrap().to_string();
    let s = format!("{b}/storage");

    // A local root.
    std::fs::create_dir_all(t.join("local/films")).unwrap();
    let (code, v) = call(
        "POST",
        &format!("{s}/roots"),
        &tok,
        Some(json!({"kind": "local", "path": t.join("local"), "role": "library"})),
    )
    .await;
    assert_eq!(code, 200, "{v}");
    let local = v["id"].as_u64().unwrap();
    assert_eq!(
        call(
            "POST",
            &format!("{s}/roots"),
            &tok,
            Some(json!({"kind": "local", "path": "rel"}))
        )
        .await
        .0,
        400
    );

    // An SMB share already mounted (found in the mount table), with a login.
    let (code, v) = call(
        "POST",
        &format!("{s}/roots"),
        &tok,
        Some(
            json!({"kind": "smb", "url": "smb://NAS/media/Movies", "user": "u", "password": "pw"}),
        ),
    )
    .await;
    assert_eq!(code, 200, "{v}");
    let smb = v["id"].as_u64().unwrap();
    // NFS that is not mounted: instructions.
    let (code, v) = call(
        "POST",
        &format!("{s}/roots"),
        &tok,
        Some(json!({"kind": "nfs", "url": "box:/export/tv"})),
    )
    .await;
    assert_eq!(code, 409);
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("box:/export/tv  /mnt/tv  nfs")
    );

    let (_, roots) = call("GET", &format!("{s}/roots"), &tok, None).await;
    let by = |id: u64| {
        roots
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()
            .clone()
    };
    let l = by(local);
    assert_eq!(
        (l["role"].as_str(), l["available"].as_bool()),
        (Some("library"), Some(true))
    );
    assert!(l["health"]["writable"] == true && l["health"]["space"]["total"].as_u64() > Some(0));
    let m = by(smb);
    assert_eq!(
        m["path"].as_str().unwrap(),
        nas.join("Movies").to_str().unwrap()
    );
    assert!(m["health"]["mounted"] == true && m["available"] == true);

    let h = sh.ext::<StorageHandle>();
    assert_eq!(h.root_of(&nas.join("Movies/x.mkv")).unwrap().id, smb);
    assert_eq!(
        h.resolve(local, "films").await.unwrap(),
        t.join("local/films").canonicalize().unwrap()
    );
    assert!(h.resolve(local, "../nas").await.is_err());
    let (_, list) = call("GET", &format!("{s}/roots/{local}/browse"), &tok, None).await;
    assert_eq!(list[0]["name"], "films");
    assert_eq!(
        call(
            "GET",
            &format!("{s}/roots/{local}/browse?path=../"),
            &tok,
            None
        )
        .await
        .0,
        400
    );
    let (_, mounts) = call("GET", &format!("{s}/mounts"), &tok, None).await;
    assert_eq!(mounts.as_array().unwrap().len(), 1);

    // The share goes away: an event, the root unavailable, its health says so.
    std::fs::write(&info, base).unwrap();
    h.recheck(&sh);
    let lost = loop {
        if let Event::Module(e) = events.recv().await.unwrap() {
            break e;
        }
    };
    assert_eq!(
        (lost.module.as_str(), lost.kind.as_str()),
        ("storage", "mount_lost")
    );
    assert_eq!(lost.data["root"], smb);
    assert!(!h.available(smb) && h.available(local));
    assert!(h.resolve(smb, "x").await.is_err());
    let (_, m) = call("GET", &format!("{s}/roots/{smb}"), &tok, None).await;
    assert!(m["health"]["mounted"] == false && m["health"]["writable"] == false);

    // Mount it through (fake) GVfs: the login goes to gio on stdin.
    let (code, v) = call("POST", &format!("{s}/roots/{smb}/mount"), &tok, None).await;
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["mounted"], true);
    assert_eq!(
        std::fs::read_to_string(&gio_log).unwrap(),
        "mount smb://NAS/media/Movies\nu\n\npw\n"
    );
    std::fs::write(&info, format!("{base}{cifs}")).unwrap();
    h.recheck(&sh);
    assert!(h.available(smb));

    // An agent root.
    let url = agent(&t.join("remote")).await;
    let (code, _) = call(
        "POST",
        &format!("{s}/roots"),
        &tok,
        Some(json!({"kind": "agent", "url": url, "remote": "media", "key": "nsa_wrong"})),
    )
    .await;
    assert_eq!(code, 400);
    let (code, v) = call("POST", &format!("{s}/roots"), &tok, Some(json!({"kind": "agent", "url": url, "remote": "media", "key": KEY, "role": "downloads"}))).await;
    assert_eq!(code, 200, "{v}");
    let ag = v["id"].as_u64().unwrap();
    assert!(sh.secrets.get(&format!("storage:agent:{ag}")).is_some());
    let (_, m) = call("GET", &format!("{s}/roots/{ag}"), &tok, None).await;
    assert!(
        m["health"]["reachable"] == true && m["health"]["writable"] == true,
        "{m}"
    );
    let (_, list) = call("GET", &format!("{s}/roots/{ag}/browse"), &tok, None).await;
    assert_eq!(list[0]["name"], "tv");
    assert!(h.agent(ag).is_some() && h.resolve(ag, "").await.is_err());

    // Change and forget.
    let (code, _) = call(
        "PUT",
        &format!("{s}/roots/{ag}"),
        &tok,
        Some(json!({"label": "Box"})),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(h.root(ag).unwrap().label, "Box");
    assert_eq!(
        call("DELETE", &format!("{s}/roots/{ag}"), &tok, None)
            .await
            .0,
        200
    );
    assert!(h.root(ag).is_none() && sh.secrets.get(&format!("storage:agent:{ag}")).is_none());
    assert!(t.join("remote/media/tv").is_dir());
    sh.stop();
}
