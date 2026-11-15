//! Live test against the THROWAWAY Jellyfin only (guard first), like `live.rs`:
//!   NOCTURNE_JF_TEST_ENV=/tmp/.../test.env cargo test -p nocturne-jellyfin-user --test playlists_live -- --ignored
use nocturne_jellyfin::{Client, Config, ItemsQuery};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::{Error, UserClient};

fn var(k: &str) -> String {
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap()
        .to_string()
}

#[tokio::test]
#[ignore]
async fn own_playlist_lifecycle_and_foreign_playlist_is_refused() {
    let _guard = TestServerGuard::connect().await.expect("guard refused");
    let url = var("NOCTURNE_JF_TEST_URL");
    let a = UserClient::authenticate_by_name(&url, "alice", &var("NOCTURNE_JF_TEST_ALICE_PW"))
        .await
        .unwrap();
    let b = UserClient::authenticate_by_name(&url, "bob", &var("NOCTURNE_JF_TEST_BOB_PW"))
        .await
        .unwrap();
    let (ca, cb) = (
        UserClient::from_session(&url, &a).unwrap(),
        UserClient::from_session(&url, &b).unwrap(),
    );
    let reader = Client::new(Config {
        base_url: url.clone(),
        token: a.access_token.clone(),
    })
    .unwrap();
    let movies = reader
        .items(
            &a.user_id,
            &ItemsQuery {
                include_types: vec!["Movie".into()],
                recursive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let ids: Vec<&str> = movies.items.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.len() >= 3);

    let p = ca
        .create_playlist("nocturne-live-pl", &ids[..2])
        .await
        .unwrap();
    ca.add_to_playlist(&p, &ids[2..3]).await.unwrap();
    let order = || async {
        ca.playlist_items(&p)
            .await
            .unwrap()
            .into_iter()
            .map(|e| (e.entry_id, e.item_id))
            .collect::<Vec<_>>()
    };
    let l = order().await;
    assert_eq!(l.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(), ids[..3]);
    // move the last entry to the top, then remove the (new) second one
    ca.move_playlist_item(&p, &l[2].0, 0).await.unwrap();
    let l2 = order().await;
    assert_eq!(l2[0].1, ids[2]);
    ca.remove_from_playlist(&p, &[l2[1].0.as_str()])
        .await
        .unwrap();
    let l3 = order().await;
    assert_eq!(l3.len(), 2);
    ca.rename_playlist(&p, "nocturne-live-pl-2").await.unwrap();
    let it = reader.item(&a.user_id, &p).await.unwrap();
    assert_eq!(it.name, "nocturne-live-pl-2");
    assert_eq!(it.x.can_delete, Some(true), "the owner may delete it");

    // another user sees it but may not change it
    assert!(matches!(
        cb.add_to_playlist(&p, &ids[..1]).await,
        Err(Error::Status { status: 403, .. })
    ));
    assert!(matches!(
        cb.rename_playlist(&p, "mine now").await,
        Err(Error::Status { status: 403, .. })
    ));
    ca.delete_playlist(&p).await.unwrap();
    assert!(reader.item(&a.user_id, &p).await.is_err());
    ca.logout().await.unwrap();
    cb.logout().await.unwrap();
}
