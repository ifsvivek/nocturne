//! Live tests against the THROWAWAY Jellyfin only. Run:
//!   scripts/test-jellyfin.sh start
//!   NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-user -- --ignored --test-threads=1
//! Safety: every test starts with `setup()`, which runs the admin crate's `TestServerGuard`
//! (env file from NOCTURNE_JF_TEST_ENV only, loopback :8097, differs from the real server, server
//! data path under /tmp). The repo `.env` is never read for credentials.
//! Output policy: nothing printed beyond pass/fail; no tokens or passwords.

use nocturne_jellyfin::{Client, Config, ItemsQuery};
use nocturne_jellyfin_admin::{ConfigPatch, guard::TestServerGuard};
use nocturne_jellyfin_user::{Error, PlaybackReport, Session, UserClient, UserConfirmed};
use std::time::Duration;

struct Env {
    url: String,
    alice_pw: String,
    bob_pw: String,
}

async fn setup() -> Env {
    let admin = TestServerGuard::connect().await.expect("guard refused");
    // Short fake clips must still keep resume positions.
    admin
        .set_server_config(&ConfigPatch {
            min_resume_duration_seconds: Some(0),
            min_resume_pct: Some(0),
            max_resume_pct: Some(90),
            ..Default::default()
        })
        .await
        .unwrap();
    let path = std::env::var("NOCTURNE_JF_TEST_ENV").unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let var = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .unwrap_or_else(|| {
                panic!("{k} missing in test env (run scripts/test-jellyfin.sh start)")
            })
            .to_string()
    };
    Env {
        url: var("NOCTURNE_JF_TEST_URL"),
        alice_pw: var("NOCTURNE_JF_TEST_ALICE_PW"),
        bob_pw: var("NOCTURNE_JF_TEST_BOB_PW"),
    }
}

async fn login(e: &Env) -> (Session, UserClient) {
    let s = UserClient::authenticate_by_name(&e.url, "alice", &e.alice_pw)
        .await
        .unwrap();
    let c = UserClient::from_session(&e.url, &s).unwrap();
    (s, c)
}

fn reader(e: &Env, s: &Session) -> Client {
    Client::new(Config {
        base_url: e.url.clone(),
        token: s.access_token.clone(),
    })
    .unwrap()
}

/// Ids of the seeded test movies (waits for the library scan).
async fn movies(r: &Client, s: &Session) -> Vec<String> {
    let q = ItemsQuery {
        include_types: vec!["Movie".into()],
        recursive: true,
        sort_by: Some("SortName".into()),
        ..Default::default()
    };
    for _ in 0..60 {
        let items = r.items(&s.user_id, &q).await.unwrap().items;
        if items.len() >= 3 {
            return items.into_iter().map(|i| i.id).collect();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("seeded movies never appeared");
}

/// `UserData.IsFavorite` (not modelled by the read-only client): raw GET with the user's token.
async fn is_favorite(e: &Env, s: &Session, id: &str) -> bool {
    let v: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/Items/{id}?userId={}", e.url, s.user_id))
        .header(
            "Authorization",
            format!("MediaBrowser Token=\"{}\"", s.access_token),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    v["UserData"]["IsFavorite"].as_bool().unwrap()
}

#[tokio::test]
#[ignore]
async fn login_success_and_failure() {
    let e = setup().await;
    let (s, _) = login(&e).await;
    assert!(!s.access_token.is_empty() && !s.user_id.is_empty() && !s.server_id.is_empty());
    assert_eq!(s.user_name, "alice");
    assert!(!s.is_admin);
    let bad = UserClient::authenticate_by_name(&e.url, "alice", "definitely-wrong").await;
    assert!(matches!(bad, Err(Error::InvalidCredentials)));
    let none = UserClient::authenticate_by_name(&e.url, "nobody", "x").await;
    assert!(matches!(none, Err(Error::InvalidCredentials)));
}

#[tokio::test]
#[ignore]
async fn quick_connect_flow() {
    let e = setup().await;
    let (_, alice) = login(&e).await;
    let anon = UserClient::anonymous(&e.url).unwrap();
    assert!(anon.quick_connect_enabled().await.unwrap());
    let qc = anon.quick_connect_initiate().await.unwrap();
    assert!(!qc.code.is_empty() && !qc.secret.is_empty());
    assert!(!anon.quick_connect_poll(&qc.secret).await.unwrap());
    alice.quick_connect_authorize(&qc.code).await.unwrap();
    assert!(anon.quick_connect_poll(&qc.secret).await.unwrap());
    let s = anon
        .authenticate_with_quick_connect(&qc.secret)
        .await
        .unwrap();
    assert!(!s.access_token.is_empty());
    assert_eq!(s.user_name, "alice");
    // the new session is usable, then logs out
    let c = UserClient::from_session(&e.url, &s).unwrap();
    c.logout().await.unwrap();
}

#[tokio::test]
#[ignore]
async fn logout_invalidates_token() {
    let e = setup().await;
    let (s, c) = login(&e).await;
    let r = reader(&e, &s);
    assert!(r.libraries(&s.user_id).await.is_ok());
    c.logout().await.unwrap();
    assert!(matches!(
        r.libraries(&s.user_id).await,
        Err(nocturne_jellyfin::Error::Status { status: 401, .. })
    ));
}

#[tokio::test]
#[ignore]
async fn playback_reporting_sets_resume_then_played() {
    let e = setup().await;
    let (s, c) = login(&e).await;
    let r = reader(&e, &s);
    let id = movies(&r, &s).await.remove(0);
    // Positions scale with the item's runtime, so any seeded clip length works.
    let runtime = r
        .item(&s.user_id, &id)
        .await
        .unwrap()
        .run_time_ticks
        .expect("test movie has a runtime");
    let (resume, finished) = (runtime * 4 / 10, runtime * 975 / 1000);
    c.mark_unplayed(&id).await.unwrap();
    let mut rep = PlaybackReport {
        item_id: id.clone(),
        media_source_id: Some(id.clone()),
        ..Default::default()
    };
    c.report_playback_start(&rep).await.unwrap();
    rep.position_ticks = resume; // 40% in
    c.report_playback_progress(&rep).await.unwrap();
    rep.is_paused = true;
    c.report_playback_progress(&rep).await.unwrap();
    c.report_playback_stopped(&rep).await.unwrap();
    let ud = r.item(&s.user_id, &id).await.unwrap().user_data.unwrap();
    assert_eq!(ud.playback_position_ticks, resume);
    assert!(!ud.played);

    // finishing past the resume threshold marks it played and clears the position
    rep.is_paused = false;
    rep.position_ticks = finished;
    c.report_playback_start(&rep).await.unwrap();
    c.report_playback_stopped(&rep).await.unwrap();
    let ud = r.item(&s.user_id, &id).await.unwrap().user_data.unwrap();
    assert!(ud.played && ud.playback_position_ticks == 0);
    c.mark_unplayed(&id).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn played_and_favorite_toggle() {
    let e = setup().await;
    let (s, c) = login(&e).await;
    let r = reader(&e, &s);
    let id = movies(&r, &s).await.remove(1);
    let played = || async {
        r.item(&s.user_id, &id)
            .await
            .unwrap()
            .user_data
            .unwrap()
            .played
    };
    c.mark_played(&id).await.unwrap();
    assert!(played().await);
    c.mark_unplayed(&id).await.unwrap();
    assert!(!played().await);
    c.set_favorite(&id, true).await.unwrap();
    assert!(is_favorite(&e, &s, &id).await);
    c.set_favorite(&id, false).await.unwrap();
    assert!(!is_favorite(&e, &s, &id).await);
}

#[tokio::test]
#[ignore]
async fn playlist_lifecycle() {
    let e = setup().await;
    let (s, c) = login(&e).await;
    let ids = movies(&reader(&e, &s), &s).await;
    let (a, b, d) = (&ids[0][..], &ids[1][..], &ids[2][..]);
    let pl = c.create_playlist("nocturne-test", &[a, b]).await.unwrap();
    let items = c.playlist_items(&pl).await.unwrap();
    assert_eq!(
        items.iter().map(|i| &i.item_id[..]).collect::<Vec<_>>(),
        [a, b]
    );
    c.add_to_playlist(&pl, &[d]).await.unwrap();
    let items = c.playlist_items(&pl).await.unwrap();
    assert_eq!(items.len(), 3);
    c.move_playlist_item(&pl, &items[2].entry_id, 0)
        .await
        .unwrap();
    let moved = c.playlist_items(&pl).await.unwrap();
    assert_eq!(moved[0].item_id, d);
    c.remove_from_playlist(&pl, &[&moved[1].entry_id])
        .await
        .unwrap();
    assert_eq!(c.playlist_items(&pl).await.unwrap().len(), 2);
    c.delete_playlist(&pl).await.unwrap();
    assert!(c.playlist_items(&pl).await.is_err());
}

#[tokio::test]
#[ignore]
async fn change_own_password_round_trip() {
    let e = setup().await;
    let s = UserClient::authenticate_by_name(&e.url, "bob", &e.bob_pw)
        .await
        .unwrap();
    let c = UserClient::from_session(&e.url, &s).unwrap();
    let yes = UserConfirmed::i_have_asked_the_user;
    let temp = "temp-pw-for-test";
    c.change_own_password(&e.bob_pw, temp, yes()).await.unwrap();
    let old = UserClient::authenticate_by_name(&e.url, "bob", &e.bob_pw).await;
    assert!(matches!(old, Err(Error::InvalidCredentials)));
    let s2 = UserClient::authenticate_by_name(&e.url, "bob", temp)
        .await
        .unwrap();
    // restore the known password so the suite stays re-runnable
    let c2 = UserClient::from_session(&e.url, &s2).unwrap();
    c2.change_own_password(temp, &e.bob_pw, yes())
        .await
        .unwrap();
    UserClient::authenticate_by_name(&e.url, "bob", &e.bob_pw)
        .await
        .unwrap();
}
