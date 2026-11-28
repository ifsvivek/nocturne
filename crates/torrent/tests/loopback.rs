//! Offline end-to-end tests: a seeding engine and a downloading engine talk over loopback. DHT, LSD, UPnP and trackers are off; the downloader is given the seeder's address directly. Nothing leaves the machine.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use nocturne_torrent::{
    AddOptions, Config, Engine, Error, Event, GoalAction, SeedGoal, Source, State, TorrentInfo,
};
use tokio::sync::broadcast;

const WAIT: Duration = Duration::from_secs(60);

fn offline(data: &Path, downloads: &Path) -> Config {
    let mut c = Config::new(data, downloads);
    c.listen_ports = 0..=0;
    c.dht = false;
    c.lsd = false;
    c
}

/// `root/content/{a.bin,b.bin}` with pseudo-random bytes, and its .torrent (no trackers).
async fn make_content(root: &Path) -> Vec<u8> {
    let dir = root.join("content");
    std::fs::create_dir_all(&dir).unwrap();
    let mut x: u32 = 0x1234_5678;
    for (name, len) in [("a.bin", 300_000usize), ("b.bin", 70_000)] {
        let data: Vec<u8> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        std::fs::write(dir.join(name), data).unwrap();
    }
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    let opts = librqbit::CreateTorrentOptions {
        name: None,
        trackers: vec![],
        piece_length: Some(16384),
    };
    let t = librqbit::create_torrent(&dir, opts, &spawner)
        .await
        .unwrap();
    t.as_bytes().unwrap().to_vec()
}

async fn wait_for(
    engine: &Engine,
    id: &str,
    what: &str,
    pred: impl Fn(&TorrentInfo) -> bool,
) -> TorrentInfo {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        if let Some(t) = engine.get(id)
            && pred(&t)
        {
            return t;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}: {:?}",
            engine.get(id).map(|t| t.state)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_event(rx: &mut broadcast::Receiver<Event>, want: Event) {
    tokio::time::timeout(WAIT, async {
        loop {
            if rx.recv().await.unwrap() == want {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {want:?}"));
}

async fn seeder(tmp: &Path, goal: SeedGoal) -> (Engine, Vec<u8>, String, SocketAddr) {
    let src = tmp.join("src");
    let bytes = make_content(&src).await;
    let mut cfg = offline(&tmp.join("seed-data"), &src);
    cfg.seed_goal = goal;
    let engine = Engine::start(cfg).await.unwrap();
    let id = engine
        .add(Source::TorrentFile(bytes.clone()), AddOptions::default())
        .await
        .unwrap();
    wait_for(&engine, &id, "seeder to seed", |t| {
        t.state == State::Seeding
    })
    .await;
    let addr = SocketAddr::from(([127, 0, 0, 1], engine.listen_port().unwrap()));
    (engine, bytes, id, addr)
}

fn same_files(a: &Path, b: &Path) {
    for f in ["a.bin", "b.bin"] {
        assert_eq!(
            std::fs::read(a.join(f)).unwrap(),
            std::fs::read(b.join(f)).unwrap(),
            "{f}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn magnet_download_ratio_goal_and_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let goal = SeedGoal {
        ratio: Some(1.0),
        seeding_time: None,
        action: GoalAction::Pause,
    };
    let (seed, _, id, addr) = seeder(tmp.path(), goal).await;

    let dl_cfg = offline(&tmp.path().join("dl-data"), &tmp.path().join("dl"));
    let dl = Engine::start(dl_cfg.clone()).await.unwrap();
    let mut events = dl.events();
    let magnet = format!("magnet:?xt=urn:btih:{id}&dn=content");
    let opts = AddOptions {
        save_path: Some("tv".into()),
        category: Some("tv".into()),
        peers: vec![addr],
        ..Default::default()
    };
    assert_eq!(dl.add(Source::Magnet(magnet), opts).await.unwrap(), id);
    wait_event(&mut events, Event::MetadataReady { id: id.clone() }).await;
    wait_event(&mut events, Event::Completed { id: id.clone() }).await;

    let t = dl.get(&id).unwrap();
    assert_eq!(t.progress, 1.0);
    assert_eq!(t.category.as_deref(), Some("tv"));
    assert_eq!(t.files.len(), 2);
    let content = tmp
        .path()
        .join("dl")
        .canonicalize()
        .unwrap()
        .join("tv/content");
    assert_eq!(t.content_path, content);
    same_files(&tmp.path().join("src/content"), &content);

    // The seeder uploaded everything once: ratio 1.0 reached, so it stops seeding.
    let s = wait_for(&seed, &id, "seeder goal", |t| t.state == State::Completed).await;
    assert!(s.ratio >= 1.0, "ratio {}", s.ratio);

    // Restart: the torrent comes back complete from fast-resume data, without a second Completed event.
    let completed = t.completed;
    dl.shutdown().await;
    let dl = Engine::start(dl_cfg).await.unwrap();
    let mut events = dl.events();
    let t = wait_for(&dl, &id, "restored seeding", |t| {
        matches!(t.state, State::Seeding | State::Stalled)
    })
    .await;
    assert_eq!(t.state, State::Seeding);
    assert_eq!(t.progress, 1.0);
    assert_eq!(t.completed, completed);
    assert_eq!(t.category.as_deref(), Some("tv"));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        events.try_recv().is_err(),
        "no events after a clean restart"
    );

    dl.remove(&id, true).await.unwrap();
    assert!(dl.list().is_empty());
    assert!(!content.exists(), "delete_files removed the content folder");
    dl.shutdown().await;
    seed.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_and_queue_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let (seed, bytes, id, addr) = seeder(tmp.path(), SeedGoal::default()).await;

    let mut cfg = offline(&tmp.path().join("dl-data"), &tmp.path().join("dl"));
    cfg.max_active_seeds = Some(0);
    let dl = Engine::start(cfg).await.unwrap();
    let opts = AddOptions {
        paused: true,
        peers: vec![addr],
        ..Default::default()
    };
    dl.add(Source::TorrentFile(bytes), opts).await.unwrap();
    wait_for(&dl, &id, "paused", |t| t.state == State::Paused).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let t = dl.get(&id).unwrap();
    assert_eq!((t.state, t.downloaded), (State::Paused, 0));

    dl.resume(&id).await.unwrap();
    // Finished, but with zero seed slots it waits in the queue instead of seeding.
    let t = wait_for(&dl, &id, "finished and queued", |t| {
        t.progress == 1.0 && t.state == State::Queued
    })
    .await;
    assert!(t.completed.is_some());
    same_files(
        &tmp.path().join("src/content"),
        &tmp.path().join("dl/content"),
    );

    dl.recheck(&id).await.unwrap();
    wait_for(&dl, &id, "recheck done", |t| {
        t.progress == 1.0 && t.state == State::Queued
    })
    .await;

    dl.pause(&id).await.unwrap();
    wait_for(&dl, &id, "completed", |t| t.state == State::Completed).await;
    dl.shutdown().await;
    seed.shutdown().await;
}

/// A bencoded single-entry dictionary `d4:info<info>e` around a raw info dict.
fn torrent_with_info(info: &[u8]) -> Vec<u8> {
    [b"d4:info".as_slice(), info, b"e"].concat()
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_escaping_paths_and_local_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let dl = Engine::start(offline(&tmp.path().join("data"), &tmp.path().join("dl")))
        .await
        .unwrap();
    let pieces = [b"6:pieces20:".as_slice(), &[0u8; 20]].concat();
    let crafted = [
        // Multi-file torrent with a `..` path component.
        [
            b"d5:filesld6:lengthi3e4:pathl2:..4:evileee4:name1:x12:piece lengthi16384e".as_slice(),
            &pieces,
            b"e",
        ]
        .concat(),
        // Single file whose name contains a separator.
        [
            b"d6:lengthi3e4:name7:../evil12:piece lengthi16384e".as_slice(),
            &pieces,
            b"e",
        ]
        .concat(),
        // Multi-file torrent whose name (the folder) is `..`.
        [
            b"d5:filesld6:lengthi3e4:pathl1:aeed6:lengthi3e4:pathl1:beee4:name2:..12:piece lengthi16384e".as_slice(),
            &pieces,
            b"e",
        ]
        .concat(),
    ];
    for info in crafted {
        let res = dl
            .add(
                Source::TorrentFile(torrent_with_info(&info)),
                AddOptions::default(),
            )
            .await;
        assert!(
            matches!(res, Err(Error::InvalidTorrent(_) | Error::PathEscape(_))),
            "{res:?}"
        );
    }
    let ok_info = [
        b"d6:lengthi3e4:name1:x12:piece lengthi16384e".as_slice(),
        &pieces,
        b"e",
    ]
    .concat();
    let res = dl
        .add(
            Source::TorrentFile(torrent_with_info(&ok_info)),
            AddOptions {
                save_path: Some("../out".into()),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(res, Err(Error::PathEscape(_))), "{res:?}");
    let huge = vec![b'd'; nocturne_torrent::MAX_TORRENT_BYTES + 1];
    assert!(matches!(
        dl.add(Source::TorrentFile(huge), AddOptions::default())
            .await,
        Err(Error::InvalidTorrent(_))
    ));
    for m in [
        "file:///etc/passwd",
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=file:///etc/passwd",
    ] {
        assert!(
            matches!(
                dl.add(Source::Magnet(m.into()), AddOptions::default())
                    .await,
                Err(Error::InvalidMagnet(_))
            ),
            "{m}"
        );
    }
    assert!(dl.list().is_empty());
    assert!(!tmp.path().join("evil").exists() && !tmp.path().join("out").exists());
    dl.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_bind_interface_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = offline(&tmp.path().join("data"), &tmp.path().join("dl"));
    cfg.bind_interface = Some("nocturne-none0".into());
    assert!(
        matches!(Engine::start(cfg).await, Err(Error::InterfaceDown(n)) if n == "nocturne-none0")
    );
}
