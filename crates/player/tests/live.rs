//! Live check: plays a few seconds of the first movie/episode, vo=null ao=null. GET only.
//! Run: `cargo test -- --ignored`. Output policy: booleans/numbers only; never titles, ids or URLs.

use nocturne_jellyfin::{Client, Config, ItemsQuery};
use nocturne_player::{Options, Player};
use std::time::{Duration, Instant};

#[tokio::test]
#[ignore]
async fn plays_first_item_and_position_advances() {
    let c = Client::new(Config::load().expect("config")).expect("client");
    let uid = c.first_user_id().await.unwrap();
    let page = c
        .items(
            &uid,
            &ItemsQuery {
                include_types: vec!["Movie".into(), "Episode".into()],
                limit: Some(1),
                recursive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let id = &page.items.first().expect("at least one playable item").id;

    let p = Player::new(Options { headless: true }).unwrap();
    p.load(
        &c.stream_url(id),
        Some(&format!("Authorization: {}", c.auth_header())),
    )
    .unwrap();

    let t = Instant::now();
    let mut start = None;
    let advanced = loop {
        let s = p.state();
        if let Some(pos) = s.position {
            let first = *start.get_or_insert(pos);
            if pos > first + 1.5 {
                break true;
            }
        }
        if t.elapsed() > Duration::from_secs(30) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let s = p.state();
    println!(
        "duration known: {}, advanced: {advanced}, eof: {}",
        s.duration.is_some(),
        s.eof
    );
    assert!(advanced);
    assert!(s.duration.is_some());
}
