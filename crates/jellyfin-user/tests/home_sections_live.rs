//! Live test against the THROWAWAY Jellyfin only (guard first):
//!   NOCTURNE_JF_TEST_ENV=/tmp/.../test.env cargo test -p nocturne-jellyfin-user --test home_sections_live -- --ignored
use nocturne_jellyfin::{Client, Config};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::{Session, UserClient};

fn var(k: &str) -> String {
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap()
        .to_string()
}

const DEFAULT: [&str; 10] = [
    "smalllibrarytiles",
    "resume",
    "resumeaudio",
    "resumebook",
    "livetv",
    "nextup",
    "latestmedia",
    "none",
    "none",
    "none",
];

#[tokio::test]
#[ignore]
async fn home_sections_round_trip_keeping_other_prefs() {
    let _admin = TestServerGuard::connect().await.expect("guard refused");
    let url = var("NOCTURNE_JF_TEST_URL");
    let s: Session =
        UserClient::authenticate_by_name(&url, "alice", &var("NOCTURNE_JF_TEST_ALICE_PW"))
            .await
            .unwrap();
    let c = UserClient::from_session(&url, &s).unwrap();
    let reader = Client::new(Config {
        base_url: url.clone(),
        token: s.access_token.clone(),
    })
    .unwrap();

    let before = reader.display_prefs(&s.user_id).await.unwrap();
    // invalid lists are refused before anything is sent
    assert!(c.set_home_sections(&["resume"]).await.is_err());
    assert!(c.set_home_sections(&["bogus"; 10]).await.is_err());

    let mut mine = DEFAULT;
    mine.swap(0, 6); // latestmedia first
    mine[1] = "none";
    c.set_home_sections(&mine).await.unwrap();
    let after = reader.display_prefs(&s.user_id).await.unwrap();
    for (i, v) in mine.iter().enumerate() {
        assert_eq!(after.get(&format!("homesection{i}")), Some(*v), "slot {i}");
    }
    // every key that was there before is still there, unchanged
    for (k, v) in &before.custom_prefs {
        if !k.starts_with("homesection") {
            assert_eq!(after.custom_prefs.get(k), Some(v), "kept {k}");
        }
    }
    // put the server default back
    c.set_home_sections(&DEFAULT).await.unwrap();
    let back = reader.display_prefs(&s.user_id).await.unwrap();
    assert_eq!(back.get("homesection0"), Some("smalllibrarytiles"));
}
