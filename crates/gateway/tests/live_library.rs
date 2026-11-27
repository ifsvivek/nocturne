//! Library and playback through the gateway against TWO throwaway Jellyfin servers plus a local folder, all in this process.
//!
//! ```sh
//! scripts/test-jellyfin.sh start      # 127.0.0.1:8097
//! scripts/test-jellyfin.sh start 2    # 127.0.0.1:8098 (overlapping clips, same provider ids)
//! NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env NOCTURNE_JF_TEST_ENV_2=/tmp/nocturne-jf-test-2/test.env \
//!   cargo test -p nocturne-gateway --test live_library -- --ignored --test-threads=1
//! ```
//!
//! Both URLs pass `TestServerGuard` (loopback, throwaway port, data path under /tmp). Output policy: counts and booleans only.
use nocturne_core::gateway::GatewaySettings;
use nocturne_core::source::SourceId;
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::limits::{Buckets, Limits};
use nocturne_gateway::server;
use nocturne_gateway::state::{Secret, State, UpKind, UpState, Upstream};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;

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
    let info: Value = serde_json::from_slice(
        &http
            .get(format!("{url}/System/Info/Public"))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
    )
    .unwrap();
    let users: Value = serde_json::from_slice(
        &http
            .get(format!("{url}/Users"))
            .header("Authorization", auth(&key))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
    )
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

struct Gw {
    st: State,
    base: String,
    http: reqwest::Client,
    token: String,
    seen: Mutex<Vec<String>>,
    _tmp: tempfile::TempDir,
}

impl Gw {
    fn req(&self, m: reqwest::Method, p: &str) -> reqwest::RequestBuilder {
        self.http.request(m, format!("{}{p}", self.base)).header(
            "Authorization",
            format!(r#"MediaBrowser Token="{}""#, self.token),
        )
    }
    /// Remember text answers for the leak scan.
    fn note(&self, ct: &str, body: &[u8]) {
        if ct.starts_with("application/json") || ct.contains("mpegurl") || ct.starts_with("text/") {
            self.seen
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(body).into_owned());
        }
    }
    async fn get(&self, p: &str) -> (StatusCode, Vec<u8>) {
        let r = self.req(reqwest::Method::GET, p).send().await.unwrap();
        let (s, ct) = (
            r.status(),
            r.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string(),
        );
        let b = r.bytes().await.unwrap().to_vec();
        self.note(&ct, &b);
        (s, b)
    }
    async fn json(&self, p: &str) -> Value {
        let (s, b) = self.get(p).await;
        assert_eq!(s, 200, "GET {p}");
        serde_json::from_slice(&b).unwrap_or_else(|_| panic!("JSON from {p}"))
    }
    async fn post(&self, p: &str, body: Value) -> (StatusCode, Vec<u8>) {
        let r = self
            .req(reqwest::Method::POST, p)
            .json(&body)
            .send()
            .await
            .unwrap();
        let (s, ct) = (
            r.status(),
            r.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string(),
        );
        let b = r.bytes().await.unwrap().to_vec();
        self.note(&ct, &b);
        (s, b)
    }
}

const LOCAL_LEN: usize = 5000;

fn local_bytes() -> Vec<u8> {
    (0..LOCAL_LEN).map(|i| (i * 7 % 251) as u8).collect()
}

async fn start(a: &Srv, b: &Srv, switches: (bool, bool, bool)) -> Gw {
    let tmp = tempfile::tempdir().unwrap();
    // The local library: one file, scanned into the index the way the app does it.
    let media = tmp.path().join("media");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("Local Only Film (2010).mp4"), local_bytes()).unwrap();
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
            ..Default::default()
        },
        allow_server_writes: switches.0,
        sync_progress: switches.1,
        sync_watched: switches.2,
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
    let base = format!("http://127.0.0.1:{}", addrs[0].port());
    let http = reqwest::Client::new();
    let r = http
        .post(format!("{base}/Users/AuthenticateByName"))
        .header(
            "X-Emby-Authorization",
            r#"MediaBrowser Client="live", Device="rig", DeviceId="rig-live", Version="1""#,
        )
        .json(&json!({"Username": "nocturne", "Pw": pw}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = serde_json::from_slice(&r.bytes().await.unwrap()).unwrap();
    Gw {
        st,
        base,
        http,
        token: v["AccessToken"].as_str().unwrap().to_string(),
        seen: Mutex::new(vec![]),
        _tmp: tmp,
    }
}

fn names(v: &Value) -> Vec<String> {
    v["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["Name"].as_str().unwrap().to_string())
        .collect()
}

fn ids(v: &Value) -> Vec<String> {
    v["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["Id"].as_str().unwrap().to_string())
        .collect()
}

fn find<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["Items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["Name"] == name)
        .unwrap_or_else(|| panic!("{name} listed"))
}

/// An item of one server by name, with its server-side id.
async fn upstream_item(s: &Srv, name: &str) -> Value {
    let http = reqwest::Client::new();
    let r = http
        .get(format!("{}/Items?userId={}&Recursive=true&IncludeItemTypes=Movie&searchTerm={}&Fields=MediaSources", s.url, s.user, name.replace(' ', "%20")))
        .header("Authorization", auth(&s.key))
        .send()
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&r.bytes().await.unwrap()).unwrap();
    v["Items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["Name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("{name} on the server"))
}

#[tokio::test]
#[ignore = "needs two throwaway Jellyfin servers: NOCTURNE_JF_TEST_ENV and NOCTURNE_JF_TEST_ENV_2"]
async fn two_servers_and_a_local_folder_behind_one_address() {
    let a = throwaway("NOCTURNE_JF_TEST_ENV").await;
    let b = throwaway("NOCTURNE_JF_TEST_ENV_2").await;
    assert_ne!(a.id, b.id, "two different servers");
    reset("NOCTURNE_JF_TEST_ENV", &a).await;
    reset("NOCTURNE_JF_TEST_ENV_2", &b).await;
    let g = start(&a, &b, (false, false, false)).await;
    let http = reqwest::Client::new();

    // A poster on one server's Clip 1, so the image route has something to pass through (throwaway server: writes are fine).
    let clip1 = upstream_item(&a, "Test Clip 1").await;
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let up = http
        .post(format!(
            "{}/Items/{}/Images/Primary",
            a.url,
            clip1["Id"].as_str().unwrap()
        ))
        .header("Authorization", auth(&a.key))
        .header("Content-Type", "image/png")
        .body(png)
        .send()
        .await
        .unwrap();
    assert!(up.status().is_success());

    // ---- views and the merged movie list
    let views = g.json("/UserViews").await;
    assert!(
        names(&views).contains(&"Movies".to_string()),
        "unified Movies view"
    );
    let movies_view = find(&views, "Movies")["Id"].as_str().unwrap().to_string();
    let list = g.json(&format!("/Items?ParentId={movies_view}&IncludeItemTypes=Movie&Recursive=true&SortBy=SortName&Fields=MediaSources")).await;
    let n = names(&list);
    println!("merged movies: {}", n.len());
    assert_eq!(
        n,
        [
            "Local Only Film",
            "Test Clip 1",
            "Test Clip 2",
            "Test Clip 3",
            "Test Clip 4"
        ],
        "union of both servers and the folder, duplicates merged, sorted"
    );
    assert_eq!(list["TotalRecordCount"], 5);
    // Paging gives the same order in pieces.
    let mut paged = vec![];
    for start in [0, 2, 4] {
        let p = g.json(&format!("/Items?ParentId={movies_view}&IncludeItemTypes=Movie&Recursive=true&SortBy=SortName&StartIndex={start}&Limit=2")).await;
        paged.extend(ids(&p));
        // Approximate on a partial page (duplicates beyond the fetched window are not seen): between the true 5 and the raw sum 7.
        assert!((5..=7).contains(&p["TotalRecordCount"].as_u64().unwrap()));
    }
    assert_eq!(paged, ids(&list));
    let desc = g.json(&format!("/Items?ParentId={movies_view}&IncludeItemTypes=Movie&Recursive=true&SortBy=SortName&SortOrder=Descending&Limit=1")).await;
    assert_eq!(names(&desc), ["Test Clip 4"]);

    // ---- a title on both servers: one item, two labelled media sources
    let merged = find(&list, "Test Clip 2");
    let merged_id = merged["Id"].as_str().unwrap().to_string();
    let detail = g.json(&format!("/Items/{merged_id}")).await;
    let sources = detail["MediaSources"].as_array().unwrap();
    assert_eq!(sources.len(), 2, "one media source per server");
    let sn: Vec<&str> = sources
        .iter()
        .map(|s| s["Name"].as_str().unwrap())
        .collect();
    assert!(
        sn[0].starts_with("Alpha · ") && sn[1].starts_with("Beta · "),
        "labelled by origin: {sn:?}"
    );
    assert!(sources.iter().all(|s| s.get("Path").is_none()));
    assert_eq!(detail["Id"], merged["Id"]);
    assert_eq!(
        find(&list, "Test Clip 1")["MediaSources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let one = g
        .json(&format!(
            "/Items/{}",
            find(&list, "Test Clip 1")["Id"].as_str().unwrap()
        ))
        .await;
    assert!(
        !one["MediaSources"][0]["Name"]
            .as_str()
            .unwrap()
            .contains(" · "),
        "a single copy keeps its own name"
    );
    // Legacy forms and Ids.
    let by_ids = g.json(&format!("/Items?Ids={merged_id}")).await;
    assert_eq!(by_ids["Items"].as_array().unwrap().len(), 1);
    let uid = g.st.users.user_id();
    let legacy = g.json(&format!("/Users/{uid}/Items/{merged_id}")).await;
    assert_eq!(legacy["Id"], merged["Id"]);
    assert_eq!(
        g.json(&format!("/Users/{uid}/Views")).await["Items"]
            .as_array()
            .unwrap()
            .len(),
        views["Items"].as_array().unwrap().len()
    );

    // ---- search, Home rows, case-insensitive routing
    let hints = g
        .json("/Search/Hints?searchTerm=Test%20Clip&limit=10")
        .await;
    assert_eq!(
        hints["SearchHints"].as_array().unwrap().len(),
        4,
        "hints de-duplicated across servers"
    );
    assert_eq!(
        g.json("/Items?SearchTerm=clip&Recursive=true&IncludeItemTypes=Movie")
            .await["TotalRecordCount"],
        4
    );
    let latest = g.json("/Items/Latest?Limit=10").await;
    assert!(latest.as_array().unwrap().len() >= 5);
    assert_eq!(g.get("/UserItems/Resume").await.0, 200);
    assert_eq!(g.get("/Shows/NextUp").await.0, 200);
    assert_eq!(g.get("/Genres").await.0, 200);
    assert_eq!(g.get("/Items/Filters").await.0, 200);
    assert_eq!(g.get(&format!("/Items/{merged_id}/Similar")).await.0, 200);
    assert_eq!(
        g.get("/users/me").await.0,
        200,
        "paths are case-insensitive"
    );
    assert_eq!(g.get("/ITEMS?Recursive=true&Limit=1").await.0, 200);

    // ---- images pass through
    let img = g
        .get(&format!(
            "/Items/{}/Images/Primary",
            find(&list, "Test Clip 1")["Id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(img.0, 200, "poster from the server that has it");
    assert_eq!(
        g.get("/Items/00000000000000000000000000000000/Images/Primary")
            .await
            .0,
        404,
        "an id the gateway never handed out"
    );

    // ---- direct play with Range, from each server
    for (srv, label) in [(&a, "Alpha"), (&b, "Beta")] {
        let sid = sources
            .iter()
            .find(|s| s["Name"].as_str().unwrap().starts_with(label))
            .unwrap()["Id"]
            .as_str()
            .unwrap();
        let r = g
            .req(
                reqwest::Method::GET,
                &format!(
                    "/Videos/{merged_id}/stream?static=true&MediaSourceId={sid}&ApiKey={}",
                    g.token
                ),
            )
            .header("Range", "bytes=0-99")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 206);
        let cr = r.headers()["content-range"].to_str().unwrap().to_string();
        assert!(cr.starts_with("bytes 0-99/"));
        let got = r.bytes().await.unwrap();
        assert_eq!(got.len(), 100);
        let up_item = upstream_item(srv, "Test Clip 2").await;
        let want = http
            .get(format!(
                "{}/Videos/{}/stream?static=true",
                srv.url,
                up_item["Id"].as_str().unwrap()
            ))
            .header("Authorization", auth(&srv.key))
            .header("Range", "bytes=0-99")
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(got, want, "{label}: the bytes of the chosen server");
        assert!(
            cr.ends_with(&format!("/{}", up_item["MediaSources"][0]["Size"])),
            "{label}: total size of its own file"
        );
    }
    // The two copies differ (other resolution): the choice of media source really picked a server.
    let sz = |l: &str| {
        sources
            .iter()
            .find(|s| s["Name"].as_str().unwrap().starts_with(l))
            .unwrap()["Size"]
            .clone()
    };
    assert_ne!(sz("Alpha"), sz("Beta"));
    // A query token works like a header token; a wrong one does not.
    assert_eq!(
        http.get(format!(
            "{}/Videos/{merged_id}/stream?static=true&ApiKey=wrong",
            g.base
        ))
        .send()
        .await
        .unwrap()
        .status(),
        401
    );

    // ---- HLS transcode: playlist rewritten, segment fetched
    let alpha_sid = sources
        .iter()
        .find(|s| s["Name"].as_str().unwrap().starts_with("Alpha"))
        .unwrap()["Id"]
        .as_str()
        .unwrap()
        .to_string();
    let profile = json!({
        "UserId": uid, "EnableDirectPlay": false, "EnableDirectStream": false, "EnableTranscoding": true, "AutoOpenLiveStream": false,
        "DeviceProfile": {"Name": "t", "MaxStreamingBitrate": 2_000_000, "DirectPlayProfiles": [], "CodecProfiles": [], "SubtitleProfiles": [],
            "TranscodingProfiles": [{"Container": "ts", "Type": "Video", "VideoCodec": "h264", "AudioCodec": "aac", "Protocol": "hls", "Context": "Streaming", "MaxAudioChannels": "2", "MinSegments": 1, "BreakOnNonKeyFrames": true}]},
    });
    let (s, body) = g
        .post(
            &format!("/Items/{merged_id}/PlaybackInfo?MediaSourceId={alpha_sid}"),
            profile,
        )
        .await;
    assert_eq!(s, 200);
    let pi: Value = serde_json::from_slice(&body).unwrap();
    let tu = pi["MediaSources"][0]["TranscodingUrl"]
        .as_str()
        .expect("a transcoding url")
        .to_string();
    assert!(
        tu.contains(&format!("ApiKey={}", g.token)),
        "the client's own token replaces the server's"
    );
    assert!(
        tu.starts_with(&format!("/videos/{}", dashed(&merged_id))),
        "gateway item id in the path: {}",
        &tu[..tu.len().min(60)]
    );
    let (s, master) = g.get(&tu).await;
    assert_eq!(s, 200);
    let master = String::from_utf8(master).unwrap();
    assert!(master.starts_with("#EXTM3U"));
    let dir = |p: &str| {
        p.rsplit_once('/')
            .map_or(String::new(), |(d, _)| d.to_string())
    };
    let variant = master
        .lines()
        .find(|l| !l.starts_with('#') && !l.is_empty())
        .expect("a variant");
    let variant_path = resolve(&dir(tu.split('?').next().unwrap()), variant);
    let (s, vl) = g.get(&variant_path).await;
    assert_eq!(s, 200, "variant playlist");
    let vl = String::from_utf8(vl).unwrap();
    let seg = vl
        .lines()
        .find(|l| !l.starts_with('#') && !l.is_empty())
        .expect("a segment");
    let seg_path = resolve(&dir(variant_path.split('?').next().unwrap()), seg);
    let (s, bytes) = g.get(&seg_path).await;
    assert_eq!(s, 200, "segment");
    println!("hls segment bytes: {}", bytes.len());
    assert!(bytes.len() > 100);

    // ---- a local item, direct play from the index
    let local = find(&list, "Local Only Film");
    let local_id = local["Id"].as_str().unwrap().to_string();
    let (s, body) = g
        .post(&format!("/Items/{local_id}/PlaybackInfo"), json!({}))
        .await;
    assert_eq!(s, 200);
    let lpi: Value = serde_json::from_slice(&body).unwrap();
    let ls = &lpi["MediaSources"][0];
    assert_eq!(
        (
            ls["Protocol"].clone(),
            ls["SupportsTranscoding"].clone(),
            ls["Size"].clone()
        ),
        (json!("File"), json!(false), json!(LOCAL_LEN))
    );
    let lsid = ls["Id"].as_str().unwrap();
    let url = format!(
        "/Videos/{local_id}/stream?static=true&MediaSourceId={lsid}&ApiKey={}",
        g.token
    );
    let r = g
        .req(reqwest::Method::GET, &url)
        .header("Range", "bytes=1000-1099")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(
        r.headers()["content-range"],
        format!("bytes 1000-1099/{LOCAL_LEN}")
    );
    assert_eq!(&r.bytes().await.unwrap()[..], &local_bytes()[1000..1100]);
    let full = g.req(reqwest::Method::GET, &url).send().await.unwrap();
    assert_eq!(full.headers()["content-type"], "video/mp4");
    assert_eq!(&full.bytes().await.unwrap()[..], &local_bytes()[..]);
    let past = g
        .req(reqwest::Method::GET, &url)
        .header("Range", "bytes=99999-")
        .send()
        .await
        .unwrap();
    assert_eq!(past.status(), 416);
    let head = g.req(reqwest::Method::HEAD, &url).send().await.unwrap();
    assert_eq!(
        (
            head.status(),
            head.headers()["content-length"]
                .to_str()
                .unwrap()
                .to_string()
        ),
        (StatusCode::OK, LOCAL_LEN.to_string())
    );
    assert_eq!(
        g.get(&format!(
            "/Videos/{local_id}/master.m3u8?MediaSourceId={lsid}"
        ))
        .await
        .0,
        404,
        "local files are direct play only"
    );

    // ---- watched and resume: recorded in the overlay, nothing sent to the servers (writes are off)
    let clip1_id = find(&list, "Test Clip 1")["Id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = g
        .req(
            reqwest::Method::POST,
            &format!("/UserPlayedItems/{clip1_id}"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        g.json(&format!("/Items/{clip1_id}")).await["UserData"]["Played"],
        true
    );
    let upstream_clip1 = upstream_item(&a, "Test Clip 1").await;
    let ud = reqwest::Client::new()
        .get(format!(
            "{}/Items/{}?userId={}",
            a.url,
            upstream_clip1["Id"].as_str().unwrap(),
            a.user
        ))
        .header("Authorization", auth(&a.key))
        .send()
        .await
        .unwrap();
    let ud: Value = serde_json::from_slice(&ud.bytes().await.unwrap()).unwrap();
    assert_eq!(
        ud["UserData"]["Played"], false,
        "the server was not written to"
    );
    let progress = json!({"ItemId": merged_id, "MediaSourceId": alpha_sid, "PositionTicks": 50_000_000i64, "IsPaused": false, "PlaySessionId": "ps-1"});
    assert_eq!(g.post("/Sessions/Playing", progress.clone()).await.0, 204);
    assert_eq!(
        g.post("/Sessions/Playing/Progress", progress.clone())
            .await
            .0,
        204
    );
    assert_eq!(
        g.json(&format!("/Items/{merged_id}")).await["UserData"]["PlaybackPositionTicks"],
        50_000_000i64
    );
    let resume = g.json("/UserItems/Resume").await;
    assert!(
        names(&resume).contains(&"Test Clip 2".to_string()),
        "a gateway-only resume point is listed"
    );
    assert_eq!(
        g.post(
            "/Sessions/Playing/Stopped",
            json!({"ItemId": merged_id, "PositionTicks": 50_000_000i64})
        )
        .await
        .0,
        204
    );
    let upstream_clip2 = upstream_item(&a, "Test Clip 2").await;
    assert_eq!(
        upstream_clip2["UserData"]["PlaybackPositionTicks"]
            .as_i64()
            .unwrap_or(0),
        0,
        "no progress forwarded with writes off"
    );
    // The overlay file is private.
    let ov = g._tmp.path().join("gateway").join("userdata.json");
    assert!(ov.exists());
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&ov).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // ---- the free conformance oracle: the app's own read client against the gateway
    let c = nocturne_jellyfin::Client::new(nocturne_jellyfin::Config {
        base_url: g.base.clone(),
        token: g.token.clone(),
    })
    .unwrap();
    let libs = c.libraries(&uid).await.unwrap();
    assert!(libs.items.iter().any(|l| l.name == "Movies"));
    let page = c
        .items(
            &uid,
            &nocturne_jellyfin::ItemsQuery {
                include_types: vec!["Movie".into()],
                sort_by: Some("SortName".into()),
                limit: Some(3),
                recursive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 3);
    let one = c.item(&uid, &page.items[0].id).await.unwrap();
    assert_eq!(one.id, page.items[0].id);
    assert!(c.resume(&uid, 5).await.is_ok());
    assert!(
        c.image_bytes(
            &clip1_id,
            nocturne_jellyfin::ImageType::Primary,
            Some(200),
            None
        )
        .await
        .is_ok()
    );

    // ---- nothing leaked, anywhere
    let all = g.seen.lock().unwrap().join("\n");
    println!("responses scanned: {} bytes", all.len());
    for (what, bad) in [
        ("token A", a.key.as_str()),
        ("token B", b.key.as_str()),
        ("url A", a.url.as_str()),
        ("url B", b.url.as_str()),
        ("path", "/tmp/nocturne-jf-test"),
        ("local path", "Local Only Film (2010).mp4"),
    ] {
        assert!(!all.contains(bad), "{what} leaked into a response");
    }
    // Any absolute file path, whatever the folder: no `"Path":"/..."` and no key holding a server or media path.
    assert!(
        !all.contains(r#""Path":"/"#) && !all.contains(r#""Path": "/"#) && !all.contains("/media/"),
        "an absolute file path leaked"
    );
    g.st.stop();
}

fn dashed(id: &str) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        &id[..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..]
    )
}

/// The throwaway server's own value for a key of its env file (`NOCTURNE_JF_TEST_ALICE_PW`).
fn env_value(var: &str, key: &str) -> String {
    let text = std::fs::read_to_string(std::env::var(var).unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap()
        .trim()
        .to_string()
}

/// A normal user session on a throwaway server (what the app stores in the keyring), instead of the admin key.
async fn alice(var: &str, s: &Srv) -> Srv {
    let r = reqwest::Client::new()
        .post(format!("{}/Users/AuthenticateByName", s.url))
        .header(
            "Authorization",
            r#"MediaBrowser Client="live", Device="rig", DeviceId="rig-alice", Version="1""#,
        )
        .json(&json!({"Username": "alice", "Pw": env_value(var, "NOCTURNE_JF_TEST_ALICE_PW")}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = serde_json::from_slice(&r.bytes().await.unwrap()).unwrap();
    Srv {
        url: s.url.clone(),
        key: v["AccessToken"].as_str().unwrap().to_string(),
        id: s.id.clone(),
        user: s.user.clone(),
    }
}

/// Every test clip unwatched and without resume point for alice, so a run does not depend on what an earlier run left behind.
async fn reset(var: &str, s: &Srv) {
    let u = alice(var, s).await;
    let http = reqwest::Client::new();
    let items = http
        .get(format!(
            "{}/Items?userId={}&Recursive=true&IncludeItemTypes=Movie",
            u.url, u.user
        ))
        .header("Authorization", auth(&u.key))
        .send()
        .await
        .unwrap();
    let items: Value = serde_json::from_slice(&items.bytes().await.unwrap()).unwrap();
    for i in items["Items"].as_array().unwrap() {
        let r = http
            .delete(format!(
                "{}/UserPlayedItems/{}?userId={}",
                u.url,
                i["Id"].as_str().unwrap(),
                u.user
            ))
            .header("Authorization", auth(&u.key))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }
}

async fn user_data(s: &Srv, name: &str) -> Value {
    upstream_item(s, name).await["UserData"].clone()
}

/// WRITES.md through the gateway: marks and reports reach only the item's own server, only with `allow_server_writes` (progress also needs `sync_progress`, other copies need `sync_watched`).
#[tokio::test]
#[ignore = "needs two throwaway Jellyfin servers: NOCTURNE_JF_TEST_ENV and NOCTURNE_JF_TEST_ENV_2"]
async fn writes_reach_only_the_items_own_server_as_the_switches_allow() {
    let (a, b) = (
        throwaway("NOCTURNE_JF_TEST_ENV").await,
        throwaway("NOCTURNE_JF_TEST_ENV_2").await,
    );
    // The 2 second clips must be resumable (a server keeps no resume point for items shorter than 5 minutes by default).
    for s in [&a, &b] {
        let http = reqwest::Client::new();
        let cfg = http
            .get(format!("{}/System/Configuration", s.url))
            .header("Authorization", auth(&s.key))
            .send()
            .await
            .unwrap();
        let mut cfg: Value = serde_json::from_slice(&cfg.bytes().await.unwrap()).unwrap();
        cfg["MinResumePct"] = json!(0);
        cfg["MaxResumePct"] = json!(90);
        cfg["MinResumeDurationSeconds"] = json!(0);
        let r = http
            .post(format!("{}/System/Configuration", s.url))
            .header("Authorization", auth(&s.key))
            .json(&cfg)
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }
    reset("NOCTURNE_JF_TEST_ENV", &a).await;
    reset("NOCTURNE_JF_TEST_ENV_2", &b).await;
    let (a, b) = (
        alice("NOCTURNE_JF_TEST_ENV", &a).await,
        alice("NOCTURNE_JF_TEST_ENV_2", &b).await,
    );
    let played = |v: &Value| v["Played"].as_bool().unwrap_or(false);
    let pos = |v: &Value| v["PlaybackPositionTicks"].as_i64().unwrap_or(0);
    let movies = "/Items?IncludeItemTypes=Movie&Recursive=true&SortBy=SortName";
    let mark =
        |g: &Gw, m: reqwest::Method, id: &str| g.req(m, &format!("/UserPlayedItems/{id}")).send();

    // Writes on, other copies off: a played mark goes to the first server only.
    let g = start(&a, &b, (true, true, false)).await;
    let list = g.json(movies).await;
    let clip3 = find(&list, "Test Clip 3")["Id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        mark(&g, reqwest::Method::POST, &clip3)
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(
        played(&user_data(&a, "Test Clip 3").await),
        "the item's own (first) server got the mark"
    );
    assert!(
        !played(&user_data(&b, "Test Clip 3").await),
        "the other copy did not (sync_watched is off)"
    );
    assert_eq!(
        mark(&g, reqwest::Method::DELETE, &clip3)
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(!played(&user_data(&a, "Test Clip 3").await));

    // A report naming the second server's media source goes there and nowhere else.
    let detail = g.json(&format!("/Items/{clip3}")).await;
    let beta = detail["MediaSources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["Name"].as_str().unwrap().starts_with("Beta"))
        .unwrap()["Id"]
        .as_str()
        .unwrap()
        .to_string();
    let rep = |t: i64| json!({"ItemId": clip3, "MediaSourceId": beta, "PositionTicks": t, "PlaySessionId": "ps-w"});
    assert_eq!(g.post("/Sessions/Playing", rep(0)).await.0, 204);
    assert_eq!(
        g.post("/Sessions/Playing/Progress", rep(3_000_000)).await.0,
        204
    );
    assert_eq!(
        g.post("/Sessions/Playing/Stopped", rep(3_000_000)).await.0,
        204
    );
    assert_eq!(
        pos(&user_data(&b, "Test Clip 3").await),
        3_000_000,
        "progress forwarded to the chosen copy's server"
    );
    assert_eq!(
        pos(&user_data(&a, "Test Clip 3").await),
        0,
        "and not to the other"
    );
    // Back to a clean state on the throwaway.
    let b_clip3 = upstream_item(&b, "Test Clip 3").await;
    let _ = reqwest::Client::new()
        .delete(format!(
            "{}/UserPlayedItems/{}?userId={}",
            b.url,
            b_clip3["Id"].as_str().unwrap(),
            b.user
        ))
        .header("Authorization", auth(&b.key))
        .send()
        .await;
    g.st.stop();

    // With sync_watched on, every copy follows a mark.
    let g = start(&a, &b, (true, true, true)).await;
    let clip3 = find(&g.json(movies).await, "Test Clip 3")["Id"]
        .as_str()
        .unwrap()
        .to_string();
    mark(&g, reqwest::Method::POST, &clip3).await.unwrap();
    assert!(
        played(&user_data(&a, "Test Clip 3").await) && played(&user_data(&b, "Test Clip 3").await)
    );
    mark(&g, reqwest::Method::DELETE, &clip3).await.unwrap();
    assert!(
        !played(&user_data(&a, "Test Clip 3").await)
            && !played(&user_data(&b, "Test Clip 3").await)
    );
    let all = g.seen.lock().unwrap().join("\n");
    assert!(!all.contains(&a.key) && !all.contains(&b.key));
    g.st.stop();

    // Writes off: the same mark stays in the overlay.
    let g = start(&a, &b, (false, true, true)).await;
    let clip3 = find(&g.json(movies).await, "Test Clip 3")["Id"]
        .as_str()
        .unwrap()
        .to_string();
    mark(&g, reqwest::Method::POST, &clip3).await.unwrap();
    assert!(
        !played(&user_data(&a, "Test Clip 3").await)
            && !played(&user_data(&b, "Test Clip 3").await),
        "allow_server_writes off: no copy written"
    );
    assert_eq!(
        g.json(&format!("/Items/{clip3}")).await["UserData"]["Played"],
        true,
        "but the gateway client sees it"
    );
    g.st.stop();
}

/// A playlist line relative to the directory of the playlist it is in (or root-relative).
fn resolve(dir: &str, line: &str) -> String {
    if line.starts_with('/') {
        line.to_string()
    } else {
        format!("{dir}/{line}")
    }
}
