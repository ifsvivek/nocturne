//! Offline HTTP tests against a tiny in-process server: what each call sends (path, key header, body)
//! and how replies map to results and errors. The fixtures are invented and carry no key.
use nocturne_arr::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// (method, target, api key header, body) of every request received.
type Log = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// `routes`: (METHOD, path-without-query, status, body). Unlisted: 404.
async fn serve(routes: Vec<(&'static str, &'static str, u16, String)>) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let log2 = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (routes, log) = (routes.clone(), log2.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let (head, start) = loop {
                    let mut chunk = [0u8; 4096];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break (String::from_utf8_lossy(&buf[..i]).to_string(), i + 4);
                    }
                };
                let header = |name: &str| {
                    head.lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                        })
                        .unwrap_or_default()
                };
                let len: usize = header("content-length").parse().unwrap_or(0);
                while buf.len() < start + len {
                    let mut chunk = [0u8; 4096];
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let mut first = head.lines().next().unwrap_or_default().split(' ');
                let (m, target) = (
                    first.next().unwrap_or_default(),
                    first.next().unwrap_or_default(),
                );
                let body = String::from_utf8_lossy(&buf[start..]).to_string();
                log.lock()
                    .unwrap()
                    .push((m.into(), target.into(), header("x-api-key"), body));
                let path = target.split('?').next().unwrap_or_default();
                let (code, out) = routes
                    .iter()
                    .find(|r| r.0 == m && r.1 == path)
                    .map_or((404, String::new()), |r| (r.2, r.3.clone()));
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    (url, log)
}

const KEY: &str = "test-key-not-real";

#[tokio::test]
async fn reads_send_the_key_and_parse() {
    let (url, log) = serve(vec![
        (
            "GET",
            "/api/v3/system/status",
            200,
            r#"{"appName":"Radarr","version":"5.1.0"}"#.into(),
        ),
        (
            "GET",
            "/api/v3/qualityprofile",
            200,
            r#"[{"id":4,"name":"HD-1080p"},{"id":6,"name":"Any"}]"#.into(),
        ),
        (
            "GET",
            "/api/v3/rootfolder",
            200,
            r#"[{"id":1,"path":"/movies","freeSpace":1}]"#.into(),
        ),
        (
            "GET",
            "/api/v3/movie/lookup",
            200,
            r#"[{"title":"Alien","year":1979,"tmdbId":348}]"#.into(),
        ),
    ])
    .await;
    let c = ArrClient::new(Service::Radarr, &url, KEY).unwrap();
    assert_eq!(c.status().await.unwrap().version, "5.1.0");
    assert_eq!(c.quality_profiles().await.unwrap()[1].name, "Any");
    assert_eq!(c.root_folders().await.unwrap()[0].path, "/movies");
    assert_eq!(c.lookup("alien").await.unwrap()[0].tmdb_id, 348);
    let log = log.lock().unwrap();
    assert!(log.iter().all(|r| r.2 == KEY), "every call carries the key");
    assert!(log.iter().all(|r| r.0 == "GET"), "reads only");
    assert!(log[3].1.contains("term=alien"));
}

#[tokio::test]
async fn progress_combines_the_library_entry_and_the_queue() {
    let (url, log) = serve(vec![
        (
            "GET",
            "/api/v3/movie",
            200,
            r#"[{"id":7,"hasFile":false}]"#.into(),
        ),
        (
            "GET",
            "/api/v3/queue",
            200,
            r#"{"records":[{"movieId":7,"status":"downloading"}]}"#.into(),
        ),
    ])
    .await;
    let c = ArrClient::new(Service::Radarr, &url, KEY).unwrap();
    assert_eq!(c.progress(348).await.unwrap(), Progress::Downloading);
    assert!(log.lock().unwrap()[0].1.contains("tmdbId=348"));
    // a title the service has never seen
    let (url, _) = serve(vec![("GET", "/api/v3/series", 200, "[]".into())]).await;
    let s = ArrClient::new(Service::Sonarr, &url, KEY).unwrap();
    assert_eq!(s.progress(81189).await.unwrap(), Progress::Unknown);
}

#[tokio::test]
async fn adding_posts_the_documented_bodies() {
    let (url, log) = serve(vec![
        ("POST", "/api/v3/movie", 201, "{}".into()),
        ("POST", "/api/v3/series", 201, "{}".into()),
    ])
    .await;
    let ok = ArrConfirmed::i_have_asked_the_user();
    let r = ArrClient::new(Service::Radarr, &url, KEY).unwrap();
    let m = NewMovie {
        title: "Alien".into(),
        year: 1979,
        tmdb_id: 348,
        profile_id: 4,
        root_folder: "/movies".into(),
    };
    r.add_movie(&m, ok).await.unwrap();
    let s = ArrClient::new(Service::Sonarr, &url, KEY).unwrap();
    let n = NewSeries {
        title: "Show".into(),
        tvdb_id: 81189,
        profile_id: 1,
        root_folder: "/tv".into(),
        seasons: vec![SeasonInfo {
            season_number: 2,
            monitored: true,
        }],
    };
    s.add_series(&n, ok).await.unwrap();
    let log = log.lock().unwrap();
    assert_eq!(log[0].1, "/api/v3/movie");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&log[0].3).unwrap(),
        add_movie_body(&m)
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&log[1].3).unwrap(),
        add_series_body(&n)
    );
    assert!(log.iter().all(|r| r.2 == KEY));
}

#[tokio::test]
async fn failures_map_to_plain_errors() {
    let (url, _) = serve(vec![
        ("GET", "/api/v3/system/status", 401, "{}".into()),
        (
            "POST",
            "/api/v3/movie",
            400,
            r#"[{"errorMessage":"This movie has already been added"}]"#.into(),
        ),
    ])
    .await;
    let c = ArrClient::new(Service::Radarr, &url, KEY).unwrap();
    assert!(matches!(c.status().await, Err(Error::Unauthorized)));
    assert!(matches!(c.root_folders().await, Err(Error::NotFound)));
    let m = NewMovie {
        title: "A".into(),
        year: 1,
        tmdb_id: 1,
        profile_id: 1,
        root_folder: "/m".into(),
    };
    match c.add_movie(&m, ArrConfirmed::i_have_asked_the_user()).await {
        Err(Error::Rejected(msg)) => assert!(msg.contains("already been added")),
        other => panic!("{other:?}"),
    }
    // nothing listens: a transport error, not a panic, and the key is not in its text
    let dead = ArrClient::new(Service::Radarr, "http://127.0.0.1:1", KEY).unwrap();
    let e = dead.status().await.unwrap_err();
    assert!(matches!(e, Error::Http(_)) && !e.to_string().contains(KEY));
}
