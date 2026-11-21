//! Live tests against the THROWAWAY Seerr of `scripts/test-seerr.sh` (and the throwaway Jellyfin it
//! is attached to). Ignored by default; they print booleans and counts only, never a credential.
//!
//!   scripts/test-jellyfin.sh start && scripts/test-seerr.sh start
//!   NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env NOCTURNE_SEERR_TEST_ENV=/tmp/nocturne-seerr-test/test.env \
//!     cargo test -p nocturne-seerr --test live -- --ignored --test-threads=1
//!
//! Every test starts with `SeerrTestGuard::connect()`, which refuses anything but a loopback Seerr
//! with its data under /tmp, and the Jellyfin behind it must pass the Jellyfin test guard too.
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_seerr::guard::{SeerrTestGuard, TestEnv};
use nocturne_seerr::*;
use std::time::Duration;

/// Guards, then a signed-out client.
async fn start() -> (TestEnv, SeerrClient) {
    let env = SeerrTestGuard::connect().expect("seerr guard");
    TestServerGuard::connect().await.expect("jellyfin guard");
    let c = SeerrClient::new(&env.url, Auth::None).unwrap();
    (env, c)
}

async fn alice(env: &TestEnv, c: &SeerrClient) -> (Me, SeerrClient) {
    let (me, cookie) = c
        .sign_in_password("alice", &env.alice_password)
        .await
        .expect("alice signs in");
    (
        me,
        SeerrClient::new(&env.url, Auth::Cookie(cookie)).unwrap(),
    )
}

/// TMDB is reached through Seerr and hiccups now and then (the first search after a start can be empty).
async fn search_retry(c: &SeerrClient, q: &str) -> SearchPage {
    for _ in 0..4 {
        if let Ok(p) = c.search(q).await
            && !p.results.is_empty()
        {
            return p;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("no search results for the test query");
}

async fn detail_retry(c: &SeerrClient, k: Kind, id: u64) -> Detail {
    for _ in 0..4 {
        if let Ok(d) = c.detail(k, id).await {
            return d;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("no details");
}

async fn cancel_all(c: &SeerrClient, me: &Me) {
    let ok = RequestConfirmed::i_have_asked_the_user();
    for r in c.my_requests(me.id, 50, 0).await.unwrap().results {
        let _ = c.delete_request(r.id, ok).await;
    }
}

#[tokio::test]
#[ignore = "needs the throwaway Seerr"]
async fn sign_in_wrong_password_and_public_reads() {
    let (env, c) = start().await;
    let s = c.status().await.unwrap();
    let p = c.public_settings().await.unwrap();
    println!(
        "version known: {}, initialized: {}, jellyfin: {}",
        !s.version.is_empty(),
        p.initialized,
        p.media_server_type == 2
    );
    assert!(p.initialized && p.media_server_login);
    assert!(matches!(
        c.sign_in_password("alice", "definitely-wrong").await,
        Err(Error::InvalidCredentials)
    ));
    let (me, a) = alice(&env, &c).await;
    println!(
        "alice id: {}, can request: {}",
        me.id,
        perm::has(me.permissions, perm::REQUEST)
    );
    assert!(perm::has(me.permissions, perm::REQUEST));
    assert_eq!(a.me().await.unwrap().id, me.id);
    // A cookie that is not ours is a plain 401.
    let bogus =
        SeerrClient::new(&env.url, Auth::Cookie(SessionCookie::from_value("nope"))).unwrap();
    assert!(matches!(bogus.me().await, Err(Error::Unauthorized)));
}

#[tokio::test]
#[ignore = "needs the throwaway Seerr"]
async fn quick_connect_starts_and_a_wrong_secret_is_refused() {
    let (_env, c) = start().await;
    let q = c.quick_connect_initiate().await.unwrap();
    println!(
        "code length: {}, secret present: {}",
        q.code.len(),
        !q.secret.is_empty()
    );
    assert!(!q.code.is_empty() && !q.secret.is_empty());
    assert!(
        !c.quick_connect_check(&q.secret).await.unwrap(),
        "nobody authorised it"
    );
    assert!(c.quick_connect_complete("0000").await.is_err());
}

#[tokio::test]
#[ignore = "needs the throwaway Seerr"]
async fn search_details_request_duplicate_list_and_cancel() {
    let (env, c) = start().await;
    let (me, a) = alice(&env, &c).await;
    cancel_all(&a, &me).await;
    let ok = RequestConfirmed::i_have_asked_the_user();

    let p = search_retry(&a, "alien").await;
    let movie = p
        .results
        .iter()
        .find(|h| h.kind() == Some(Kind::Movie) && h.id == 348)
        .expect("Alien (1979) in the results");
    println!(
        "results: {}, first is a title: {}",
        p.results.len(),
        !movie.display_title().is_empty()
    );
    let d = detail_retry(&a, Kind::Movie, 348).await;
    assert_eq!(d.display_title(), "Alien");

    let ask = NewRequest {
        kind: Kind::Movie,
        tmdb_id: 348,
        seasons: Seasons::All,
    };
    let r = a.create_request(&ask, ok).await.expect("request created");
    println!("created, pending: {}", r.status == 1);
    assert_eq!(
        (r.status, r.tmdb_id()),
        (1, Some(348)),
        "a normal user's request waits for approval"
    );
    assert!(
        matches!(a.create_request(&ask, ok).await, Err(Error::Conflict)),
        "the same title twice"
    );

    let mine = a.my_requests(me.id, 20, 0).await.unwrap();
    assert!(mine.results.iter().any(|x| x.id == r.id));
    let again = detail_retry(&a, Kind::Movie, 348).await;
    assert!(
        again.media_info.is_some_and(|m| m.status >= 2),
        "the title now carries a status"
    );

    a.delete_request(r.id, ok)
        .await
        .expect("own pending request cancelled");
    let after = a.my_requests(me.id, 20, 0).await.unwrap();
    assert!(!after.results.iter().any(|x| x.id == r.id));
    println!("cancelled: true");
}

#[tokio::test]
#[ignore = "needs the throwaway Seerr"]
async fn a_show_request_carries_its_seasons() {
    let (env, c) = start().await;
    let (me, a) = alice(&env, &c).await;
    cancel_all(&a, &me).await;
    let ok = RequestConfirmed::i_have_asked_the_user();
    let d = detail_retry(&a, Kind::Tv, 1396).await;
    let seasons = d.seasons.iter().filter(|s| s.season_number > 0).count();
    println!("seasons listed: {seasons}");
    assert!(seasons >= 2);
    let ask = NewRequest {
        kind: Kind::Tv,
        tmdb_id: 1396,
        seasons: Seasons::Some(vec![1, 2]),
    };
    let r = a
        .create_request(&ask, ok)
        .await
        .expect("show request created");
    let got: Vec<u32> = r.seasons.iter().map(|s| s.season_number).collect();
    println!("seasons requested: {}", got.len());
    assert_eq!(got, [1, 2]);
    a.delete_request(r.id, ok).await.unwrap();
}

#[tokio::test]
#[ignore = "needs the throwaway Seerr"]
async fn a_used_up_quota_is_refused_with_its_own_error() {
    let (env, c) = start().await;
    let (me, a) = alice(&env, &c).await;
    cancel_all(&a, &me).await;
    // The administrator gives alice a quota of one movie a week (admin write on the THROWAWAY server only).
    let (_, admin_cookie) = c
        .sign_in_password(&env.admin_user, &env.admin_password)
        .await
        .expect("admin signs in");
    let http = reqwest::Client::new();
    let set = |limit: serde_json::Value, days: serde_json::Value| {
        let (http, url, id) = (http.clone(), env.url.clone(), me.id);
        let cookie = format!("connect.sid={}", admin_cookie.value());
        async move {
            http.post(format!("{url}/api/v1/user/{id}/settings/main"))
                .header("Cookie", cookie)
                .json(&serde_json::json!({"movieQuotaLimit": limit, "movieQuotaDays": days}))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        }
    };
    assert!(set(1.into(), 7.into()).await, "quota set");
    let ok = RequestConfirmed::i_have_asked_the_user();
    let first = a
        .create_request(
            &NewRequest {
                kind: Kind::Movie,
                tmdb_id: 348,
                seasons: Seasons::All,
            },
            ok,
        )
        .await;
    let second = a
        .create_request(
            &NewRequest {
                kind: Kind::Movie,
                tmdb_id: 603,
                seasons: Seasons::All,
            },
            ok,
        )
        .await;
    let q = a.quota(me.id).await.unwrap();
    println!(
        "first ok: {}, second refused as quota: {}, quota restricted: {}",
        first.is_ok(),
        matches!(second, Err(Error::QuotaExceeded(_))),
        q.movie.restricted
    );
    // Restore before asserting so a failure does not leave the throwaway restricted.
    set(serde_json::Value::Null, serde_json::Value::Null).await;
    cancel_all(&a, &me).await;
    assert!(first.is_ok());
    assert!(matches!(second, Err(Error::QuotaExceeded(_))));
    assert!(q.movie.restricted && q.movie.remaining == Some(0));
}
