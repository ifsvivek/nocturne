//! One watched state across sources, against a THROWAWAY Jellyfin (server + a local library holding the
//! same title, matched by the IMDb id in both folder names). Run:
//!   NOCTURNE_JF_TEST_DIR=/tmp/x NOCTURNE_JF_TEST_PORT=8105 scripts/test-jellyfin.sh start   (with the media
//!   folder `Cascade Film (2011) [imdbid-tt9000011]` created under $T/media/Movies first)
//!   NOCTURNE_JF_TEST_ENV=/tmp/x/test.env NOCTURNE_JF_TEST_PORT=8105 cargo test -p nocturne-core \
//!     --test live_watched -- --ignored
//! Writes go only to the guarded throwaway server (`TestServerGuard`) and a temp local library.
use nocturne_core::backend::Backend;
use nocturne_core::local::LocalBackend;
use nocturne_core::source::{Media, Source, SourceId};
use nocturne_core::watched::{Cause, Change, Gate, Via, find_copies, plan};
use nocturne_jellyfin::{BrowseQuery, Config};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::UserClient;
use nocturne_local::Library;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const SERVER: SourceId = SourceId(1);
const LOCAL: SourceId = SourceId(2);

async fn find(b: &Backend, tries: u32) -> Media {
    for _ in 0..tries {
        let q = BrowseQuery {
            include_types: vec!["Movie".into()],
            recursive: true,
            ..Default::default()
        };
        if let Ok((v, _)) = b.browse(&q).await
            && let Some(m) = v.into_iter().find(|m| m.title.starts_with("Cascade Film"))
        {
            return m;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("the title never showed up");
}

#[tokio::test]
#[ignore = "needs a throwaway Jellyfin: NOCTURNE_JF_TEST_ENV (+ NOCTURNE_JF_TEST_PORT)"]
async fn a_watched_change_reaches_the_other_copy_by_identity_and_gates() {
    TestServerGuard::connect().await.expect("guard refused");
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    let var = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .unwrap()
            .to_string()
    };
    let url = var("NOCTURNE_JF_TEST_URL");
    let sess = UserClient::authenticate_by_name(&url, "alice", &var("NOCTURNE_JF_TEST_ALICE_PW"))
        .await
        .unwrap();
    let user = UserClient::from_session(&url, &sess).unwrap();
    let source = Source::with_user(
        Config {
            base_url: url.clone(),
            token: sess.access_token.clone(),
        },
        &sess.user_id,
    )
    .unwrap()
    .with_origin(SERVER);
    let server = Backend::Jellyfin(source);

    // A local library with the same title (same IMDb id in the folder name).
    let dir = std::env::temp_dir().join(format!("nocturne-live-watched-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let f = dir.join("media/Films/Cascade Film (2011) [imdbid-tt9000011]");
    std::fs::create_dir_all(&f).unwrap();
    std::fs::write(
        f.join("Cascade Film (2011) [imdbid-tt9000011].mkv"),
        b"fake",
    )
    .unwrap();
    // A different title that must never be matched.
    let g = dir.join("media/Films/Other Film (2011)");
    std::fs::create_dir_all(&g).unwrap();
    std::fs::write(g.join("Other Film (2011).mkv"), b"fake2").unwrap();
    let mut lib = Library::open(dir.join("data")).unwrap();
    lib.scan(&[dir.join("media/Films")], &AtomicBool::new(false), |_| {})
        .unwrap();
    let l: LocalBackend = LocalBackend::new(LOCAL, lib);
    let local = Backend::Local(l.clone());
    let backends = vec![(SERVER, server.clone()), (LOCAL, local.clone())];

    let s_item = find(&server, 30).await;
    let l_item = find(&local, 1).await;
    assert!(
        !s_item.ids.is_empty(),
        "the server read the id from the folder name"
    );

    // Identity: each side finds exactly the other, nothing else.
    let from_server = find_copies(&s_item, &backends).await;
    let from_local = find_copies(&l_item, &backends).await;
    assert_eq!(from_server.len(), 1);
    assert_eq!(
        (from_server[0].origin, from_server[0].id.clone()),
        (LOCAL, l_item.id.clone())
    );
    assert_eq!(from_local.len(), 1);
    assert_eq!(
        (from_local[0].origin, from_local[0].id.clone()),
        (SERVER, s_item.id.clone())
    );

    let via = |o: SourceId| Some(if o == SERVER { Via::Server } else { Via::Local });
    let gate = |enabled, writes| Gate {
        enabled,
        allow_server_writes: writes,
        dev_token: false,
        sync_progress: false,
    };
    let steps = |from: &Media, copies: &[Media], v, g| {
        plan(
            (from.origin, &from.id),
            copies,
            Change::Played(v),
            Cause::User,
            g,
            via,
        )
    };

    // OFF: nothing follows.
    assert!(steps(&l_item, &from_local, true, gate(false, true)).is_empty());

    // ON, server changes ON: marking the local copy watched marks the server copy.
    let st = steps(&l_item, &from_local, true, gate(true, true));
    assert_eq!(st.len(), 1);
    assert_eq!((st[0].origin, st[0].via), (SERVER, Via::Server));
    user.mark_played(&st[0].id).await.unwrap();
    let (m, _) = server.full(&s_item.id).await.unwrap();
    assert!(m.played, "the server copy followed");

    // ON, server changes OFF: from the server copy only the local one follows; the other direction
    // reaches nothing (the server is skipped silently).
    assert!(steps(&l_item, &from_local, false, gate(true, false)).is_empty());
    let st = steps(&s_item, &from_server, false, gate(true, false));
    assert_eq!(st.len(), 1);
    assert_eq!((st[0].origin, st[0].via), (LOCAL, Via::Local));
    l.set_played(&from_server[0], true).await.unwrap();
    assert!(
        local.full(&l_item.id).await.unwrap().0.played,
        "the local copy followed"
    );
    l.set_played(&from_server[0], false).await.unwrap();
    user.mark_unplayed(&s_item.id).await.unwrap();
    assert!(!server.full(&s_item.id).await.unwrap().0.played);

    let _ = std::fs::remove_dir_all(dir);
}
