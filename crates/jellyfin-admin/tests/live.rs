//! Live tests against the THROWAWAY Jellyfin only (see `guard`). Run:
//!   scripts/test-jellyfin.sh start
//!   NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-admin -- --ignored --test-threads=1
//! Output policy: counts and booleans only.

use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_admin::{AdminClient, ConfigPatch, Confirmed};
use std::time::Duration;
use tokio::sync::RwLock;

/// Normal tests share the lock; the restart test takes it exclusively.
static SERVER: RwLock<()> = RwLock::const_new(());

fn yes() -> Confirmed {
    Confirmed::i_have_asked_the_user()
}

/// The throwaway server URL (loopback, fixed port; `connect()` verified the server behind it).
fn test_url() -> String {
    format!(
        "http://127.0.0.1:{}",
        nocturne_jellyfin_admin::guard::TEST_PORT
    )
}

/// Unauthenticated login as a fake user (creates a device/session on success).
async fn login(name: &str, pw: &str, device: &str) -> bool {
    let auth = format!(
        "MediaBrowser Client=\"fake\", Device=\"fakedev\", DeviceId=\"{device}\", Version=\"1\""
    );
    reqwest::Client::new()
        .post(format!("{}/Users/AuthenticateByName", test_url()))
        .header("Authorization", auth)
        .json(&serde_json::json!({"Username": name, "Pw": pw}))
        .send()
        .await
        .unwrap()
        .status()
        .is_success()
}

fn uniq(p: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{p}-{n}")
}

async fn wait_for<F: std::future::Future<Output = bool>>(mut f: impl FnMut() -> F) -> bool {
    for _ in 0..40 {
        if f().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

#[tokio::test]
#[ignore]
async fn reads() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let info = c.system_info().await.unwrap();
    assert!(!info.version.is_empty() && info.web_socket_port_number == 8097);
    let log = c.activity_log(0, 5).await.unwrap();
    assert!(!log.items.is_empty() && log.total_record_count >= log.items.len() as u32);
    let page2 = c.activity_log(1, 1).await.unwrap();
    assert_eq!(page2.items.len(), 1);
    let users = c.users().await.unwrap();
    for n in ["admin", "alice", "bob"] {
        assert!(users.iter().any(|u| u.name == n), "user {n}");
    }
    let admin = users.iter().find(|u| u.name == "admin").unwrap();
    assert!(admin.policy.is_administrator);
    let detail = c.user(&admin.id).await.unwrap();
    assert_eq!(detail.name, "admin");
    assert!(!detail.policy.extra.is_empty());
    let tasks = c.scheduled_tasks().await.unwrap();
    assert!(
        tasks.len() > 3
            && tasks
                .iter()
                .all(|t| !t.id.is_empty() && !t.state.is_empty())
    );
    assert_eq!(
        c.scheduled_task(&tasks[0].id).await.unwrap().id,
        tasks[0].id
    );
    let libs = c.virtual_folders().await.unwrap();
    assert!(
        libs.iter()
            .any(|l| l.name == "Test Movies" && !l.item_id.is_empty())
    );
    assert!(!c.plugins().await.unwrap().is_empty());
    let files = c.log_files().await.unwrap();
    assert!(!files.is_empty());
    let tail = c.log_tail(&files[0].name, 5).await.unwrap();
    assert!(!tail.is_empty() && tail.lines().count() <= 5);
    c.branding().await.unwrap();
    assert!(c.server_config().await.unwrap().ui_culture.is_some());
    assert!(!c.api_keys().await.unwrap().is_empty());
    c.sessions().await.unwrap();
    c.devices().await.unwrap();
    assert!(c.item_counts().await.unwrap().movie_count <= 1000);
    let net = c.network_config().await.unwrap();
    assert_eq!(net.internal_http_port, 8097);
}

#[tokio::test]
#[ignore]
async fn user_lifecycle() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let name = uniq("tmpuser");
    let u = c.create_user(&name, "pw-initial-1").await.unwrap();
    assert_eq!(u.name, name);
    c.set_user_disabled(&u.id, true).await.unwrap();
    assert!(c.user(&u.id).await.unwrap().policy.is_disabled);
    c.set_user_disabled(&u.id, false).await.unwrap();
    assert!(!c.user(&u.id).await.unwrap().policy.is_disabled);
    c.set_user_admin(&u.id, true).await.unwrap();
    let p = c.user(&u.id).await.unwrap().policy;
    assert!(
        p.is_administrator && !p.extra.is_empty(),
        "other policy fields survive"
    );
    c.set_user_admin(&u.id, false).await.unwrap();
    assert!(!c.user(&u.id).await.unwrap().policy.is_administrator);
    c.set_user_password(&u.id, "pw-changed-2", yes())
        .await
        .unwrap();
    let dev = uniq("pwdev");
    assert!(
        !login(&name, "pw-initial-1", &dev).await,
        "old password rejected"
    );
    assert!(
        login(&name, "pw-changed-2", &dev).await,
        "new password accepted"
    );
    c.reset_user_password(&u.id, yes()).await.unwrap();
    assert!(
        !login(&name, "pw-changed-2", &dev).await,
        "password cleared"
    );
    assert!(
        login(&name, "", &dev).await,
        "empty password accepted after reset"
    );
    c.delete_device(&dev, yes()).await.unwrap();
    c.delete_user(&u.id, yes()).await.unwrap();
    assert!(!c.users().await.unwrap().iter().any(|x| x.id == u.id));
}

#[tokio::test]
#[ignore]
async fn task_start_stop() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let t = c
        .scheduled_tasks()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key == "RefreshLibrary")
        .expect("scan task");
    c.start_task(&t.id).await.unwrap();
    // tiny library: it may already be finished; either way the call must succeed
    let _ = c.stop_task(&t.id).await;
    assert!(wait_for(|| async { c.scheduled_task(&t.id).await.unwrap().state == "Idle" }).await);
}

#[tokio::test]
#[ignore]
async fn library_refresh() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    c.refresh_library().await.unwrap();
    let lib = c
        .virtual_folders()
        .await
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Test Movies")
        .unwrap();
    c.refresh_library_folder(&lib.item_id).await.unwrap();
    assert!(
        wait_for(|| async {
            c.virtual_folders()
                .await
                .unwrap()
                .iter()
                .all(|l| l.refresh_status.as_deref().is_none_or(|s| s == "Idle"))
        })
        .await
    );
    // unknown library id is a clean error, not a panic
    assert!(
        c.refresh_library_folder("00000000000000000000000000000000")
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore]
async fn api_key_create_delete() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let app = uniq("nocturne-admin-test");
    let before = c.api_keys().await.unwrap().len();
    let key = c.create_api_key(&app).await.unwrap();
    assert_eq!(c.api_keys().await.unwrap().len(), before + 1);
    assert!(!format!("{key:?}").contains(&key.access_token));
    // the new key works...
    let cfg = nocturne_jellyfin_admin::Config {
        base_url: test_url(),
        token: key.access_token.clone(),
    };
    let k = AdminClient::new(cfg).unwrap();
    assert!(k.system_info().await.is_ok());
    // ...until deleted
    c.delete_api_key(&key.access_token, yes()).await.unwrap();
    assert_eq!(c.api_keys().await.unwrap().len(), before);
    assert!(k.system_info().await.is_err());
}

#[tokio::test]
#[ignore]
async fn session_and_device() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let (name, dev) = (uniq("sessuser"), uniq("dev"));
    let u = c.create_user(&name, "pw-session-1").await.unwrap();
    // Log in as the fake user from a fake device to create a device + session.
    assert!(login(&name, "pw-session-1", &dev).await);

    assert!(c.devices().await.unwrap().iter().any(|d| d.id == dev));
    let s = c
        .sessions()
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.device_id == dev)
        .expect("session");
    assert_eq!(s.user_name.as_deref(), Some(name.as_str()));
    // the fake client cannot be remote-controlled; the call must not panic (error or ok both fine)
    println!(
        "message ok: {}",
        c.send_session_message(&s.id, "hi", "test", 1000)
            .await
            .is_ok()
    );
    println!("stop ok: {}", c.stop_session_playback(&s.id).await.is_ok());

    c.delete_device(&dev, yes()).await.unwrap();
    assert!(!c.devices().await.unwrap().iter().any(|d| d.id == dev));
    assert!(
        !c.sessions()
            .await
            .unwrap()
            .iter()
            .any(|s| s.device_id == dev)
    );
    c.delete_user(&u.id, yes()).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn server_config_update() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let orig = c.server_config().await.unwrap();
    let n_keys = orig.extra.len();
    c.set_server_config(&ConfigPatch {
        log_file_retention_days: Some(7),
        server_name: Some("nocturne-test".into()),
        ..Default::default()
    })
    .await
    .unwrap();
    let now = c.server_config().await.unwrap();
    assert_eq!(now.log_file_retention_days, Some(7));
    assert_eq!(now.server_name.as_deref(), Some("nocturne-test"));
    assert_eq!(
        now.ui_culture, orig.ui_culture,
        "untouched fields unchanged"
    );
    assert_eq!(
        now.extra.len(),
        n_keys,
        "unknown fields survive the round trip"
    );
    c.set_server_config(&ConfigPatch {
        log_file_retention_days: orig.log_file_retention_days,
        server_name: Some(orig.server_name.clone().unwrap_or_default()),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(
        c.server_config().await.unwrap().log_file_retention_days,
        orig.log_file_retention_days
    );
}

#[tokio::test]
#[ignore]
async fn restart_server_comes_back() {
    let _g = SERVER.write().await;
    let c = TestServerGuard::connect().await.unwrap();
    c.restart_server(yes()).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut back = false;
    for _ in 0..60 {
        if c.system_info().await.is_ok() {
            back = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(
        back,
        "throwaway server did not come back after restart (run scripts/test-jellyfin.sh start)"
    );
}

/// Shuts the throwaway down for real, checks it is gone, then starts it again with the script.
#[tokio::test]
#[ignore]
async fn shutdown_server_stops_it() {
    let _g = SERVER.write().await;
    let c = TestServerGuard::connect().await.unwrap();
    c.shutdown_server(yes()).await.unwrap();
    let mut gone = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if c.system_info().await.is_err() {
            gone = true;
            break;
        }
    }
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/test-jellyfin.sh"
    );
    // The process lingers briefly while shutting down; `start` must not see it as running.
    for _ in 0..30 {
        let st = std::process::Command::new(script)
            .arg("status")
            .output()
            .unwrap();
        if !st.status.success() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let up = std::process::Command::new(script)
        .arg("start")
        .status()
        .unwrap();
    assert!(up.success(), "could not start the throwaway again");
    assert!(gone, "server still answered after shutdown");
    let mut back = false;
    for _ in 0..30 {
        if c.system_info().await.is_ok() {
            back = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(back, "throwaway did not come back");
}
