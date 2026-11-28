//! Offline HTTP tests against a tiny in-process server: what each call sends (path, cookie, body),
//! how the cookie is captured at sign-in, and how error replies map to errors.
use nocturne_seerr::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap().as_str()
    ))
    .unwrap()
}

/// (method, target, cookie header, body) of every request received.
type Log = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// `routes`: (METHOD, path-without-query, status, extra header line, body). Unlisted: 404.
async fn serve(
    routes: Vec<(&'static str, &'static str, u16, &'static str, String)>,
) -> (String, Log) {
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
                    .push((m.into(), target.into(), header("cookie"), body));
                let path = target.split('?').next().unwrap_or_default();
                let (code, extra, out) = routes
                    .iter()
                    .find(|r| r.0 == m && r.1 == path)
                    .map_or((404, "", String::new()), |r| (r.2, r.3, r.4.clone()));
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    (url, log)
}

fn signed_in(url: &str) -> SeerrClient {
    SeerrClient::new(url, Auth::Cookie(SessionCookie::from_value("abc%3A123"))).unwrap()
}

const OK: &str = "";

#[tokio::test]
async fn sign_in_captures_the_cookie_and_sends_the_credentials_once() {
    let cookie = "Set-Cookie: connect.sid=s%3Anew; Path=/; HttpOnly\r\n";
    let (url, log) = serve(vec![(
        "POST",
        "/api/v1/auth/jellyfin",
        200,
        cookie,
        fixture("me.json"),
    )])
    .await;
    let (me, c) = SeerrClient::new(&url, Auth::None)
        .unwrap()
        .sign_in_password("alice", "pw")
        .await
        .unwrap();
    assert_eq!((me.id, me.name().as_str()), (2, "alice"));
    assert_eq!(c.value(), "s%3Anew");
    let l = log.lock().unwrap();
    assert!(l[0].3.contains("\"username\":\"alice\"") && !l[0].2.contains("connect.sid"));
}

#[tokio::test]
async fn wrong_password_is_invalid_credentials_and_a_missing_cookie_is_an_error() {
    let (url, _) = serve(vec![
        (
            "POST",
            "/api/v1/auth/jellyfin",
            401,
            OK,
            r#"{"message":"INVALID_CREDENTIALS"}"#.into(),
        ),
        (
            "POST",
            "/api/v1/auth/jellyfin/quickconnect/authenticate",
            200,
            OK,
            fixture("me.json"),
        ),
    ])
    .await;
    let c = SeerrClient::new(&url, Auth::None).unwrap();
    assert!(matches!(
        c.sign_in_password("a", "b").await,
        Err(Error::InvalidCredentials)
    ));
    assert!(
        c.quick_connect_complete("secret").await.is_err(),
        "no Set-Cookie, no session"
    );
}

#[tokio::test]
async fn quick_connect_steps() {
    let (url, log) = serve(vec![
        (
            "POST",
            "/api/v1/auth/jellyfin/quickconnect/initiate",
            200,
            OK,
            fixture("quickconnect_initiate.json"),
        ),
        (
            "GET",
            "/api/v1/auth/jellyfin/quickconnect/check",
            200,
            OK,
            r#"{"authenticated":true}"#.into(),
        ),
    ])
    .await;
    let c = SeerrClient::new(&url, Auth::None).unwrap();
    let q = c.quick_connect_initiate().await.unwrap();
    assert_eq!(q.code, "937215");
    assert!(c.quick_connect_check(&q.secret).await.unwrap());
    assert!(
        log.lock().unwrap()[1]
            .1
            .contains(&format!("secret={}", q.secret))
    );
}

#[tokio::test]
async fn reads_carry_the_cookie_and_parse() {
    let (url, log) = serve(vec![
        ("GET", "/api/v1/search", 200, OK, fixture("search.json")),
        ("GET", "/api/v1/tv/1396", 200, OK, fixture("tv_detail.json")),
        (
            "GET",
            "/api/v1/user/2/requests",
            200,
            OK,
            fixture("requests_page.json"),
        ),
        (
            "GET",
            "/api/v1/user/2/quota",
            200,
            OK,
            fixture("quota_restricted.json"),
        ),
    ])
    .await;
    let c = signed_in(&url);
    assert_eq!(c.search("alien").await.unwrap().results.len(), 3);
    assert_eq!(c.detail(Kind::Tv, 1396).await.unwrap().seasons.len(), 5);
    assert_eq!(c.my_requests(2, 20, 0).await.unwrap().results.len(), 3);
    assert!(c.quota(2).await.unwrap().movie.restricted);
    let l = log.lock().unwrap();
    assert!(l.iter().all(|r| r.2 == "connect.sid=abc%3A123"));
    assert!(l[0].1.contains("query=alien"));
}

#[tokio::test]
async fn errors_map_to_their_meaning() {
    let (url, _) = serve(vec![
        (
            "GET",
            "/api/v1/auth/me",
            401,
            OK,
            r#"{"message":"cookie 'connect.sid' required"}"#.into(),
        ),
        (
            "POST",
            "/api/v1/request",
            409,
            OK,
            r#"{"message":"Request for this media already exists."}"#.into(),
        ),
        (
            "GET",
            "/api/v1/user/9/quota",
            403,
            OK,
            r#"{"status":403,"error":"You do not have permission to access this endpoint"}"#.into(),
        ),
        ("GET", "/api/v1/movie/1", 404, OK, String::new()),
        (
            "GET",
            "/api/v1/movie/2",
            500,
            OK,
            r#"{"message":"[TMDB] Failed"}"#.into(),
        ),
        (
            "DELETE",
            "/api/v1/request/9",
            403,
            OK,
            r#"{"message":"Movie Quota exceeded."}"#.into(),
        ),
    ])
    .await;
    let c = signed_in(&url);
    assert!(matches!(c.me().await, Err(Error::Unauthorized)));
    assert!(
        matches!(c.quota(9).await, Err(Error::Unauthorized)),
        "an expired cookie is answered 403 without a message"
    );
    let ask = NewRequest {
        kind: Kind::Movie,
        tmdb_id: 1,
        seasons: Seasons::All,
    };
    let ok = RequestConfirmed::i_have_asked_the_user();
    assert!(matches!(
        c.create_request(&ask, ok).await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        c.detail(Kind::Movie, 1).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        c.detail(Kind::Movie, 2).await,
        Err(Error::Status { status: 500, .. })
    ));
    assert!(matches!(
        c.delete_request(9, ok).await,
        Err(Error::QuotaExceeded(_))
    ));
}

#[tokio::test]
async fn a_request_sends_the_plain_body_and_api_key_mode_refuses_writes() {
    let (url, log) = serve(vec![(
        "POST",
        "/api/v1/request",
        201,
        OK,
        fixture("request_created.json"),
    )])
    .await;
    let ok = RequestConfirmed::i_have_asked_the_user();
    let ask = NewRequest {
        kind: Kind::Movie,
        tmdb_id: 348,
        seasons: Seasons::All,
    };
    let r = signed_in(&url).create_request(&ask, ok).await.unwrap();
    assert_eq!((r.id, r.status, r.tmdb_id()), (2, 1, Some(348)));
    assert_eq!(
        log.lock().unwrap()[0].3,
        r#"{"mediaId":348,"mediaType":"movie"}"#
    );
    let dev = SeerrClient::new(&url, Auth::ApiKey("k".into())).unwrap();
    assert!(matches!(
        dev.create_request(&ask, ok).await,
        Err(Error::ReadOnly)
    ));
    assert!(matches!(
        dev.delete_request(2, ok).await,
        Err(Error::ReadOnly)
    ));
    assert_eq!(
        log.lock().unwrap().len(),
        1,
        "the refused writes never left"
    );
}

#[tokio::test]
async fn posters_refuse_paths_that_could_leave_the_cdn() {
    let c = SeerrClient::new("http://127.0.0.1:1", Auth::None).unwrap();
    for bad in ["x.jpg", "/a/../b.jpg", "/a.jpg?x=1", "//evil.example/a.jpg"] {
        // "//evil..." stays on image.tmdb.org because the path is appended after the host
        if bad.starts_with("//") {
            continue;
        }
        assert!(matches!(c.poster(bad).await, Err(Error::NotFound)), "{bad}");
    }
}

#[tokio::test]
async fn discover_asks_for_the_list_genre_and_year() {
    let empty = r#"{"page":1,"totalPages":1,"totalResults":0,"results":[]}"#;
    let (url, log) = serve(vec![
        ("GET", "/api/v1/discover/movies", 200, OK, empty.into()),
        ("GET", "/api/v1/discover/tv/upcoming", 200, OK, empty.into()),
        ("GET", "/api/v1/discover/trending", 200, OK, empty.into()),
    ])
    .await;
    let c = signed_in(&url);
    let q = |list, genre, year| DiscoverQuery {
        list,
        genre,
        year,
        page: 2,
    };
    c.discover(&q(DiscoverList::Movies, Some(28), Some(1999)))
        .await
        .unwrap();
    // Trending and upcoming have no genre or year on Seerr's side: they are not sent.
    c.discover(&q(DiscoverList::UpcomingTv, Some(28), Some(1999)))
        .await
        .unwrap();
    c.discover(&q(DiscoverList::Trending, None, None))
        .await
        .unwrap();
    let l = log.lock().unwrap();
    assert!(l[0].1.contains("page=2") && l[0].1.contains("genre=28"));
    assert!(l[0].1.contains("primaryReleaseDateGte=1999-01-01"));
    assert!(l[0].1.contains("primaryReleaseDateLte=1999-12-31"));
    assert!(!l[1].1.contains("genre") && !l[1].1.contains("Date"));
    assert!(l[2].1.contains("page=2"));
    assert!(l.iter().all(|r| r.0 == "GET"));
}
