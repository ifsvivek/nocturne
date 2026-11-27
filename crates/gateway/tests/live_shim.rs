//! A real remote-control client against the gateway: jellyfin-mpv-shim (run by hand in the headless session, see below) signs in to this gateway, which merges TWO throwaway servers and a local folder; this test plays the part of the controlling app.
//!
//! ```sh
//! scripts/test-jellyfin.sh start; scripts/test-jellyfin.sh start 2
//! scripts/headless-session.sh start
//! NOCTURNE_SHIM_DIR=/tmp/nocturne-shim-run NOCTURNE_JF_TEST_ENV=... NOCTURNE_JF_TEST_ENV_2=... \
//!   cargo test -p nocturne-gateway --test live_shim -- --ignored --nocapture
//! # then, once the file $NOCTURNE_SHIM_DIR/READY exists: sign the shim in to 127.0.0.1:8099 as `nocturne` with the passphrase in $NOCTURNE_SHIM_DIR/gw.pw
//! ```
//!
//! The merged movie "Test Clip 2" must be long enough to pause and seek (replace its file on both servers with a 90 s clip). Output: booleans and counts only.
use nocturne_core::gateway::GatewaySettings;
use nocturne_core::source::SourceId;
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::limits::{Buckets, Limits};
use nocturne_gateway::server;
use nocturne_gateway::state::{Secret, State, UpKind, UpState, Upstream};
use nocturne_gateway::stream::Stats;
use nocturne_jellyfin_admin::guard::TestServerGuard;
use serde_json::{Value, json};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

struct Srv {
    url: String,
    key: String,
    id: String,
    user: String,
}

fn auth(key: &str) -> String {
    format!(r#"MediaBrowser Client="t", Device="t", DeviceId="t", Version="1", Token="{key}""#)
}

async fn throwaway(var: &str) -> Srv {
    let (_, url, key) = TestServerGuard::connect_env(var).await.expect("guard");
    let http = reqwest::Client::new();
    let info: Value = http
        .get(format!("{url}/System/Info/Public"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let users: Value = http
        .get(format!("{url}/Users"))
        .header("Authorization", auth(&key))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let user = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["Name"] == "alice")
        .expect("alice")["Id"]
        .as_str()
        .unwrap()
        .to_string();
    Srv {
        url,
        key,
        id: info["Id"].as_str().unwrap().to_string(),
        user,
    }
}

struct Ctl {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Ctl {
    async fn get(&self, p: &str) -> Value {
        self.http
            .get(format!("{}{p}", self.base))
            .header(
                "Authorization",
                format!(r#"MediaBrowser Token="{}""#, self.token),
            )
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap_or(Value::Null)
    }
    async fn post(&self, p: &str) -> u16 {
        self.http
            .post(format!("{}{p}", self.base))
            .header(
                "Authorization",
                format!(r#"MediaBrowser Token="{}""#, self.token),
            )
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }
    /// The shim's session as clients see it.
    async fn shim(&self) -> Option<Value> {
        self.get("/Sessions")
            .await
            .as_array()?
            .iter()
            .find(|s| {
                s["Client"]
                    .as_str()
                    .is_some_and(|c| c.to_ascii_lowercase().contains("mpv"))
            })
            .cloned()
    }
    /// Poll the shim's session until `f` holds, at most `secs`.
    async fn until(&self, secs: u64, f: impl Fn(&Value) -> bool) -> Option<Value> {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if let Some(s) = self.shim().await
                && f(&s)
            {
                return Some(s);
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        None
    }
}

fn ticks(s: &Value) -> i64 {
    s["PlayState"]["PositionTicks"].as_i64().unwrap_or(0)
}

#[tokio::test]
#[ignore = "needs two throwaway servers and a jellyfin-mpv-shim run by hand; see the file header"]
async fn the_shim_is_driven_through_the_gateway() {
    let dir = std::path::PathBuf::from(
        std::env::var("NOCTURNE_SHIM_DIR").unwrap_or_else(|_| "/tmp/nocturne-shim-run".into()),
    );
    assert!(dir.starts_with("/tmp"));
    std::fs::create_dir_all(&dir).unwrap();
    let a = throwaway("NOCTURNE_JF_TEST_ENV").await;
    let b = throwaway("NOCTURNE_JF_TEST_ENV_2").await;

    // A local demo folder with one real, long-enough clip.
    let tmp = tempfile::tempdir().unwrap();
    let media = tmp.path().join("media");
    std::fs::create_dir_all(&media).unwrap();
    let film = media.join("Local Only Film (2010).mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x90:rate=10:duration=60",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=500:duration=60",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&film)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg made the local clip");
    let mut lib = nocturne_local::Library::open(tmp.path().join("local").join("3")).unwrap();
    lib.scan(
        std::slice::from_ref(&media),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    drop(lib);

    let paths = Paths {
        data: tmp.path().join("gateway"),
        runtime: None,
        settings: None,
        sources: None,
    };
    let cfg = Config {
        gw: GatewaySettings {
            port: 8099,
            remote_control: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let wide = Limits {
        ip: Buckets::new(1e9, 1e9),
        token: Buckets::new(1e9, 1e9),
        ..Default::default()
    };
    let st = State::open_with(paths, cfg, wide).unwrap();
    let srv = |id: u32, label: &str, s: &Srv| Upstream {
        id: SourceId(id),
        label: label.into(),
        kind: UpKind::Server {
            url: s.url.clone(),
            user_id: s.user.clone(),
            source_key: s.id.clone(),
            token: Some(Secret::new(s.key.clone())),
        },
        state: UpState::Ready,
    };
    st.set_upstreams(vec![
        srv(1, "Alpha", &a),
        srv(2, "Beta", &b),
        Upstream {
            id: SourceId(3),
            label: "Local".into(),
            kind: UpKind::Local { roots: vec![media] },
            state: UpState::Ready,
        },
    ]);
    let pw = st.users.take_unseen().unwrap();
    tokio::spawn(server::run(st.clone()));
    let addrs = tokio::time::timeout(Duration::from_secs(5), st.wait_listening())
        .await
        .unwrap();
    let base = format!("http://127.0.0.1:{}", addrs[0].port());
    nocturne_gateway::config::write_private(&dir.join("gw.pw"), pw.as_bytes()).unwrap();
    let _ = std::fs::remove_file(dir.join("DONE"));
    std::fs::write(dir.join("READY"), b"").unwrap();
    println!(
        "gateway up on port {}; waiting for the shim",
        addrs[0].port()
    );

    let http = reqwest::Client::new();
    let r = http
        .post(format!("{base}/Users/AuthenticateByName"))
        .header(
            "X-Emby-Authorization",
            r#"MediaBrowser Client="Driver", Device="driver", DeviceId="driver-1", Version="1""#,
        )
        .json(&json!({"Username": "nocturne", "Pw": pw}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let token = r.json::<Value>().await.unwrap()["AccessToken"]
        .as_str()
        .unwrap()
        .to_string();
    let c = Ctl { http, base, token };

    let s = c
        .until(180, |s| s["SupportsRemoteControl"] == true)
        .await
        .expect("the shim signed in and opened its socket within 180 s");
    println!(
        "shim session listed, controllable: true (client {})",
        s["Client"]
    );
    let sid = s["Id"].as_str().unwrap().to_string();
    let stats = st.ext::<Stats>();

    let movies = c.get("/Items?IncludeItemTypes=Movie&Recursive=true").await;
    let find = |name: &str| {
        movies["Items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["Name"] == name)
            .unwrap_or_else(|| panic!("{name} listed"))
            .clone()
    };
    let mut results: Vec<(String, bool)> = vec![];
    let mut note = |what: &str, ok: bool| {
        println!("{what}: {ok}");
        results.push((what.to_string(), ok));
    };

    for (name, merged) in [("Test Clip 2", true), ("Local Only Film", false)] {
        let item = find(name);
        let gid = item["Id"].as_str().unwrap().to_string();
        if merged {
            println!(
                "{name}: {} media sources",
                item["MediaSources"].as_array().map_or(0, Vec::len)
            );
        }
        let before = (
            stats.requests.load(Relaxed),
            stats.ranged.load(Relaxed),
            stats.bytes.load(Relaxed),
        );
        let code = c
            .post(&format!(
                "/Sessions/{sid}/Playing?PlayCommand=PlayNow&ItemIds={gid}"
            ))
            .await;
        note(&format!("{name}: Play accepted (204)"), code == 204);
        let playing = c
            .until(40, |s| {
                s["NowPlayingItem"]["Id"] == gid.as_str() && ticks(s) > 0
            })
            .await;
        note(
            &format!("{name}: the shim reports playing it (position advancing)"),
            playing.is_some(),
        );
        if let Some(p) = &playing {
            note(
                &format!("{name}: NowPlayingItem carries the real name and type"),
                p["NowPlayingItem"]["Name"] == name && p["NowPlayingItem"]["Type"] == "Movie",
            );
        }
        let after = (
            stats.requests.load(Relaxed),
            stats.ranged.load(Relaxed),
            stats.bytes.load(Relaxed),
        );
        println!(
            "{name}: media requests through the gateway +{}, with Range +{}, body bytes announced +{}",
            after.0 - before.0,
            after.1 - before.1,
            after.2 - before.2
        );
        note(
            &format!("{name}: the gateway served the stream"),
            after.0 > before.0 && after.2 > before.2,
        );
        note(
            &format!("{name}: Range requests flowed"),
            after.1 > before.1,
        );

        let code = c.post(&format!("/Sessions/{sid}/Playing/Pause")).await;
        let paused = c.until(10, |s| s["PlayState"]["IsPaused"] == true).await;
        note(
            &format!("{name}: Pause (204, then reported paused)"),
            code == 204 && paused.is_some(),
        );
        let at = paused.as_ref().map_or(0, ticks);

        let target = at + 300_000_000; // 30 s on
        let code = c
            .post(&format!(
                "/Sessions/{sid}/Playing/Seek?SeekPositionTicks={target}"
            ))
            .await;
        let moved = c.until(10, |s| ticks(s) >= target - 20_000_000).await;
        note(
            &format!("{name}: Seek (204, then position moved ~30 s forward)"),
            code == 204 && moved.is_some(),
        );

        let code = c.post(&format!("/Sessions/{sid}/Playing/Unpause")).await;
        let resumed = c.until(10, |s| s["PlayState"]["IsPaused"] == false).await;
        note(
            &format!("{name}: Unpause"),
            code == 204 && resumed.is_some(),
        );

        let code = c.post(&format!("/Sessions/{sid}/Playing/Stop")).await;
        let stopped = c.until(15, |s| s["NowPlayingItem"].is_null()).await;
        note(
            &format!("{name}: Stop (204, then nothing playing)"),
            code == 204 && stopped.is_some(),
        );
    }
    std::fs::write(dir.join("DONE"), b"").unwrap();
    st.stop();
    let failed: Vec<&str> = results
        .iter()
        .filter(|r| !r.1)
        .map(|r| r.0.as_str())
        .collect();
    assert!(failed.is_empty(), "failed: {failed:?}");
}
