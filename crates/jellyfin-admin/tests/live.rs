//! Live tests against the THROWAWAY Jellyfin only (see `guard`). Run:
//!   scripts/test-jellyfin.sh start
//!   NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-admin -- --ignored --test-threads=1
//! Output policy: counts and booleans only.

use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_admin::{AdminClient, ConfigPatch, Confirmed, NetworkEdit, TaskTrigger};
use serde_json::json;
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
        nocturne_jellyfin_admin::guard::test_port()
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
    assert!(
        !info.version.is_empty()
            && info.web_socket_port_number == nocturne_jellyfin_admin::guard::test_port()
    );
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
    assert_eq!(
        net.internal_http_port,
        nocturne_jellyfin_admin::guard::test_port()
    );
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
async fn branding_roundtrip() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let orig = c.branding().await.unwrap();
    c.set_branding(Some("Hello from tests"), Some("body{margin:0}"), None)
        .await
        .unwrap();
    let b = c.branding().await.unwrap();
    assert_eq!(b.login_disclaimer.as_deref(), Some("Hello from tests"));
    assert_eq!(b.custom_css.as_deref(), Some("body{margin:0}"));
    c.set_branding(
        Some(orig.login_disclaimer.as_deref().unwrap_or("")),
        Some(orig.custom_css.as_deref().unwrap_or("")),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        c.branding().await.unwrap().custom_css.unwrap_or_default(),
        orig.custom_css.unwrap_or_default()
    );
}

#[tokio::test]
#[ignore]
async fn network_safe_edits() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let orig = c.network_config().await.unwrap();
    // refused before sending
    assert!(
        c.set_network(&NetworkEdit::HttpPort(0), yes())
            .await
            .is_err()
    );
    assert!(
        c.set_network(&NetworkEdit::LanNetworks(vec!["bogus".into()]), yes())
            .await
            .is_err()
    );
    c.set_network(
        &NetworkEdit::LanNetworks(vec!["192.168.77.0/24".into()]),
        yes(),
    )
    .await
    .unwrap();
    c.set_network(
        &NetworkEdit::RemoteAccess(!orig.enable_remote_access),
        yes(),
    )
    .await
    .unwrap();
    let n = c.network_config().await.unwrap();
    assert_eq!(n.local_network_subnets, ["192.168.77.0/24"]);
    assert_eq!(n.enable_remote_access, !orig.enable_remote_access);
    assert_eq!(
        n.internal_http_port, orig.internal_http_port,
        "ports untouched"
    );
    assert_eq!(n.local_network_addresses, orig.local_network_addresses);
    c.set_network(
        &NetworkEdit::LanNetworks(orig.local_network_subnets.clone()),
        yes(),
    )
    .await
    .unwrap();
    c.set_network(&NetworkEdit::RemoteAccess(orig.enable_remote_access), yes())
        .await
        .unwrap();
    let n = c.network_config().await.unwrap();
    assert_eq!(n.local_network_subnets, orig.local_network_subnets);
    assert_eq!(n.enable_remote_access, orig.enable_remote_access);
}

#[tokio::test]
#[ignore]
async fn task_triggers_edit() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let t = c
        .scheduled_tasks()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key == "DeleteCacheFiles")
        .expect("a task to edit");
    let before = t.triggers.clone();
    c.add_task_trigger(&t.id, TaskTrigger::weekly("friday", "04:45").unwrap())
        .await
        .unwrap();
    let now = c.scheduled_task(&t.id).await.unwrap().triggers;
    assert_eq!(now.len(), before.len() + 1);
    assert_eq!(now.last().unwrap().describe(), "Friday at 04:45");
    let ix = now.len() - 1;
    c.replace_task_trigger(&t.id, ix, TaskTrigger::every_minutes(120).unwrap())
        .await
        .unwrap();
    assert_eq!(
        c.scheduled_task(&t.id).await.unwrap().triggers[ix].describe(),
        "Every 2 h"
    );
    c.delete_task_trigger(&t.id, ix, yes()).await.unwrap();
    let after = c.scheduled_task(&t.id).await.unwrap().triggers;
    assert_eq!(after, before, "back to the original list");
    assert!(c.delete_task_trigger(&t.id, 99, yes()).await.is_err());
}

#[tokio::test]
#[ignore]
async fn trickplay_edit() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let orig = c.server_config().await.unwrap();
    let q = orig.extra["TrickplayOptions"]["JpegQuality"].clone();
    assert!(
        c.set_trickplay_field("JpegQuality", json!(1000))
            .await
            .is_err()
    );
    c.set_trickplay_field("JpegQuality", json!(61))
        .await
        .unwrap();
    c.set_trickplay_field("WidthResolutions", json!([240, 320]))
        .await
        .unwrap();
    let t = c.server_config().await.unwrap().extra["TrickplayOptions"].clone();
    assert_eq!(
        (t["JpegQuality"].clone(), t["WidthResolutions"].clone()),
        (json!(61), json!([240, 320]))
    );
    assert_eq!(
        t["TileWidth"], orig.extra["TrickplayOptions"]["TileWidth"],
        "others untouched"
    );
    c.set_trickplay_field("JpegQuality", q).await.unwrap();
    c.set_trickplay_field(
        "WidthResolutions",
        orig.extra["TrickplayOptions"]["WidthResolutions"].clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        c.server_config().await.unwrap().extra["TrickplayOptions"],
        orig.extra["TrickplayOptions"]
    );
}

#[tokio::test]
#[ignore]
async fn library_add_edit_remove() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let name = uniq("Tmp Library");
    let dir = |n: &str| format!("/tmp/nocturne-jf-live-{n}");
    let (d1, d2) = (dir("a"), dir("b"));
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();
    c.add_library(&name, "homevideos", &d1).await.unwrap();
    let find = || async {
        c.virtual_folders()
            .await
            .unwrap()
            .into_iter()
            .find(|l| l.name == name)
    };
    let lib = find().await.expect("library created");
    assert_eq!(lib.collection_type.as_deref(), Some("homevideos"));
    assert_eq!(lib.locations, std::slice::from_ref(&d1));
    c.add_library_path(&name, &d2).await.unwrap();
    assert_eq!(find().await.unwrap().locations.len(), 2);
    c.set_library_option(&lib.item_id, "EnableRealtimeMonitor", json!(true))
        .await
        .unwrap();
    c.set_library_option(&lib.item_id, "AutomaticRefreshIntervalDays", json!(7))
        .await
        .unwrap();
    let o = find().await.unwrap().library_options;
    assert_eq!(
        (
            o["EnableRealtimeMonitor"].clone(),
            o["AutomaticRefreshIntervalDays"].clone()
        ),
        (json!(true), json!(7))
    );
    assert_eq!(
        o["PathInfos"].as_array().unwrap().len(),
        2,
        "paths survive an option edit"
    );
    c.remove_library_path(&name, &d2, yes()).await.unwrap();
    assert_eq!(find().await.unwrap().locations, std::slice::from_ref(&d1));
    c.remove_library(&name, yes()).await.unwrap();
    assert!(find().await.is_none());
    assert!(
        d1.starts_with("/tmp") && std::path::Path::new(&d1).exists(),
        "media folders are never deleted"
    );
    std::fs::remove_dir_all(&d1).ok();
    std::fs::remove_dir_all(&d2).ok();
    assert!(c.add_library("x", "movies", "relative").await.is_err());
}

/// Installs a small catalog plugin into the throwaway (needs internet on the server), restarts
/// it, disables/enables the plugin, then uninstalls it. Exclusive: it restarts the server.
#[tokio::test]
#[ignore]
async fn plugin_install_disable_uninstall() {
    let _g = SERVER.write().await;
    let c = TestServerGuard::connect().await.unwrap();
    let cat = c.packages().await.unwrap();
    let pkg = cat
        .iter()
        .find(|p| p.name == "Local Intros")
        .expect("catalog entry");
    assert!(!pkg.versions.is_empty());
    c.install_package(pkg).await.unwrap();
    let restart = |c: AdminClient| async move {
        c.restart_server(yes()).await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        for _ in 0..60 {
            if c.system_info().await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        panic!("throwaway did not come back");
    };
    restart(c.clone()).await;
    let find = || async {
        c.plugins()
            .await
            .unwrap()
            .into_iter()
            .find(|p| p.name == "Local Intros")
    };
    let p = find().await.expect("installed plugin listed after restart");
    assert!(p.can_uninstall);
    c.set_plugin_enabled(&p.id, &p.version, false)
        .await
        .unwrap();
    assert_ne!(find().await.unwrap().status, "Active");
    c.set_plugin_enabled(&p.id, &p.version, true).await.unwrap();
    c.uninstall_plugin(&p.id, &p.version, yes()).await.unwrap();
    restart(c.clone()).await;
    assert!(find().await.is_none(), "uninstalled");
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

#[tokio::test]
#[ignore]
async fn plugin_repositories_roundtrip() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let before = c.plugin_repositories().await.unwrap();
    let url = "https://repo.invalid/nocturne-test/manifest.json";
    c.add_plugin_repository("Nocturne test", url).await.unwrap();
    assert!(
        c.add_plugin_repository("again", url).await.is_err(),
        "duplicate refused"
    );
    let l = c.plugin_repositories().await.unwrap();
    assert_eq!(l.len(), before.len() + 1);
    assert!(l.iter().any(|r| r.url == url && r.enabled));
    c.set_plugin_repository_enabled(url, false).await.unwrap();
    let l = c.plugin_repositories().await.unwrap();
    assert!(l.iter().any(|r| r.url == url && !r.enabled));
    c.remove_plugin_repository(url, yes()).await.unwrap();
    assert_eq!(c.plugin_repositories().await.unwrap(), before);
}

#[tokio::test]
#[ignore]
async fn library_rename_and_providers() {
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let (name, renamed) = (uniq("Prov Library"), uniq("Renamed Library"));
    let dir = "/tmp/nocturne-jf-live-prov";
    std::fs::create_dir_all(dir).unwrap();
    c.add_library(&name, "movies", dir).await.unwrap();
    let find = |n: String| {
        let c = c.clone();
        async move {
            c.virtual_folders()
                .await
                .unwrap()
                .into_iter()
                .find(|l| l.name == n)
        }
    };
    assert!(find(name.clone()).await.is_some(), "created");
    c.rename_library(&name, &renamed).await.unwrap();
    assert!(find(name.clone()).await.is_none());
    // the renamed library gets a new item id once the server has refreshed it
    let mut lib2 = None;
    for _ in 0..40 {
        lib2 = find(renamed.clone())
            .await
            .filter(|l| !l.item_id.is_empty());
        if lib2.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let lib2 = lib2.expect("renamed library with an id");
    let lib = lib2.clone();
    assert!(c.rename_library(&renamed, &renamed).await.is_err());

    // what the server offers for movies, then reorder and disable one metadata fetcher
    let avail = c.available_library_options(Some("movies")).await.unwrap();
    let movie = avail
        .type_options
        .iter()
        .find(|t| t.kind == "Movie")
        .unwrap();
    assert!(movie.metadata_fetchers.len() >= 2 && !movie.image_fetchers.is_empty());
    let mut list = nocturne_jellyfin_admin::edit::current_fetchers(
        &lib2.library_options,
        "Movie",
        movie,
        false,
    );
    let first = list[0].name.clone();
    nocturne_jellyfin_admin::edit::move_fetcher(&mut list, 0, false).unwrap();
    nocturne_jellyfin_admin::edit::toggle_fetcher(&mut list, 0).unwrap();
    let second_was = (list[0].name.clone(), list[0].enabled);
    c.set_library_fetchers(&lib.item_id, "Movie", false, &list)
        .await
        .unwrap();
    let o = find(renamed.clone()).await.unwrap().library_options;
    let back = nocturne_jellyfin_admin::edit::current_fetchers(&o, "Movie", movie, false);
    assert_eq!(back, list, "order and enabled state persisted");
    assert_eq!(back[1].name, first);
    assert_eq!((back[0].name.clone(), back[0].enabled), second_was);
    let imgs = nocturne_jellyfin_admin::edit::current_fetchers(&o, "Movie", movie, true);
    assert!(
        imgs.iter().any(|f| f.enabled),
        "image fetchers were not emptied by a metadata edit"
    );
    // image fetchers
    let mut il = imgs.clone();
    nocturne_jellyfin_admin::edit::toggle_fetcher(&mut il, 0).unwrap();
    c.set_library_fetchers(&lib.item_id, "Movie", true, &il)
        .await
        .unwrap();
    let o = find(renamed.clone()).await.unwrap().library_options;
    assert_eq!(
        nocturne_jellyfin_admin::edit::current_fetchers(&o, "Movie", movie, true),
        il
    );
    // local readers
    c.set_local_reader(&lib.item_id, "Nfo", false)
        .await
        .unwrap();
    let o = find(renamed.clone()).await.unwrap().library_options;
    assert_eq!(
        nocturne_jellyfin_admin::edit::disabled_readers(&o),
        ["Nfo".to_string()]
    );
    c.remove_library(&renamed, yes()).await.unwrap();
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
#[ignore]
async fn user_policy_access_and_image() {
    use nocturne_jellyfin_admin::{PolicyEdit, edit};
    let _g = SERVER.read().await;
    let c = TestServerGuard::connect().await.unwrap();
    let name = uniq("polu");
    let u = c.create_user(&name, "pw-for-test-user").await.unwrap();
    let pol = || async { c.user(&u.id).await.unwrap().policy };

    c.edit_user_policy(&u.id, &PolicyEdit::Flag("EnableRemoteAccess".into(), false))
        .await
        .unwrap();
    c.edit_user_policy(
        &u.id,
        &PolicyEdit::Flag("EnableContentDownloading".into(), false),
    )
    .await
    .unwrap();
    c.edit_user_policy(&u.id, &PolicyEdit::BitrateLimit(4_000_000))
        .await
        .unwrap();
    c.edit_user_policy(&u.id, &PolicyEdit::MaxActiveSessions(2))
        .await
        .unwrap();
    c.edit_user_policy(&u.id, &PolicyEdit::LoginAttempts(5))
        .await
        .unwrap();
    c.edit_user_policy(
        &u.id,
        &PolicyEdit::BlockedTags(edit::split_tags("gore, Sci Fi")),
    )
    .await
    .unwrap();
    let ratings = c.parental_ratings().await.unwrap();
    assert!(ratings.len() > 3 && ratings.windows(2).all(|w| w[0].value <= w[1].value));
    let pg = ratings
        .iter()
        .find(|r| r.name == "PG-13")
        .expect("PG-13")
        .value;
    c.edit_user_policy(&u.id, &PolicyEdit::MaxParentalRating(pg))
        .await
        .unwrap();
    let p = pol().await;
    assert!(!p.flag("EnableRemoteAccess") && !p.flag("EnableContentDownloading"));
    assert!(p.flag("EnableMediaPlayback"), "untouched flags stay");
    assert_eq!(p.get("RemoteClientBitrateLimit"), json!(4_000_000));
    assert_eq!(p.get("MaxActiveSessions"), json!(2));
    assert_eq!(p.get("LoginAttemptsBeforeLockout"), json!(5));
    assert_eq!(p.get("BlockedTags"), json!(["gore", "Sci Fi"]));
    assert_eq!(p.get("MaxParentalRating").as_i64().map(|v| v as i32), pg);
    c.edit_user_policy(&u.id, &PolicyEdit::MaxParentalRating(None))
        .await
        .unwrap();
    assert!(pol().await.get("MaxParentalRating").is_null());
    assert!(
        c.edit_user_policy(&u.id, &PolicyEdit::BitrateLimit(-1))
            .await
            .is_err()
    );

    // library access: block one library, then allow it again
    let libs = c.virtual_folders().await.unwrap();
    let lib = libs
        .iter()
        .find(|l| !l.item_id.is_empty())
        .unwrap()
        .item_id
        .clone();
    c.set_user_library_access(&u.id, &lib, false).await.unwrap();
    let raw = serde_json::to_value(pol().await).unwrap();
    assert!(!edit::folder_allowed(&raw, &lib), "blocked");
    assert_eq!(raw["EnableAllFolders"], false);
    c.set_user_library_access(&u.id, &lib, true).await.unwrap();
    let raw = serde_json::to_value(pol().await).unwrap();
    assert!(edit::folder_allowed(&raw, &lib) && raw["EnableAllFolders"] == true);

    // profile image
    let png = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=32x32",
            "-frames:v",
            "1",
            "-f",
            "image2",
            "-c:v",
            "png",
            "pipe:1",
        ])
        .output()
        .unwrap()
        .stdout;
    assert!(png.starts_with(&[0x89, b'P']));
    assert!(c.user(&u.id).await.unwrap().primary_image_tag.is_none());
    c.set_user_image(&u.id, &png).await.unwrap();
    assert!(c.user(&u.id).await.unwrap().primary_image_tag.is_some());
    c.remove_user_image(&u.id, yes()).await.unwrap();
    assert!(c.user(&u.id).await.unwrap().primary_image_tag.is_none());
    assert!(c.set_user_image(&u.id, b"plain text").await.is_err());

    // display preferences are readable (read only)
    let dp = c.display_preferences(&u.id, "emby").await.unwrap();
    assert!(dp.is_object());
    c.delete_user(&u.id, yes()).await.unwrap();
}
