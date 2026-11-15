//! Live tests against the THROWAWAY Jellyfin only (guard first), like `live.rs`:
//!   NOCTURNE_JF_TEST_ENV=/tmp/.../test.env cargo test -p nocturne-jellyfin-user --test profile_live -- --ignored
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::{Session, UserClient, UserConfirmed};

fn var(k: &str) -> String {
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap()
        .to_string()
}

async fn login(name: &str, pw_key: &str) -> (String, Session, UserClient) {
    let _admin = TestServerGuard::connect().await.expect("guard refused");
    let url = var("NOCTURNE_JF_TEST_URL");
    let s = UserClient::authenticate_by_name(&url, name, &var(pw_key))
        .await
        .unwrap();
    let c = UserClient::from_session(&url, &s).unwrap();
    (url, s, c)
}

#[tokio::test]
#[ignore]
async fn own_image_and_display_language() {
    let (_url, _s, c) = login("alice", "NOCTURNE_JF_TEST_ALICE_PW").await;
    let png = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=green:s=32x32",
            "-frames:v",
            "1",
            "-f",
            "image2",
            "-c:v",
            "png",
            "pipe:1",
        ])
        .output()
        .unwrap()
        .stdout;
    assert!(c.set_own_image(b"not an image").await.is_err());
    c.remove_own_image(UserConfirmed::i_have_asked_the_user())
        .await
        .ok();
    assert!(c.profile().await.unwrap().primary_image_tag.is_none());
    c.set_own_image(&png).await.unwrap();
    assert!(c.profile().await.unwrap().primary_image_tag.is_some());
    c.remove_own_image(UserConfirmed::i_have_asked_the_user())
        .await
        .unwrap();
    assert!(c.profile().await.unwrap().primary_image_tag.is_none());

    // language: offered list, refused unknown, set, read back, other prefs kept, clear
    let opts = c.language_options().await.unwrap();
    assert!(opts.iter().any(|o| o.value == "en-US") && opts.iter().any(|o| o.value == "de"));
    assert!(c.set_display_language("xx-nope").await.is_err());
    c.set_display_language("de").await.unwrap();
    assert_eq!(c.display_language().await.unwrap().as_deref(), Some("de"));
    c.set_display_language("").await.unwrap();
    assert_eq!(c.display_language().await.unwrap(), None);
}

/// Two accounts on one server hold their own tokens at the same time, so switching between
/// them needs no password: each stored token keeps working until it is signed out.
#[tokio::test]
#[ignore]
async fn two_stored_tokens_work_side_by_side() {
    let (url, a, ca) = login("alice", "NOCTURNE_JF_TEST_ALICE_PW").await;
    let (_, b, cb) = login("bob", "NOCTURNE_JF_TEST_BOB_PW").await;
    let me_a = ca.me(a.clone()).await.unwrap();
    let me_b = cb.me(b.clone()).await.unwrap();
    assert_eq!(
        (me_a.user_name.as_str(), me_b.user_name.as_str()),
        ("alice", "bob")
    );
    // a token that is revoked answers 401, which the app treats as "sign in again"
    ca.logout().await.unwrap();
    let dead = UserClient::from_session(&url, &a).unwrap().me(a).await;
    assert!(matches!(
        dead,
        Err(nocturne_jellyfin_user::Error::Status { status: 401, .. })
    ));
    cb.me(b).await.unwrap();
    cb.logout().await.unwrap();
}
