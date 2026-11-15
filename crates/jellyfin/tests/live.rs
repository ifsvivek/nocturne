//! Live read-only checks against a real server. Run: `cargo test -- --ignored`.
//! Requires NOCTURNE_JF_TOKEN (e.g. in the workspace-root `.env`).
//! Output policy: counts and booleans only; never print titles, paths, names, ids or tokens.

use nocturne_jellyfin::{Client, Config, ImageType, ItemsQuery};

fn client() -> Client {
    Client::new(Config::load().expect("config")).expect("client")
}

#[tokio::test]
#[ignore]
async fn public_info_and_users() {
    let c = client();
    let info = c.public_info().await.unwrap();
    assert!(!info.version.is_empty());
    let users = c.users().await.unwrap();
    println!("users: {}", users.len());
    assert!(!users.is_empty());
}

#[tokio::test]
#[ignore]
async fn libraries_items_and_detail() {
    let c = client();
    let uid = c.first_user_id().await.unwrap();
    let libs = c.libraries(&uid).await.unwrap();
    println!("libraries: {}", libs.items.len());
    assert!(!libs.items.is_empty());

    let page = c
        .items(
            &uid,
            &ItemsQuery {
                include_types: vec!["Movie".into(), "Series".into()],
                sort_by: Some("SortName".into()),
                limit: Some(5),
                recursive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    println!(
        "page items: {}, total: {}",
        page.items.len(),
        page.total_record_count
    );
    assert!(page.items.len() <= 5);
    let Some(first) = page.items.first() else {
        return;
    };

    let detail = c.item(&uid, &first.id).await.unwrap();
    println!("detail id matches: {}", detail.id == first.id);
    assert_eq!(detail.id, first.id);

    println!(
        "has primary tag: {}",
        first.image_tags.contains_key("Primary")
    );
    let url = c.image_url(&first.id, ImageType::Primary, Some(300), None);
    println!("image url built: {}", url.contains("maxWidth=300"));
}

#[tokio::test]
#[ignore]
async fn seasons_and_episodes() {
    let c = client();
    let uid = c.first_user_id().await.unwrap();
    let series = c
        .items(
            &uid,
            &ItemsQuery {
                include_types: vec!["Series".into()],
                limit: Some(1),
                recursive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let Some(s) = series.items.first() else {
        println!("no series on server; skipped");
        return;
    };
    let seasons = c.seasons(&uid, &s.id).await.unwrap();
    println!("seasons: {}", seasons.items.len());
    let eps = c
        .episodes(&uid, &s.id, seasons.items.first().map(|x| x.id.as_str()))
        .await
        .unwrap();
    println!("episodes in first season: {}", eps.items.len());
    assert!(eps.items.iter().all(|e| e.kind == "Episode"));
}
