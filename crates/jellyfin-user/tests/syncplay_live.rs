//! Live SyncPlay test against the THROWAWAY Jellyfin only (guard first), like `live.rs`:
//!   NOCTURNE_JF_TEST_ENV=/tmp/.../test.env cargo test -p nocturne-jellyfin-user --test syncplay_live -- --ignored --nocapture
//! Needs at least one movie in the library and the users alice and bob (scripts/test-jellyfin.sh).
use nocturne_jellyfin::{Client, Config, ItemsQuery};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::syncplay::ws::{self, SocketEvent};
use nocturne_jellyfin_user::syncplay::*;
use nocturne_jellyfin_user::{UserClient, UserConfirmed};
use std::sync::mpsc::Receiver;
use std::time::Duration;

fn var(k: &str) -> String {
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap()
        .to_string()
}

struct Member {
    api: UserClient,
    rx: Receiver<SocketEvent>,
    task: tokio::task::JoinHandle<()>,
}

impl Member {
    async fn new(url: &str, name: &str, pw_key: &str) -> Self {
        // The server keys sessions by app + device id: each member needs its own.
        unsafe { std::env::set_var("NOCTURNE_DEVICE_ID", format!("nocturne-synctest-{name}")) };
        let s = UserClient::authenticate_by_name(url, name, &var(pw_key))
            .await
            .unwrap();
        let api = UserClient::from_session(url, &s).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(ws::run(api.clone(), tx));
        Self { api, rx, task }
    }

    /// Waits for a message matching `f`; everything else seen on the way is printed.
    async fn wait<T>(&self, what: &str, mut f: impl FnMut(&Incoming) -> Option<T>) -> T {
        let end = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < end {
            while let Ok(e) = self.rx.try_recv() {
                eprintln!("  [{what}] {e:?}");
                if let SocketEvent::Message(m) = &e
                    && let Some(v) = f(m)
                {
                    return v;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for {what}");
    }
}

#[tokio::test]
#[ignore]
async fn two_members_group_lifecycle() {
    let _guard = TestServerGuard::connect().await.expect("guard refused");
    let c = UserConfirmed::i_have_asked_the_user();
    let url = var("NOCTURNE_JF_TEST_URL");
    let alice = Member::new(&url, "alice", "NOCTURNE_JF_TEST_ALICE_PW").await;
    let bob = Member::new(&url, "bob", "NOCTURNE_JF_TEST_BOB_PW").await;
    tokio::time::sleep(Duration::from_millis(1500)).await; // let both sockets connect
    assert!(alice.api.syncplay_access().await.unwrap().can_create());

    // Clock: loopback, so the offset must be tiny and the delay small.
    let mut t = TimeSync::default();
    for _ in 0..4 {
        t.add(alice.api.syncplay_time_sample().await.unwrap());
    }
    eprintln!("clock offset {:.2} ms, ping {} ms", t.offset(), t.ping());
    assert!(t.offset().abs() < 50.0);

    // alice creates, bob sees it listed and joins.
    alice.api.syncplay_new("live-sync-test", c).await.unwrap();
    let g = alice
        .wait("GroupJoined", |m| match m {
            Incoming::Group(GroupUpdate::GroupJoined(g)) => Some(g.clone()),
            _ => None,
        })
        .await;
    assert_eq!(g.group_name, "live-sync-test");
    let listed = bob.api.syncplay_list().await.unwrap();
    assert!(listed.iter().any(|x| x.group_id == g.group_id));
    bob.api.syncplay_join(&g.group_id, c).await.unwrap();
    bob.wait("bob GroupJoined", |m| {
        matches!(m, Incoming::Group(GroupUpdate::GroupJoined(_))).then_some(())
    })
    .await;
    alice
        .wait("alice sees bob", |m| match m {
            Incoming::Group(GroupUpdate::UserJoined(n)) if n == "bob" => Some(()),
            _ => None,
        })
        .await;

    // A queue from the library, started for both.
    let reader = Client::new(Config {
        base_url: url.clone(),
        token: var("NOCTURNE_JF_TEST_TOKEN"),
    })
    .unwrap();
    let uid = reader.first_user_id().await.unwrap();
    let movies = reader
        .items(
            &uid,
            &ItemsQuery {
                include_types: vec!["Movie".into()],
                recursive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let ids: Vec<String> = movies.items.iter().take(2).map(|i| i.id.clone()).collect();
    alice
        .api
        .syncplay_set_new_queue(
            &NewQueue {
                item_ids: ids.clone(),
                start_index: 0,
                start_ticks: 0,
            },
            c,
        )
        .await
        .unwrap();
    let mut qs = Vec::new();
    for m in [&alice, &bob] {
        qs.push(
            m.wait("PlayQueue", |m| match m {
                Incoming::Group(GroupUpdate::PlayQueue(q)) => Some(q.clone()),
                _ => None,
            })
            .await,
        );
    }
    assert_eq!(qs[0], qs[1], "both members get the same queue");
    assert_eq!(qs[0].reason, QueueReason::NewPlaylist);
    assert_eq!(qs[0].playlist.len(), ids.len());
    let pli = qs[0].playing().unwrap().playlist_item_id.clone();

    // Both report ready: the group starts (Unpause) for both with the same When.
    for m in [&alice, &bob] {
        let r = Report {
            when_ms: now_ms() + t.offset(),
            pos_ticks: 0,
            playing: false,
            playlist_item_id: pli.clone(),
        };
        m.api.syncplay_report(false, &r, c).await.unwrap();
    }
    let mut starts = Vec::new();
    for m in [&alice, &bob] {
        starts.push(
            m.wait("Unpause", |m| match m {
                Incoming::Command(c) if c.command == CommandKind::Unpause => Some(c.clone()),
                _ => None,
            })
            .await,
        );
    }
    assert_eq!(starts[0].when_ms, starts[1].when_ms);
    eprintln!(
        "group start leads emission by {:.0} ms",
        starts[0].when_ms - starts[0].emitted_at_ms
    );

    // Pause by one member reaches both with the same position.
    alice.api.syncplay_pause(c).await.unwrap();
    let mut pauses = Vec::new();
    for m in [&alice, &bob] {
        pauses.push(
            m.wait("Pause", |m| match m {
                Incoming::Command(c) if c.command == CommandKind::Pause => Some(c.clone()),
                _ => None,
            })
            .await,
        );
    }
    assert_eq!(pauses[0].position_ticks, pauses[1].position_ticks);

    // Bob seeks: everyone gets a Seek command to that position.
    bob.api.syncplay_seek(600_000_000, c).await.unwrap();
    for m in [&alice, &bob] {
        let s = m
            .wait("Seek", |m| match m {
                Incoming::Command(c) if c.command == CommandKind::Seek => Some(c.clone()),
                _ => None,
            })
            .await;
        assert_eq!(s.position_ticks, 600_000_000);
    }

    // Leaving: the other member is told, then the empty group is gone.
    bob.api.syncplay_leave(c).await.unwrap();
    alice
        .wait("alice sees bob leave", |m| match m {
            Incoming::Group(GroupUpdate::UserLeft(n)) if n == "bob" => Some(()),
            _ => None,
        })
        .await;
    alice.api.syncplay_leave(c).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        alice
            .api
            .syncplay_list()
            .await
            .unwrap()
            .iter()
            .all(|x| x.group_id != g.group_id)
    );
    alice.task.abort();
    bob.task.abort();
}
