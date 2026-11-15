//! Live test against the THROWAWAY Jellyfin only (guard first), like `live.rs`:
//!   NOCTURNE_JF_TEST_ENV=/tmp/.../test.env cargo test -p nocturne-jellyfin-user --test sessions_live -- --ignored
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::UserClient;

#[tokio::test]
#[ignore]
async fn own_sessions_lists_this_device_only_for_this_user() {
    let admin = TestServerGuard::connect().await.expect("guard refused");
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    let var = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .unwrap()
            .to_string()
    };
    let url = var("NOCTURNE_JF_TEST_URL");
    let s = UserClient::authenticate_by_name(&url, "alice", &var("NOCTURNE_JF_TEST_ALICE_PW"))
        .await
        .unwrap();
    // an administrator-visible session of another user must not leak into alice's list
    let bob = UserClient::authenticate_by_name(&url, "bob", &var("NOCTURNE_JF_TEST_BOB_PW"))
        .await
        .unwrap();
    assert!(admin.sessions().await.unwrap().len() >= 2);
    let c = UserClient::from_session(&url, &s).unwrap();
    let mine = c.own_sessions().await.unwrap();
    assert!(!mine.is_empty() && mine.iter().all(|x| x.user_id == s.user_id));
    assert!(mine[0].current, "this app's session comes first");
    assert!(mine.iter().all(|x| x.user_id != bob.user_id));
    c.logout().await.unwrap();
    UserClient::from_session(&url, &bob)
        .unwrap()
        .logout()
        .await
        .unwrap();
}
