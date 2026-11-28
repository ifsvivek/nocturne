//! jellyfin-web through the gateway: the requests the web client (and the official phone apps, which wrap it) makes on every screen, replayed against TWO throwaway servers plus a local folder.
//!
//! ```sh
//! NOCTURNE_JF_TEST_WEBDIR=/tmp/nocturne-jf-web scripts/test-jellyfin.sh start     # a copy of jellyfin-web under /tmp, so `/web` has something to pass through
//! NOCTURNE_JF_TEST_WEBDIR=/tmp/nocturne-jf-web scripts/test-jellyfin.sh start 2
//! NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env NOCTURNE_JF_TEST_ENV_2=/tmp/nocturne-jf-test-2/test.env \
//!   cargo test -p nocturne-gateway --test live_web -- --ignored --test-threads=1
//! ```
//!
//! `NOCTURNE_GW_HOLD=<file>` keeps the gateway up for a real browser: the address and the gateway password go into that file (0600) and the test waits until the file is deleted, then replays as usual.
//! Output policy: counts and booleans only.
use nocturne_core::gateway::GatewaySettings;
use nocturne_core::source::SourceId;
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::limits::{Buckets, Limits};
use nocturne_gateway::server;
use nocturne_gateway::state::{Secret, State, UpKind, UpState, Upstream};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

struct Srv {
    url: String,
    key: String,
    id: String,
    user: String,
}

fn env_value(var: &str, key: &str) -> String {
    let text = std::fs::read_to_string(std::env::var(var).unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap()
        .trim()
        .to_string()
}

/// alice's session on a throwaway server: what the app keeps in the keyring.
async fn throwaway(var: &str) -> Srv {
    let (_, url, admin) = TestServerGuard::connect_env(var).await.expect("guard");
    let http = reqwest::Client::new();
    let info: Value = http
        .get(format!("{url}/System/Info/Public"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r: Value = http
        .post(format!("{url}/Users/AuthenticateByName"))
        .header(
            "Authorization",
            r#"MediaBrowser Client="live", Device="rig", DeviceId="rig-web", Version="1""#,
        )
        .json(&json!({"Username": "alice", "Pw": env_value(var, "NOCTURNE_JF_TEST_ALICE_PW")}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    drop(admin);
    Srv {
        url,
        key: r["AccessToken"].as_str().unwrap().to_string(),
        id: info["Id"].as_str().unwrap().to_string(),
        user: r["User"]["Id"].as_str().unwrap().to_string(),
    }
}

struct Gw {
    st: State,
    base: String,
    pw: String,
    _tmp: tempfile::TempDir,
}

/// A gateway on a free loopback port with temp dirs, over both servers and a local folder holding one real (playable) clip.
async fn start(a: &Srv, b: &Srv) -> Gw {
    let tmp = tempfile::tempdir().unwrap();
    let media = tmp.path().join("media");
    std::fs::create_dir_all(&media).unwrap();
    let clip: PathBuf = PathBuf::from(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap())
        .with_file_name("media/Movies/Test Clip 1 (2001)/Test Clip 1 (2001).mp4");
    std::fs::copy(&clip, media.join("Local Only Film (2010).mp4")).unwrap();
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
    let wide = Limits {
        ip: Buckets::new(1e9, 1e9),
        token: Buckets::new(1e9, 1e9),
        ..Default::default()
    };
    let cfg = Config {
        gw: GatewaySettings {
            port: 0,
            remote_control: true,
            ..Default::default()
        },
        allow_server_writes: false,
        sync_progress: false,
        sync_watched: false,
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
        srv(1, "Alpha", a),
        srv(2, "Beta", b),
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
    Gw {
        st,
        base: format!("http://127.0.0.1:{}", addrs[0].port()),
        pw,
        _tmp: tmp,
    }
}

async fn hold(g: &Gw) {
    let Ok(f) = std::env::var("NOCTURNE_GW_HOLD") else {
        return;
    };
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut fh = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&f)
            .unwrap();
        writeln!(fh, "{}\n{}", g.base, g.pw).unwrap();
    }
    while std::path::Path::new(&f).exists() {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// What the web client sends with every call.
const WEB: &str = r#"MediaBrowser Client="Jellyfin Web", Device="Chrome", DeviceId="live-web-1", Version="10.11.0""#;

struct Client {
    base: String,
    http: reqwest::Client,
    token: Option<String>,
    /// Every text answer, for the leak scan.
    seen: Vec<String>,
}

impl Client {
    async fn call(&mut self, m: reqwest::Method, p: &str, body: Option<Value>) -> (u16, Vec<u8>) {
        let auth = match &self.token {
            Some(t) => format!(r#"{WEB}, Token="{t}""#),
            None => WEB.to_string(),
        };
        let mut r = self
            .http
            .request(m, format!("{}{p}", self.base))
            .header("Authorization", auth);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let r = r.send().await.unwrap();
        let s = r.status().as_u16();
        let ct = r
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let b = r.bytes().await.unwrap().to_vec();
        if ct.contains("json") || ct.contains("mpegurl") || ct.starts_with("text/") {
            self.seen.push(String::from_utf8_lossy(&b).into_owned());
        }
        (s, b)
    }
    async fn get(&mut self, p: &str) -> (u16, Vec<u8>) {
        self.call(reqwest::Method::GET, p, None).await
    }
    /// A GET that must answer 200 with JSON.
    async fn json(&mut self, p: &str) -> Value {
        let (s, b) = self.get(p).await;
        assert_eq!(s, 200, "GET {p}");
        serde_json::from_slice(&b).unwrap_or_else(|_| panic!("JSON from {p}"))
    }
    async fn post(&mut self, p: &str, body: Value) -> u16 {
        self.call(reqwest::Method::POST, p, Some(body)).await.0
    }
}

fn resolve(dir: &str, line: &str) -> String {
    if line.starts_with('/') {
        line.to_string()
    } else {
        format!("{dir}/{line}")
    }
}

fn dir(p: &str) -> String {
    let p = p.split('?').next().unwrap();
    p.rsplit_once('/')
        .map_or(String::new(), |(d, _)| d.to_string())
}

/// The first URI of a playlist: an `EXT-X-MAP` init segment if there is one, else the first media line.
fn first_uri(pl: &str) -> String {
    if let Some(l) = pl.lines().find(|l| l.starts_with("#EXT-X-MAP:")) {
        return l.split('"').nth(1).unwrap().to_string();
    }
    pl.lines()
        .find(|l| !l.starts_with('#') && !l.is_empty())
        .expect("a uri")
        .to_string()
}

#[tokio::test]
#[ignore = "needs two throwaway Jellyfin servers with a web client: NOCTURNE_JF_TEST_ENV and NOCTURNE_JF_TEST_ENV_2"]
async fn the_web_client_works_through_the_gateway() {
    let a = throwaway("NOCTURNE_JF_TEST_ENV").await;
    let b = throwaway("NOCTURNE_JF_TEST_ENV_2").await;
    let g = start(&a, &b).await;
    hold(&g).await;
    let mut c = Client {
        base: g.base.clone(),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        token: None,
        seen: vec![],
    };

    // ---- the start page: `/` sends the browser (and the phone apps) to the web client, which a server passes through
    let r = c.http.get(format!("{}/", g.base)).send().await.unwrap();
    assert!(r.status().is_redirection());
    assert_eq!(r.headers()["location"], "/web/");
    let (s, page) = c.get("/web/").await;
    assert_eq!(
        s, 200,
        "the web client (start the servers with NOCTURNE_JF_TEST_WEBDIR)"
    );
    assert!(String::from_utf8_lossy(&page).contains("<html"));

    // ---- sign-in, then what the web client asks before it draws Home
    for p in [
        "/System/Info/Public",
        "/Branding/Configuration",
        "/QuickConnect/Enabled",
        "/Users/Public",
    ] {
        c.json(p).await;
    }
    let (s, body) = c
        .call(
            reqwest::Method::POST,
            "/Users/AuthenticateByName",
            Some(json!({"Username": "nocturne", "Pw": g.pw})),
        )
        .await;
    assert_eq!(s, 200);
    let login: Value = serde_json::from_slice(&body).unwrap();
    c.token = Some(login["AccessToken"].as_str().unwrap().to_string());
    let uid = login["User"]["Id"].as_str().unwrap().to_string();
    assert_eq!(
        c.post("/Sessions/Capabilities/Full", json!({"PlayableMediaTypes": ["Audio", "Video"], "SupportsMediaControl": true, "SupportedCommands": ["Play"]}))
            .await,
        204
    );
    let info = c.json("/System/Info").await;
    assert!(
        info["CastReceiverApplications"].is_array(),
        "the playback settings page iterates it"
    );
    c.json("/System/Endpoint").await;
    c.json(&format!("/Users/{uid}")).await;
    assert!(c.json("/SyncPlay/List").await.is_array());
    c.json(&format!(
        "/DisplayPreferences/usersettings?userId={uid}&client=emby"
    ))
    .await;
    let views = c.json(&format!("/UserViews?userId={uid}")).await;
    let view = views["Items"][0]["Id"].as_str().unwrap().to_string();
    c.json(&format!(
        "/Users/{uid}/Items/Resume?Limit=12&MediaTypes=Video&EnableTotalRecordCount=false"
    ))
    .await;
    c.json(&format!(
        "/Users/{uid}/Items/Latest?Limit=16&ParentId={view}&Fields=PrimaryImageAspectRatio"
    ))
    .await;
    c.json(&format!("/Shows/NextUp?userId={uid}&Limit=24"))
        .await;

    // ---- the library page and its filters
    let list = c
        .json(&format!("/Users/{uid}/Items?SortBy=SortName&SortOrder=Ascending&IncludeItemTypes=Movie&Recursive=true&Fields=PrimaryImageAspectRatio&StartIndex=0&ParentId={view}&Limit=100"))
        .await;
    let id_of = |name: &str| {
        list["Items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["Name"] == name)
            .unwrap_or_else(|| panic!("{name} listed"))["Id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (clip, local) = (id_of("Test Clip 1"), id_of("Local Only Film"));
    let f2 = c
        .json(&format!(
            "/Items/Filters2?userId={uid}&parentId={view}&includeItemTypes=Movie"
        ))
        .await;
    assert!(f2["Genres"].is_array() && f2["Tags"].is_array());
    assert!(
        c.json(&format!("/Users/{uid}/GroupingOptions"))
            .await
            .is_array()
    );

    // ---- item pages: a server item and a local one (every answer in the shape the page iterates)
    for id in [&clip, &local] {
        c.json(&format!("/Users/{uid}/Items/{id}")).await;
        assert!(
            c.json(&format!("/Items/{id}/Ancestors?userId={uid}"))
                .await
                .is_array(),
            "Ancestors is a list"
        );
        assert!(
            c.json(&format!("/Items/{id}/Similar?userId={uid}&limit=12"))
                .await["Items"]
                .is_array()
        );
        assert!(
            c.json(&format!("/Items/{id}/Collections?userId={uid}"))
                .await["Items"]
                .is_array()
        );
        assert!(
            c.json(&format!("/Users/{uid}/Items/{id}/SpecialFeatures"))
                .await
                .is_array()
        );
        c.json(&format!(
            "/Items/{id}/ThemeMedia?userId={uid}&InheritFromParent=true"
        ))
        .await;
    }
    // Pictures come through plain <img> tags without a token.
    let r = c
        .http
        .get(format!(
            "{}/Items/{clip}/Images/Primary?fillHeight=297&fillWidth=198&quality=96",
            g.base
        ))
        .send()
        .await
        .unwrap();
    assert_ne!(r.status(), 401, "pictures need no token");

    // ---- search and favourites (kept by the gateway: server writes are off)
    let hits = c
        .json(&format!(
            "/Users/{uid}/Items?searchTerm=clip&Limit=24&Recursive=true&IncludeItemTypes=Movie"
        ))
        .await;
    assert!(hits["TotalRecordCount"].as_u64().unwrap() >= 1);
    assert_eq!(
        c.post(&format!("/Users/{uid}/FavoriteItems/{clip}"), json!({}))
            .await,
        200
    );
    let fav = c
        .json(&format!(
            "/Users/{uid}/Items?Filters=IsFavorite&Recursive=true&IncludeItemTypes=Movie"
        ))
        .await;
    assert!(
        fav["Items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["Id"] == clip.as_str())
    );

    // ---- settings pages: user configuration and display preferences stay in the gateway
    c.json("/Localization/Cultures").await;
    assert_eq!(
        c.post(
            &format!("/Users/Configuration?userId={uid}"),
            json!({"EnableNextEpisodeAutoPlay": false, "SubtitleMode": "Always"})
        )
        .await,
        204
    );
    let me = c.json("/Users/Me").await;
    assert_eq!(me["Configuration"]["EnableNextEpisodeAutoPlay"], false);
    assert_eq!(me["Configuration"]["SubtitleMode"], "Always");
    assert_eq!(
        c.post(
            &format!("/DisplayPreferences/usersettings?userId={uid}&client=emby"),
            json!({"CustomPrefs": {"enableBackdrops": "1"}})
        )
        .await,
        204
    );
    let dp = c
        .json(&format!(
            "/DisplayPreferences/usersettings?userId={uid}&client=emby"
        ))
        .await;
    assert_eq!(dp["CustomPrefs"]["enableBackdrops"], "1");

    // ---- playback: HLS transcode the way hls.js asks for it (fMP4 segments, an init segment behind EXT-X-MAP)
    let (s, body) = c
        .call(
            reqwest::Method::POST,
            &format!("/Items/{clip}/PlaybackInfo?UserId={uid}&StartTimeTicks=0&IsPlayback=true&AutoOpenLiveStream=true&MaxStreamingBitrate=1000000"),
            Some(json!({"DeviceProfile": {"MaxStreamingBitrate": 1_000_000, "DirectPlayProfiles": [], "SubtitleProfiles": [],
                "TranscodingProfiles": [{"Container": "mp4", "Type": "Video", "AudioCodec": "aac", "VideoCodec": "h264", "Context": "Streaming", "Protocol": "hls", "MaxAudioChannels": "2", "MinSegments": "1", "BreakOnNonKeyFrames": true}]}})),
        )
        .await;
    assert_eq!(s, 200);
    let pi: Value = serde_json::from_slice(&body).unwrap();
    let ms = &pi["MediaSources"][0];
    let tu = ms["TranscodingUrl"]
        .as_str()
        .expect("a transcoding url")
        .to_string();
    assert!(tu.contains("master.m3u8"));
    let (s, master) = c.get(&tu).await;
    assert_eq!(s, 200);
    let master = String::from_utf8(master).unwrap();
    let main = resolve(&dir(&tu), &first_uri(&master));
    let (s, pl) = c.get(&main).await;
    assert_eq!(s, 200, "variant playlist");
    let pl = String::from_utf8(pl).unwrap();
    let seg = resolve(&dir(&main), &first_uri(&pl));
    let (s, bytes) = c.get(&seg).await;
    assert_eq!(s, 200, "first segment");
    assert!(bytes.len() > 100);
    // Reports, then the stop point shows as resume (gateway overlay).
    let rep = |t: i64| json!({"ItemId": clip, "MediaSourceId": ms["Id"], "PlaySessionId": pi["PlaySessionId"], "PositionTicks": t, "PlayMethod": "Transcode", "IsPaused": false});
    assert_eq!(c.post("/Sessions/Playing", rep(0)).await, 204);
    assert_eq!(
        c.post("/Sessions/Playing/Progress", rep(4_000_000)).await,
        204
    );
    let sessions = c
        .json(&format!("/Sessions?ControllableByUserId={uid}"))
        .await;
    assert!(sessions.is_array(), "the cast menu lists sessions");
    assert_eq!(
        c.post("/Sessions/Playing/Stopped", rep(4_000_000)).await,
        204
    );
    let item = c.json(&format!("/Users/{uid}/Items/{clip}")).await;
    assert_eq!(item["UserData"]["PlaybackPositionTicks"], 4_000_000);
    // Direct play of the local copy.
    let (s, _) = c
        .get(&format!("/Videos/{local}/stream.mp4?Static=true"))
        .await;
    assert_eq!(s, 200);

    // ---- sign-out ends the token
    assert_eq!(c.post("/Sessions/Logout", json!({})).await, 204);
    assert_eq!(c.get("/Users/Me").await.0, 401);

    // ---- nothing of a server in any answer
    let all = c.seen.join("\n");
    println!("answers scanned: {} bytes", all.len());
    for (what, bad) in [
        ("token A", a.key.as_str()),
        ("token B", b.key.as_str()),
        ("url A", a.url.as_str()),
        ("url B", b.url.as_str()),
        ("path", "/tmp/nocturne-jf-test"),
        ("local path", "Local Only Film (2010).mp4"),
    ] {
        assert!(!all.contains(bad), "{what} leaked into an answer");
    }
    assert!(
        !all.contains(r#""Path":"/"#),
        "an absolute file path leaked"
    );
    g.st.stop();
}
