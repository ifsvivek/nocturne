//! The library manager end to end through the REST API, with the in-memory torrent engine, indexer and metadata: add an indexer, libraries and titles; search; grab the best release; finish the download; import under the naming templates; history. For a movie, an episode, an album and a book, plus a failed download (blocklist, search again) and a request.
use nocturne_core::server::ServerConfig;
use nocturne_core::server::{Title, Unit};
use nocturne_server::fake::{self, FakeDownloader, FakeIndexer, FakeMeta};
use nocturne_server::library::{Catalog, CatalogSlot, Engine};
use nocturne_server::module::Registry;
use nocturne_server::modules::{downloads::Backend, with};
use nocturne_server::state::{Paths, Secrets, Shared};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct T {
    base: String,
    tok: String,
    http: reqwest::Client,
}

impl T {
    async fn req(&self, m: &str, path: &str, body: Option<Value>) -> Value {
        let mut rb = self
            .http
            .request(m.parse().unwrap(), format!("{}{path}", self.base))
            .bearer_auth(&self.tok);
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb.send().await.unwrap();
        let code = r.status();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        assert!(code.is_success(), "{m} {path}: {code} {v}");
        v
    }
    async fn get(&self, p: &str) -> Value {
        self.req("GET", p, None).await
    }
    async fn post(&self, p: &str, b: Value) -> Value {
        self.req("POST", p, Some(b)).await
    }
}

/// Poll `f` every 100 ms for up to 20 s.
async fn until<F: AsyncFn() -> Option<V>, V>(what: &str, f: F) -> V {
    for _ in 0..200 {
        if let Some(v) = f().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

fn library(name: &str, kind: &str, mode: &str, root: &Path) -> Value {
    json!({"name": name, "kind": kind, "mode": mode, "root": root, "folder_template": "", "file_template": ""})
}

fn title(kind: &str, name: &str, year: u16, lib: u64, profile: u64) -> Value {
    json!({"kind": kind, "name": name, "year": year, "ids": {}, "profile": profile, "library": lib, "monitor": "all", "search": false})
}

#[tokio::test(flavor = "multi_thread")]
async fn movie_episode_album_and_book_from_search_to_import() {
    let d = tempfile::tempdir().unwrap();
    let dl = Arc::new(FakeDownloader::default());
    let ix = Arc::new(FakeIndexer::default());
    let meta = Arc::new(FakeMeta::default());
    meta.units.lock().unwrap().insert(
        "Show".into(),
        vec![
            fake::unit("Pilot", Some(1), Some(1), "2020-01-01"),
            fake::unit("Two", Some(1), Some(2), "2020-01-08"),
            fake::unit("Three", Some(1), Some(3), "2099-01-15"),
        ],
    );
    meta.units.lock().unwrap().insert(
        "Simon & Garfunkel".into(),
        vec![fake::unit("Bookends", None, None, "1968-04-03")],
    );
    meta.units.lock().unwrap().insert(
        "Ursula K. Le Guin".into(),
        vec![fake::unit("The Dispossessed", None, None, "1974-05-01")],
    );

    let mut cfg = ServerConfig::default();
    cfg.http.port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    cfg.library.enabled = true;
    cfg.downloads.enabled = true;
    cfg.requests.enabled = true;
    cfg.downloads.dir = Some(d.path().join("downloads"));
    let sh = Shared::new(
        Paths::under(d.path()),
        cfg,
        Secrets::memory(),
        Registry::new(with(
            Backend::Fake(dl.clone()),
            Engine::new(ix.clone(), meta),
        )),
    )
    .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(nocturne_server::http::run(sh.clone(), Some(tx)));
    let addr = rx.await.unwrap()[0];
    let http = reqwest::Client::new();
    let base = format!("http://{addr}/api/v1");
    let v: Value = http
        .post(format!("{base}/setup"))
        .json(&json!({"username": "admin", "password": "correct horse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let t = T {
        base,
        tok: v["token"].as_str().unwrap().into(),
        http,
    };
    let st = t.get("/status").await;
    assert!(
        st["modules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["running"] == true),
        "{st}"
    );

    // Indexer (key to the secret store, never returned), libraries (an existing folder for movies, managed ones for the rest).
    let ixid = t
        .post("/library/indexers", json!({"def": {"id": 0, "name": "Fake", "protocol": "torrent", "url": "http://127.0.0.1:9/torznab", "categories": [], "kinds": ["movie", "series", "music", "book"], "enabled": true, "priority": 1}, "api_key": "secret-key"}))
        .await["id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        sh.secrets.get(&format!("indexer:{ixid}")).as_deref(),
        Some("secret-key")
    );
    assert!(
        !t.get("/library/indexers")
            .await
            .to_string()
            .contains("secret-key")
    );
    let roots: Vec<_> = ["movies", "shows", "music", "books"]
        .iter()
        .map(|n| d.path().join("media").join(n))
        .collect();
    std::fs::create_dir_all(&roots[0]).unwrap();
    std::fs::write(roots[0].join("user file.mkv"), b"the user's").unwrap();
    let mut libs = vec![];
    for (i, (k, mode)) in [
        ("movie", "existing"),
        ("series", "managed"),
        ("music", "managed"),
        ("book", "managed"),
    ]
    .iter()
    .enumerate()
    {
        libs.push(
            t.post("/library/libraries", library(k, k, mode, &roots[i]))
                .await["id"]
                .as_u64()
                .unwrap(),
        );
    }
    // Standard profiles were made at the first start, one per kind in this order.
    let profiles = t.get("/library/profiles").await;
    let prof = |k: &str| {
        profiles
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["kind"] == k)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    };

    ix.set(
        ixid,
        vec![
            fake::release(ixid, "Night.Train.2020.720p.HDTV.x264-LOW", 900 << 20, 50),
            fake::release(
                ixid,
                "Night.Train.2020.1080p.BluRay.x264-GRP",
                1500 << 20,
                10,
            ),
            fake::release(ixid, "Night.Train.2020.CAM-BAD", 700 << 20, 900),
            fake::release(
                ixid,
                "Other.Film.2020.1080p.BluRay.x264-GRP",
                1500 << 20,
                10,
            ),
            fake::release(ixid, "Show.S01E02.1080p.WEB-DL-GRP", 400 << 20, 10),
            fake::release(
                ixid,
                "Simon & Garfunkel - Bookends (1968) [FLAC]",
                300 << 20,
                10,
            ),
            fake::release(
                ixid,
                "Ursula K. Le Guin - The Dispossessed (1974) [EPUB]",
                2 << 20,
                10,
            ),
        ],
    );

    // Interactive search shows every release with its decision.
    let movie = t
        .post(
            "/library/titles",
            title("movie", "Night Train", 2020, libs[0], prof("movie")),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    let cands = t
        .post(&format!("/library/titles/{movie}/releases"), json!({}))
        .await;
    let first = &cands.as_array().unwrap()[0];
    assert_eq!(
        first["release"]["title"],
        "Night.Train.2020.1080p.BluRay.x264-GRP"
    );
    assert!(cands.to_string().contains("CAM is not allowed"), "{cands}");
    assert!(cands.to_string().contains("not for this title"));

    // Automatic search grabs the best one.
    t.post(&format!("/library/titles/{movie}/search"), json!({}))
        .await;
    let q = until("the movie grab", async || {
        let q = t.get("/library/queue").await;
        (q.as_array().unwrap().len() == 1).then_some(q)
    })
    .await;
    assert_eq!(
        q[0]["grab"]["release"]["title"],
        "Night.Train.2020.1080p.BluRay.x264-GRP"
    );
    let torrent = q[0]["grab"]["torrent"].as_str().unwrap().to_string();
    dl.finish(
        &torrent,
        &[
            ("Night.Train.2020.1080p.BluRay.x264-GRP.mkv", b"movie"),
            ("sample.mkv", b"s"),
        ],
    );
    let dest = roots[0].join("Night Train (2020)/Night Train (2020) Bluray-1080p.mkv");
    until("the movie import", async || dest.exists().then_some(())).await;
    assert_eq!(std::fs::read(&dest).unwrap(), b"movie");
    assert_eq!(
        std::fs::read(roots[0].join("user file.mkv")).unwrap(),
        b"the user's",
        "the user's file is untouched"
    );
    let det = t.get(&format!("/library/titles/{movie}")).await;
    assert_eq!(det["units"][0]["file"]["quality"], "bluray1080");
    let h = t.get(&format!("/library/history?title={movie}")).await;
    let ev: Vec<_> = h
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ev, ["imported", "grabbed"]);
    // Still seeding: in the queue as seeding.
    assert_eq!(t.get("/library/queue").await[0]["grab"]["state"], "seeding");

    // An episode: only the asked one, into the season folder.
    let show = t
        .post(
            "/library/titles",
            title("series", "Show", 2020, libs[1], prof("series")),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    let units = t.get(&format!("/library/titles/{show}")).await["units"].clone();
    assert_eq!(units.as_array().unwrap().len(), 3);
    let e2 = units
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["episode"] == 2)
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    t.post(
        &format!("/library/titles/{show}/search"),
        json!({"units": [e2]}),
    )
    .await;
    let g = until("the episode grab", async || {
        let q = t.get("/library/queue").await;
        q.as_array()
            .unwrap()
            .iter()
            .find(|i| i["grab"]["title"] == show)
            .cloned()
    })
    .await;
    dl.finish(
        g["grab"]["torrent"].as_str().unwrap(),
        &[("Show.S01E02.1080p.WEB-DL-GRP.mkv", b"ep2")],
    );
    let dest = roots[1].join("Show (2020)/Season 01/Show - S01E02 - Two WEBDL-1080p.mkv");
    until("the episode import", async || dest.exists().then_some(())).await;

    // An album: every track, kept names.
    let artist = t
        .post(
            "/library/titles",
            title("music", "Simon & Garfunkel", 1964, libs[2], prof("music")),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    t.post(&format!("/library/titles/{artist}/search"), json!({}))
        .await;
    let g = until("the album grab", async || {
        let q = t.get("/library/queue").await;
        q.as_array()
            .unwrap()
            .iter()
            .find(|i| i["grab"]["title"] == artist)
            .cloned()
    })
    .await;
    dl.finish(
        g["grab"]["torrent"].as_str().unwrap(),
        &[
            ("01 Save the Life.flac", b"a"),
            ("02 Old Friends.flac", b"b"),
            ("cover.jpg", b"c"),
        ],
    );
    let album = roots[2].join("Simon & Garfunkel/Bookends (1968)");
    until("the album import", async || {
        album.join("02 Old Friends.flac").exists().then_some(())
    })
    .await;
    assert!(album.join("01 Save the Life.flac").exists() && !album.join("cover.jpg").exists());

    // A book.
    let author = t
        .post(
            "/library/titles",
            title("book", "Ursula K. Le Guin", 1929, libs[3], prof("book")),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    t.post(&format!("/library/titles/{author}/search"), json!({}))
        .await;
    let g = until("the book grab", async || {
        let q = t.get("/library/queue").await;
        q.as_array()
            .unwrap()
            .iter()
            .find(|i| i["grab"]["title"] == author)
            .cloned()
    })
    .await;
    dl.finish(
        g["grab"]["torrent"].as_str().unwrap(),
        &[("The Dispossessed.epub", b"book")],
    );
    let book = roots[3].join("Ursula K. Le Guin/The Dispossessed (1974)/The Dispossessed.epub");
    until("the book import", async || book.exists().then_some(())).await;

    // A failed download: blocklisted, removed, and the next best release grabbed.
    ix.set(
        ixid,
        vec![
            fake::release(
                ixid,
                "Other.Film.2020.1080p.BluRay.x264-GRP",
                1500 << 20,
                10,
            ),
            fake::release(ixid, "Other.Film.2020.720p.WEB-DL.x264-ALT", 900 << 20, 10),
        ],
    );
    let other = t
        .post(
            "/library/titles",
            title("movie", "Other Film", 2020, libs[0], prof("movie")),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    t.post(&format!("/library/titles/{other}/search"), json!({}))
        .await;
    let g = until("the first grab", async || {
        let q = t.get("/library/queue").await;
        q.as_array()
            .unwrap()
            .iter()
            .find(|i| i["grab"]["title"] == other)
            .cloned()
    })
    .await;
    assert_eq!(
        g["grab"]["release"]["title"],
        "Other.Film.2020.1080p.BluRay.x264-GRP"
    );
    dl.fail(g["grab"]["torrent"].as_str().unwrap(), "tracker says no");
    let g2 = until("the second grab", async || {
        let q = t.get("/library/queue").await;
        q.as_array()
            .unwrap()
            .iter()
            .find(|i| {
                i["grab"]["title"] == other
                    && i["grab"]["release"]["title"] != g["grab"]["release"]["title"]
            })
            .cloned()
    })
    .await;
    assert_eq!(
        g2["grab"]["release"]["title"],
        "Other.Film.2020.720p.WEB-DL.x264-ALT"
    );
    let bl = t.get("/library/blocklist").await;
    assert_eq!(bl[0]["reason"], "tracker says no");
    assert!(
        !dl.ids()
            .contains(&g["grab"]["torrent"].as_str().unwrap().to_string()),
        "the failed torrent is gone"
    );

    // A request becomes a monitored title in the default library and profile of its kind.
    let r = t
        .post(
            "/requests",
            json!({"kind": "movie", "name": "Third Movie", "year": 2021}),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    let reqs = t.get("/requests").await;
    let mine = reqs
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == r)
        .unwrap();
    assert_eq!(
        (mine["status"].as_str(), mine["requested_by"].as_str()),
        (Some("approved"), Some("admin"))
    );
    let tid = mine["title"].as_u64().unwrap();
    assert_eq!(
        t.get(&format!("/library/titles/{tid}")).await["title"]["library"],
        libs[0]
    );

    // A title a source already has: available at once, never grabbed.
    struct OnSource;
    impl Catalog for OnSource {
        fn has(&self, t: &Title, _: &Unit) -> bool {
            t.name == "On Source"
        }
    }
    *sh.ext::<CatalogSlot>().0.write().unwrap() = Some(Arc::new(OnSource));
    ix.set(
        ixid,
        vec![fake::release(
            ixid,
            "On.Source.2020.1080p.BluRay.x264-GRP",
            1500 << 20,
            10,
        )],
    );
    let r = t
        .post(
            "/requests",
            json!({"kind": "movie", "name": "On Source", "year": 2020}),
        )
        .await["id"]
        .as_u64()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let reqs = t.get("/requests").await;
    let mine = reqs
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == r)
        .unwrap();
    assert_eq!(mine["status"], "available");
    let on = mine["title"].as_u64().unwrap();
    let q = t.get("/library/queue").await;
    assert!(
        !q.as_array()
            .unwrap()
            .iter()
            .any(|i| i["grab"]["title"] == on),
        "{q}"
    );

    // Counters and jobs are reported.
    let st = t.get("/status").await;
    assert_eq!(st["titles"], 7, "{st}");
    assert!(st["missing"].as_u64().unwrap() >= 2, "{st}");
    assert_eq!(t.get("/library/jobs").await.as_array().unwrap().len(), 4);
    sh.stop();
}
