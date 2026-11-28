//! Offline tests: fixture parsing, and the clients against a tiny in-process HTTP server. No real indexer or download client is ever contacted; the fixtures are hand-made and the key is fake.
use nocturne_indexer::usenet::{Nzbget, Sabnzbd, UsenetClient, UsenetState};
use nocturne_indexer::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const KEY: &str = "test-key-not-real";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap().as_str()
    ))
    .unwrap()
}

/// (method, target, headers, body) of every request.
type Log = Arc<Mutex<Vec<(String, String, String, Vec<u8>)>>>;
/// method, target -> (status, extra header lines, body)
type Handler = Arc<dyn Fn(&str, &str) -> (u16, String, Vec<u8>) + Send + Sync>;

async fn serve(h: Handler) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let log2 = log.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let (h, log) = (h.clone(), log2.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let (head, start) = loop {
                    let mut chunk = [0u8; 8192];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break (String::from_utf8_lossy(&buf[..i]).to_string(), i + 4);
                    }
                };
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse().ok())?
                    })
                    .unwrap_or(0);
                while buf.len() < start + len {
                    let mut chunk = [0u8; 8192];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let mut first = head.lines().next().unwrap_or_default().split(' ');
                let (m, target) = (
                    first.next().unwrap_or_default().to_string(),
                    first.next().unwrap_or_default().to_string(),
                );
                let (code, extra, out) = h(&m, &target);
                log.lock()
                    .unwrap()
                    .push((m, target, head.clone(), buf[start..].to_vec()));
                let mut resp = format!(
                    "HTTP/1.1 {code} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    out.len()
                )
                .into_bytes();
                resp.extend_from_slice(&out);
                let _ = s.write_all(&resp).await;
            });
        }
    });
    (url, log)
}

fn query(target: &str, key: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.replace('+', " ").replace("%2C", ","))
    })
}

fn indexer(url: &str) -> Indexer {
    Indexer::new(7, url, KEY, Protocol::Torrent, Duration::ZERO).unwrap()
}

#[test]
fn caps_fixtures() {
    let c = parse_caps(&fixture("caps_torznab.xml")).unwrap();
    assert_eq!(c.limit_max, Some(100));
    assert!(c.supports(SearchMode::Movie, "imdbid") && c.supports(SearchMode::Movie, "tmdbid"));
    assert!(c.supports(SearchMode::Tv, "tvdbid") && c.supports(SearchMode::Tv, "ep"));
    assert!(c.supports(SearchMode::Music, "artist"));
    assert!(c.book.is_none());
    assert_eq!(c.categories.len(), 4);
    assert_eq!(c.categories[0].subcats.len(), 3);

    let n = parse_caps(&fixture("caps_newznab.xml")).unwrap();
    assert_eq!(n.limit_default, Some(50));
    assert!(
        n.supports(SearchMode::Music, "album"),
        "audio-search counts as music"
    );
    assert!(n.supports(SearchMode::Book, "author"));
    assert!(!n.supports(SearchMode::Movie, "tmdbid"));
}

#[test]
fn torznab_results_fixture() {
    let p = parse_results(&fixture("torznab_movie.xml"), 7, Protocol::Torrent).unwrap();
    assert_eq!(p.releases.len(), 2, "the item without a link is dropped");
    let a = &p.releases[0];
    assert_eq!(
        a.title,
        "Night.Of.The.Example.1968.1080p.BluRay.x264-EXAMPLE"
    );
    assert_eq!(a.guid, "http://tracker.example.invalid/torrent/1001");
    assert_eq!(a.size, Some(8_589_934_592));
    assert_eq!(
        (a.seeders, a.leechers, a.grabs),
        (Some(87), Some(9), Some(1520))
    );
    assert_eq!(
        a.info_hash.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    assert_eq!(a.imdb_id.as_deref(), Some("tt0063350"));
    assert_eq!(a.tmdb_id, Some(10331));
    assert_eq!(a.categories, vec![2000, 100014]);
    assert_eq!(a.publish_date, Some(1_716_019_200));
    assert_eq!(a.indexer_id, 7);
    assert_eq!(a.kind(), Some(MediaKind::Movie));
    assert!(a.download_url.as_deref().unwrap().contains("FIXTUREKEY"));
    assert!(
        !format!("{a:?}").contains("FIXTUREKEY"),
        "Debug redacts link queries"
    );
    assert!(a.magnet.is_none());

    let b = &p.releases[1];
    assert!(b.download_url.is_none());
    assert!(
        b.magnet
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:89AB")
    );
    assert_eq!(
        b.info_hash.as_deref(),
        Some("89abcdef0123456789abcdef0123456789abcdef")
    );
    assert_eq!((b.seeders, b.leechers), (Some(12), Some(3)));
    assert_eq!(b.publish_date, Some(1_704_067_200));
    assert!(!b.needs_usenet_client());
}

#[test]
fn newznab_results_fixture() {
    let p = parse_results(&fixture("newznab_tv.xml"), 3, Protocol::Usenet).unwrap();
    assert_eq!((p.offset, p.total, p.count), (0, Some(137), 2));
    assert_eq!(p.next_offset(2), Some(2));
    let a = &p.releases[0];
    assert!(a.needs_usenet_client());
    assert_eq!(a.tvdb_id, Some(81189));
    assert_eq!(a.imdb_id.as_deref(), Some("tt0903747"));
    assert_eq!(a.categories, vec![5000, 5040]);
    assert_eq!(a.grabs, Some(311));
    assert_eq!(a.seeders, None);
    assert_eq!(
        a.info_url.as_deref(),
        Some("https://nzb.example.invalid/details/5f1d2c#comments")
    );
    let b = &p.releases[1];
    assert_eq!(
        b.size,
        Some(1_288_490_188),
        "size attr when the enclosure length is 0"
    );
    assert_eq!(b.guid, "a7b8c9");
}

#[test]
fn error_document() {
    match parse_results(&fixture("error_apikey.xml"), 1, Protocol::Torrent) {
        Err(Error::Indexer { code: 100, .. }) => {}
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        parse_results("<html>nope", 1, Protocol::Torrent),
        Err(Error::Parse(_))
    ));
    assert!(
        parse_results(&fixture("empty.xml"), 1, Protocol::Torrent)
            .unwrap()
            .releases
            .is_empty()
    );
}

#[tokio::test]
async fn search_ids_first_then_text_and_caps_cached() {
    let (url, log) = serve(Arc::new(|_, t: &str| {
        let body = match query(t, "t").as_deref() {
            Some("caps") => fixture("caps_torznab.xml"),
            Some("movie") if query(t, "imdbid").is_some() => fixture("empty.xml"),
            Some("movie") => fixture("torznab_movie.xml"),
            _ => fixture("empty.xml"),
        };
        (200, String::new(), body.into_bytes())
    }))
    .await;
    let ix = indexer(&url);
    let q = SearchQuery {
        kind: Some(MediaKind::Movie),
        text: Some("Night of the Example".into()),
        year: Some(1968),
        imdb_id: Some("tt0063350".into()),
        ..SearchQuery::default()
    };
    let p = ix.search(&q).await.unwrap();
    assert_eq!(p.releases.len(), 2);
    let _ = ix.search(&q).await.unwrap();
    let log = log.lock().unwrap();
    let targets: Vec<&str> = log.iter().map(|l| l.1.as_str()).collect();
    assert_eq!(
        targets.iter().filter(|t| t.contains("t=caps")).count(),
        1,
        "caps cached: {targets:?}"
    );
    assert_eq!(targets.len(), 5);
    let id = targets[1];
    assert!(
        id.starts_with("/api?t=movie&imdbid=0063350&cat=2000&extended=1&apikey="),
        "{id}"
    );
    let text = targets[2];
    assert_eq!(
        query(text, "q").as_deref(),
        Some("Night of the Example 1968")
    );
    assert!(query(text, "imdbid").is_none());
    assert_eq!(query(text, "apikey").as_deref(), Some(KEY));
}

#[tokio::test]
async fn latest_and_pagination_params() {
    let (url, log) = serve(Arc::new(|_, _: &str| {
        (200, String::new(), fixture("newznab_tv.xml").into_bytes())
    }))
    .await;
    let ix = Indexer::new(
        3,
        &format!("{url}/newznab/"),
        KEY,
        Protocol::Usenet,
        Duration::ZERO,
    )
    .unwrap();
    let p = ix.latest(&[5040, 2040], 100, Some(50)).await.unwrap();
    assert!(
        p.releases
            .iter()
            .all(|r| r.protocol == Protocol::Usenet && r.indexer_id == 3)
    );
    let t = log.lock().unwrap()[0].1.clone();
    assert!(
        t.starts_with(
            "/newznab/api?t=search&cat=5040%2C2040&extended=1&offset=100&limit=50&apikey="
        ),
        "{t}"
    );
}

#[tokio::test]
async fn backoff_on_429_and_key_never_in_errors() {
    let (url, log) = serve(Arc::new(|_, _: &str| {
        (429, "Retry-After: 120\r\n".into(), Vec::new())
    }))
    .await;
    let ix = indexer(&url);
    let e = ix.caps().await.unwrap_err();
    assert!(matches!(e, Error::Status(429)), "{e:?}");
    match ix.caps().await.unwrap_err() {
        Error::Backoff(d) => assert!(d > Duration::from_secs(100) && d <= Duration::from_secs(120)),
        e => panic!("{e:?}"),
    }
    assert_eq!(log.lock().unwrap().len(), 1, "no request while backing off");
    assert!(!format!("{ix:?}").contains(KEY));

    let (url, _) = serve(Arc::new(|_, _: &str| {
        (200, String::new(), fixture("error_apikey.xml").into_bytes())
    }))
    .await;
    let e = indexer(&url).caps().await.unwrap_err();
    assert!(matches!(e, Error::Indexer { code: 100, .. }));
    assert!(!e.to_string().contains(KEY), "{e}");
    assert!(!format!("{e:?}").contains(KEY));

    let (url, _) = serve(Arc::new(|_, _: &str| (503, String::new(), Vec::new()))).await;
    let ix = indexer(&url);
    let _ = ix.caps().await;
    assert!(matches!(ix.caps().await, Err(Error::Backoff(d)) if d > Duration::from_secs(50)));
}

#[tokio::test]
async fn unauthorized_and_size_cap() {
    let (url, _) = serve(Arc::new(|_, _: &str| (401, String::new(), Vec::new()))).await;
    assert!(matches!(
        indexer(&url).caps().await,
        Err(Error::Unauthorized)
    ));
    let (url, _) = serve(Arc::new(|_, _: &str| {
        (200, String::new(), vec![b' '; 17 * 1024 * 1024])
    }))
    .await;
    assert!(matches!(
        indexer(&url).caps().await,
        Err(Error::TooLarge(_))
    ));
}

#[tokio::test]
async fn rate_limit_spaces_requests() {
    let (url, _) = serve(Arc::new(|_, _: &str| {
        (200, String::new(), fixture("empty.xml").into_bytes())
    }))
    .await;
    let ix = Indexer::new(1, &url, KEY, Protocol::Torrent, Duration::from_millis(150)).unwrap();
    let t = std::time::Instant::now();
    for _ in 0..3 {
        ix.latest(&[], 0, None).await.unwrap();
    }
    assert!(
        t.elapsed() >= Duration::from_millis(300),
        "{:?}",
        t.elapsed()
    );
}

#[tokio::test]
async fn downloads_redirects_and_magnets() {
    let (other, other_log) =
        serve(Arc::new(|_, _: &str| (200, String::new(), b"x".to_vec()))).await;
    let other2 = other.clone();
    let (url, _) = serve(Arc::new(move |_, t: &str| match t {
        "/dl/file" => (200, String::new(), b"d8:announce...e".to_vec()),
        "/dl/hop" => (302, "Location: /dl/file\r\n".into(), Vec::new()),
        "/dl/magnet" => (
            302,
            "Location: magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\r\n".into(),
            Vec::new(),
        ),
        // 127.0.0.1 vs localhost: another host.
        "/dl/away" => (
            302,
            format!(
                "Location: {}/x\r\n",
                other2.replace("127.0.0.1", "localhost")
            ),
            Vec::new(),
        ),
        _ => (404, String::new(), Vec::new()),
    }))
    .await;
    let ix = indexer(&url);
    assert_eq!(
        ix.download(&format!("{url}/dl/file")).await.unwrap(),
        Download::File(b"d8:announce...e".to_vec())
    );
    assert_eq!(
        ix.download(&format!("{url}/dl/hop")).await.unwrap(),
        Download::File(b"d8:announce...e".to_vec())
    );
    match ix.download(&format!("{url}/dl/magnet")).await.unwrap() {
        Download::Magnet(m) => assert!(m.starts_with("magnet:?xt=urn:btih:0123")),
        d => panic!("{d:?}"),
    }
    assert!(matches!(
        ix.download(&format!("{url}/dl/away")).await,
        Err(Error::Redirect)
    ));
    assert!(other_log.lock().unwrap().is_empty());
    assert!(matches!(
        ix.download("file:///etc/passwd").await,
        Err(Error::Config(_))
    ));
    assert!(Indexer::new(1, "ftp://x/", KEY, Protocol::Torrent, Duration::ZERO).is_err());
    assert!(Indexer::new(1, "http://x/", " ", Protocol::Torrent, Duration::ZERO).is_err());
}

#[tokio::test]
async fn sabnzbd_add_queue_history() {
    let (url, log) = serve(Arc::new(|_, t: &str| {
        let body = match query(t, "mode").as_deref() {
            Some("addfile") => r#"{"status": true, "nzo_ids": ["SABnzbd_nzo_p86tgx"]}"#.to_string(),
            Some("queue") => fixture("sab_queue.json"),
            Some("history") => fixture("sab_history.json"),
            _ => r#"{"status": false, "error": "API Key Incorrect"}"#.to_string(),
        };
        (
            200,
            "Content-Type: application/json\r\n".into(),
            body.into_bytes(),
        )
    }))
    .await;
    let c = UsenetClient::Sabnzbd(Sabnzbd::new(&url, KEY).unwrap());
    assert!(!format!("{c:?}").contains(KEY));
    let id = c
        .add_nzb("Example.Show.S02E05", b"<nzb>fixture</nzb>", Some("tv"))
        .await
        .unwrap();
    assert_eq!(id, "SABnzbd_nzo_p86tgx");
    {
        let log = log.lock().unwrap();
        let (m, t, head, body) = &log[0];
        assert_eq!(m, "POST");
        assert!(
            t.starts_with(
                "/api?mode=addfile&output=json&nzbname=Example.Show.S02E05&cat=tv&apikey="
            ),
            "{t}"
        );
        assert!(
            head.to_lowercase()
                .contains("multipart/form-data; boundary=")
        );
        let body = String::from_utf8_lossy(body);
        assert!(
            body.contains("filename=\"Example.Show.S02E05.nzb\"")
                && body.contains("<nzb>fixture</nzb>")
        );
    }
    let q = c.queue().await.unwrap();
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].state, UsenetState::Downloading);
    assert!((q[0].progress - 0.5).abs() < 1e-6);
    assert_eq!(q[0].category.as_deref(), Some("tv"));
    assert_eq!(q[1].state, UsenetState::Queued);
    let h = c.history().await.unwrap();
    assert_eq!(h[0].state, UsenetState::Completed);
    assert_eq!(
        h[0].path.as_deref(),
        Some("/downloads/complete/tv/Example.Show.S02E04.1080p.WEB.H264-GROUP")
    );
    assert_eq!(h[1].state, UsenetState::Failed);
    assert!(h[1].error.as_deref().unwrap().contains("Repair failed"));
}

#[tokio::test]
async fn sabnzbd_bad_key() {
    let (url, _) = serve(Arc::new(|_, _: &str| {
        (
            200,
            String::new(),
            br#"{"status": false, "error": "API Key Incorrect"}"#.to_vec(),
        )
    }))
    .await;
    let c = Sabnzbd::new(&url, KEY).unwrap();
    assert!(matches!(c.queue().await, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn nzbget_append_listgroups_history() {
    // NZBGet is JSON-RPC on one path: answer by method name in the body.
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let seen2 = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let seen = seen2.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                loop {
                    let mut chunk = [0u8; 8192];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let txt = String::from_utf8_lossy(&buf).to_string();
                    let Some(i) = txt.find("\r\n\r\n") else {
                        continue;
                    };
                    let len: usize = txt[..i]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() < i + 4 + len {
                        continue;
                    }
                    let body = txt[i + 4..].to_string();
                    seen.lock()
                        .unwrap()
                        .push((txt[..i].to_string(), body.clone()));
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let (code, out) = if !txt[..i].to_lowercase().contains("authorization: basic") {
                        (401, String::new())
                    } else {
                        match v["method"].as_str().unwrap() {
                            "append" => (200, r#"{"version": "1.1", "id": 1, "result": 19}"#.to_string()),
                            "listgroups" => (200, fixture("nzbget_listgroups.json")),
                            "history" => (200, fixture("nzbget_history.json")),
                            _ => (200, r#"{"version":"1.1","id":1,"error":{"code":1,"message":"Invalid procedure"}}"#.to_string()),
                        }
                    };
                    let resp = format!(
                        "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                        out.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                    return;
                }
            });
        }
    });
    let c = UsenetClient::Nzbget(Nzbget::new(&url, "nzbget", "fixture-password").unwrap());
    assert!(!format!("{c:?}").contains("fixture-password"));
    assert_eq!(
        c.add_nzb("Some.Release", b"<nzb/>", Some("movies"))
            .await
            .unwrap(),
        "19"
    );
    {
        let seen = seen.lock().unwrap();
        assert!(seen[0].0.starts_with("POST /jsonrpc "), "{}", seen[0].0);
        let body: serde_json::Value = serde_json::from_str(&seen[0].1).unwrap();
        assert_eq!(body["method"], "append");
        assert_eq!(body["params"][0], "Some.Release.nzb");
        assert_eq!(body["params"][1], "PG56Yi8+"); // base64 of <nzb/>
        assert_eq!(body["params"][2], "movies");
    }
    let q = c.queue().await.unwrap();
    assert_eq!(q[0].id, "17");
    assert_eq!(q[0].state, UsenetState::Downloading);
    assert!((q[0].progress - 0.75).abs() < 1e-6);
    assert_eq!(q[1].state, UsenetState::Processing);
    let h = c.history().await.unwrap();
    assert_eq!(h[0].state, UsenetState::Completed);
    assert_eq!(
        h[0].path.as_deref(),
        Some("/downloads/complete/tv/Example.Show.S02E04")
    );
    assert_eq!(h[1].state, UsenetState::Failed);
    assert_eq!(h[1].error.as_deref(), Some("FAILURE/PAR"));
}
