//! End-to-end through a loopback HTTP server that serves responses in the providers' formats (invented
//! content). No real provider is contacted.

use super::*;
use crate::local::fixture::build;
use crate::online::http;
use crate::source::SourceId;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Route = (&'static str, Mutex<VecDeque<(u16, Vec<u8>)>>);

struct Mock {
    base: String,
    log: Arc<Mutex<Vec<String>>>,
}

impl Mock {
    fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

/// `(path prefix, [(status, body)])`: first matching prefix answers; its list is consumed front to back and the
/// last entry repeats. Anything else is a 404.
type Reply = Vec<(u16, Vec<u8>)>;

async fn serve(routes: Vec<(&'static str, Reply)>) -> Mock {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log = Arc::new(Mutex::new(vec![]));
    // `{BASE}` in a body stands for this server's own address (download links point back at it).
    let routes: Arc<Vec<Route>> = Arc::new(
        routes
            .into_iter()
            .map(|(p, r)| {
                let r = r
                    .into_iter()
                    .map(|(c, b)| {
                        (
                            c,
                            if b.windows(6).any(|w| w == b"{BASE}") {
                                String::from_utf8_lossy(&b)
                                    .replace("{BASE}", &base)
                                    .into_bytes()
                            } else {
                                b
                            },
                        )
                    })
                    .collect();
                (p, Mutex::new(r))
            })
            .collect(),
    );
    let lg = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (routes, lg) = (routes.clone(), lg.clone());
            tokio::spawn(async move {
                let mut buf = vec![];
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let want: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() < head_end + want {
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                lg.lock().unwrap().push(head);
                let (status, body) = routes
                    .iter()
                    .find(|(p, _)| path.starts_with(p))
                    .map(|(_, q)| {
                        let mut q = q.lock().unwrap();
                        if q.len() > 1 {
                            q.pop_front().unwrap()
                        } else {
                            q[0].clone()
                        }
                    })
                    .unwrap_or((404, b"{}".to_vec()));
                let extra = if status == 429 {
                    "Retry-After: 0\r\n"
                } else {
                    ""
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    Mock { base, log }
}

fn ok(s: &str) -> Vec<(u16, Vec<u8>)> {
    vec![(200, s.as_bytes().to_vec())]
}

fn engine(m: &Mock) -> Engine {
    let ep = Endpoints {
        tmdb: format!("{}/3", m.base),
        tmdb_img: format!("{}/img/", m.base),
        tvmaze: m.base.clone(),
        musicbrainz: format!("{}/mb", m.base),
        caa: format!("{}/caa", m.base),
        subs: format!("{}/os", m.base),
        openlibrary: format!("{}/ol", m.base),
    };
    // Gates shrunk so the tests do not wait on real rate limits (the spacing itself is tested in `http`).
    Engine::paced(ep, Arc::default(), 0.001)
}

const JPEG: &[u8] = b"\xff\xd8\xff fake image bytes";

#[tokio::test]
async fn tmdb_movies_are_matched_asked_and_chosen() {
    let m = serve(vec![
        ("/3/search/movie?query=Heat", ok(r#"{"results":[{"id":7,"title":"Heat","release_date":"1995-12-15","overview":"Cops and robbers."},{"id":8,"title":"Heat Wave","release_date":"2001-01-01"}]}"#)),
        ("/3/search/movie?query=Solaris&year=2000", ok(r#"{"results":[{"id":21,"title":"Solaris","release_date":"1972-03-20","overview":"Soviet."},{"id":22,"title":"Solaris","release_date":"2002-11-27","overview":"Remake."}]}"#)),
        ("/3/movie/7", ok(r#"{"id":7,"title":"Heat","release_date":"1995-12-15","overview":"Cops and robbers.","genres":[{"name":"Crime"}],"runtime":170,"vote_average":7.9,"imdb_id":"tt0113277","poster_path":"/p.jpg","backdrop_path":"/b.jpg"}"#)),
        ("/3/movie/21", ok(r#"{"id":21,"title":"Solaris","release_date":"1972-03-20","overview":"Soviet.","poster_path":"/s.jpg"}"#)),
        ("/img/w500/", vec![(200, JPEG.to_vec())]),
        ("/img/w1280/", vec![(200, JPEG.to_vec())]),
    ])
    .await;
    let (t, b) = build(
        SourceId(1),
        &[
            "Films/Heat (1995)/Heat (1995).mkv",
            "Films/Solaris (2000)/Solaris (2000).mkv",
        ],
    );
    let e = engine(&m);
    let p = Providers {
        tmdb_key: Some("K3Y".into()),
        ..Default::default()
    };

    let mut last = Progress::default();
    let rep = e.run(&b, &p, |p| last = p).await;
    assert_eq!((rep.matched, rep.asked, rep.errors), (1, 1, 0), "{rep:?}");
    assert_eq!(last, Progress { done: 2, total: 2 });

    let heat = b.with(|l| l.search("Heat").remove(0));
    assert_eq!(heat.plot.as_deref(), Some("Cops and robbers."));
    assert_eq!(
        (heat.details_from, heat.runtime_min),
        (Some("TMDB"), Some(170))
    );
    assert_eq!(heat.ids.imdb.as_deref(), Some("tt0113277"));
    let data = b.with(|l| l.data_dir().to_path_buf());
    let poster = heat.art.poster.unwrap();
    assert!(poster.starts_with(&data) && std::fs::read(&poster).unwrap() == JPEG);
    assert!(heat.art.backdrop.is_some());

    // The ambiguous one waits for the user instead of being guessed.
    let pending = b.with(|l| l.online_pending());
    assert_eq!(pending.len(), 1);
    let (key, _, cands) = &pending[0];
    assert_eq!(cands.len(), 2);
    assert_eq!(b.with(|l| l.search("Solaris").remove(0)).details_from, None);

    // The user picks the 1972 one; the next run fetches exactly that.
    let idx = cands.iter().position(|c| c.year == Some(1972)).unwrap();
    assert!(b.with_mut(|l| l.choose_online(key, idx)).unwrap());
    let rep = e.run(&b, &p, |_| {}).await;
    assert_eq!((rep.matched, rep.asked), (1, 0), "{rep:?}");
    assert_eq!(
        b.with(|l| l.search("Solaris").remove(0)).plot.as_deref(),
        Some("Soviet.")
    );

    // Privacy and politeness: key only to TMDB, only titles and years in the queries, a real User-Agent, no paths.
    let reqs = m.requests();
    let tmp = t.path().display().to_string();
    for r in &reqs {
        assert!(
            !r.contains(&tmp) && !r.contains("Films") && !r.contains(".mkv"),
            "{r}"
        );
        assert!(r.to_lowercase().contains("user-agent: nocturne/"), "{r}");
    }
    // Files are visited in directory order, which differs per filesystem (APFS vs ext4 / btrfs): check presence, not position.
    assert!(
        reqs.iter()
            .any(|r| r.contains("query=Heat&year=1995&api_key=K3Y")),
        "{reqs:?}"
    );
    // Looked up once: a third run asks nothing.
    let before = m.requests().len();
    e.run(&b, &p, |_| {}).await;
    assert_eq!(m.requests().len(), before);
}

#[tokio::test]
async fn read_token_is_sent_as_bearer_and_a_refused_key_stops_the_run() {
    let m = serve(vec![("/3/", vec![(401, b"{}".to_vec())])]).await;
    let (_t, b) = build(
        SourceId(1),
        &["Films/Heat (1995).mkv", "Films/Alien (1979).mkv"],
    );
    let e = engine(&m);
    let p = Providers {
        tmdb_key: Some("eyJtoken".into()),
        ..Default::default()
    };
    let rep = e.run(&b, &p, |_| {}).await;
    assert!(rep.bad_key);
    assert_eq!(rep.matched, 0);
    assert!(
        m.requests()[0]
            .to_lowercase()
            .contains("authorization: bearer eyjtoken")
    );
    assert!(!m.requests()[0].contains("api_key"));
    assert_eq!(
        m.requests().len(),
        1,
        "the run stops using TMDB at the first refusal"
    );
    assert!(e.validate_tmdb("eyJtoken").await == Err(Err::Auth));
}

#[tokio::test]
async fn rate_limited_requests_are_retried() {
    let m = serve(vec![(
        "/3/configuration",
        vec![
            (429, b"{}".to_vec()),
            (503, b"{}".to_vec()),
            (200, b"{\"images\":{}}".to_vec()),
        ],
    )])
    .await;
    assert_eq!(engine(&m).validate_tmdb("K").await, Ok(()));
    assert_eq!(m.requests().len(), 3);
}

#[tokio::test]
async fn tvmaze_shows_and_episodes_and_musicbrainz_albums() {
    let m = serve(vec![
        ("/search/shows", ok(r#"[{"score":1,"show":{"id":82,"name":"Show","premiered":"2011-04-17","summary":"<p>Cold.</p>"}}]"#)),
        ("/shows/82/episodebynumber?season=1&number=1", ok(r#"{"id":9,"name":"Winter Pilot","airdate":"2011-04-17","runtime":62,"summary":"<p>It starts.</p>"}"#)),
        ("/shows/82", ok(r#"{"id":82,"name":"Show","premiered":"2011-04-17","genres":["Drama"],"summary":"<p>Cold.</p>","externals":{"imdb":"tt1"},"image":{"original":"IMG"}}"#)),
        ("/mb/release-group?query=", ok(r#"{"release-groups":[{"id":"rg-1","title":"Album","first-release-date":"1969-09-26","artist-credit":[{"name":"Artist"}]}]}"#)),
        ("/mb/release-group/rg-1", ok(r#"{"id":"rg-1","title":"Album","first-release-date":"1969-09-26","tags":[{"count":3,"name":"rock"}]}"#)),
        ("/caa/release-group/rg-1/front-500", vec![(200, JPEG.to_vec())]),
    ])
    .await;
    let (_t, b) = build(
        SourceId(1),
        &[
            "Shows/Show/Season 01/Show - S01E01.mkv",
            "Music/Artist/Album/01 - Song.flac",
        ],
    );
    // The image URL in the TVmaze fixture points back at the mock.
    let e = engine(&m);
    let p = Providers {
        tvmaze: true,
        musicbrainz: true,
        ..Default::default()
    };
    let rep = e.run(&b, &p, |_| {}).await;
    // show + episode + album; the show's poster URL ("IMG") is not fetchable, which only costs the picture.
    assert_eq!((rep.matched, rep.errors), (3, 0), "{rep:?}");

    let series = b.with(|l| l.series().remove(0));
    assert_eq!(
        (series.details_from, series.plot.as_deref()),
        (Some("TVmaze"), Some("Cold."))
    );
    assert_eq!(series.genres, ["Drama"]);
    assert_eq!(series.ids.imdb.as_deref(), Some("tt1"));
    let ep = b.with(|l| l.episodes("Show", 1).remove(0));
    assert_eq!(
        (ep.title.as_str(), ep.runtime_min),
        ("Winter Pilot", Some(62))
    );
    let track = b.with(|l| l.items(nocturne_local::Kind::Track).remove(0));
    assert_eq!(
        (track.year, track.genres.clone(), track.details_from),
        (Some(1969), vec!["rock".to_string()], Some("MusicBrainz"))
    );
    assert_eq!(track.title, "Song", "the album lookup never renames tracks");
    assert_eq!(std::fs::read(track.art.poster.unwrap()).unwrap(), JPEG);
    assert!(
        m.requests()
            .iter()
            .any(|r| r.contains("/mb/release-group?query=releasegroup"))
    );
}

#[tokio::test]
async fn nothing_is_asked_unless_a_provider_is_on() {
    let m = serve(vec![]).await;
    let (_t, b) = build(SourceId(1), &["Films/Heat (1995).mkv"]);
    let rep = engine(&m).run(&b, &Providers::default(), |_| {}).await;
    assert_eq!(rep, Report::default());
    assert!(m.requests().is_empty());
    // Providers::from_flags: the master switch and the per-provider switch both count.
    let f = Flags {
        tmdb: true,
        tvmaze: true,
        ..Default::default()
    };
    assert!(!Providers::from_flags(&f, Some("k".into())).any());
    let f = Flags {
        details: true,
        tmdb: true,
        ..Default::default()
    };
    assert!(
        Providers::from_flags(&f, Some("k".into()))
            .tmdb_key
            .is_some()
    );
    assert!(Providers::from_flags(&f, None).tmdb_key.is_none());
}

#[tokio::test]
async fn subtitles_are_searched_downloaded_and_stored_in_the_data_dir() {
    let m = serve(vec![
        ("/os/subtitles?", ok(r#"{"data":[{"id":"1","attributes":{"language":"en","ratings":7.0,"download_count":9,"release":"Heat.1995","files":[{"file_id":555,"file_name":"heat.srt"}]}}]}"#)),
        ("/os/login", ok(r#"{"token":"JWT123","status":200}"#)),
        ("/os/download", ok(r#"{"link":"{BASE}/dl/555.srt","file_name":"heat.srt","remaining":19}"#)),
        ("/dl/555.srt", ok("1\n00:00:01,000 --> 00:00:02,000\nHello\n")),
    ])
    .await;
    let (_t, b) = build(SourceId(1), &["Films/Heat (1995)/Heat (1995).mkv"]);
    let e = engine(&m);
    let heat = b.with(|l| l.search("Heat").remove(0));
    let q = subs::Query {
        title: "Heat".into(),
        year: Some(1995),
        langs: vec!["en".into()],
        ..Default::default()
    };
    let hits = e.find_subtitles("APIKEY", &q).await.unwrap();
    assert_eq!(hits.len(), 1);
    let (name, bytes) = e
        .download_subtitle("APIKEY", Some(&("user".into(), "pw".into())), &hits[0])
        .await
        .unwrap();
    let path = b
        .save_subtitle(&heat.id.0.to_string(), &name, &bytes)
        .unwrap();
    let data = b.with(|l| l.data_dir().to_path_buf());
    assert!(path.starts_with(data.join("subtitles")) && std::fs::read(&path).unwrap() == bytes);
    assert_eq!(b.subtitles_of(&heat.id.0.to_string()), vec![path]);

    let reqs = m.requests().join("\n").to_lowercase();
    assert!(reqs.contains("api-key: apikey") && reqs.contains("authorization: bearer jwt123"));
    assert!(
        reqs.contains("languages=en") && reqs.contains("query=heat") && reqs.contains("year=1995")
    );
    assert!(!reqs.contains(".mkv") && !reqs.contains("moviehash"));

    // A key without a login: the app shows its "needs your login" note; nothing is requested.
    let before = m.requests().len();
    let r = e.download_subtitle("APIKEY", None, &hits[0]).await;
    assert_eq!(r.err(), Some(http::Err::Auth));
    assert_eq!(m.requests().len(), before);
}
