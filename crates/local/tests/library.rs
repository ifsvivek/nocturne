//! End-to-end checks on temporary fixture trees (tiny files, no ffprobe needed).

use nocturne_local::{
    Kind, Library, LibraryKind, LocalId, Override, ResolveError, ScanReport, Status,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::TempDir;

struct Fx {
    _t: TempDir,
    media: PathBuf,
    data: PathBuf,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let media = t.path().join("media");
    fs::create_dir(&media).unwrap();
    Fx {
        data: t.path().join("data"),
        media,
        _t: t,
    }
}

/// Write a file whose content is unique per path (so content hashes differ).
fn put(root: &Path, rel: &str) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, format!("content of {rel}")).unwrap();
    p
}

fn scan(lib: &mut Library, roots: &[&Path]) -> ScanReport {
    let roots: Vec<PathBuf> = roots.iter().map(|p| p.to_path_buf()).collect();
    lib.scan(&roots, &AtomicBool::new(false), |_| {}).unwrap()
}

fn id_of(lib: &Library, title: &str) -> LocalId {
    lib.search(title)
        .into_iter()
        .find(|v| v.title == title)
        .unwrap_or_else(|| panic!("no item titled {title}"))
        .id
}

#[test]
fn indexes_movies_shows_music() {
    let f = fx();
    put(&f.media, "Movies/Heat (1995)/Heat (1995).mkv");
    put(&f.media, "Movies/The.Matrix.1999.1080p.mkv");
    put(&f.media, "Shows/Show/Season 01/Show - S01E01 - Pilot.mkv");
    put(&f.media, "Shows/Show/Season 01/Show - S01E02-E03.mkv");
    put(&f.media, "Shows/Show/Specials/Show - S00E01.mkv");
    put(&f.media, "Music/Artist/Album/01 - Song.flac");
    put(&f.media, "Misc/random clip.mp4");
    put(&f.media, "Movies/readme.txt");
    let mut lib = Library::open(&f.data).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!(rep.added, 7);
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);

    let movies: Vec<_> = lib
        .items(Kind::Movie)
        .into_iter()
        .map(|v| v.title)
        .collect();
    assert_eq!(movies, ["Heat", "The Matrix"]);
    assert_eq!(lib.items(Kind::Track).len(), 1);
    assert_eq!(lib.items(Kind::Unmatched)[0].title, "random clip");

    let series = lib.series();
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].seasons, [0, 1]);
    assert_eq!(series[0].episodes, 3);
    assert_eq!(lib.seasons("show"), [0, 1]);
    let eps = lib.episodes("Show", 1);
    assert_eq!(eps.len(), 2);
    assert_eq!((eps[1].episode, eps[1].episode_end), (Some(2), Some(3)));

    let libs = lib.libraries();
    assert_eq!(libs.len(), 1);
    assert_eq!(libs[0].kind, LibraryKind::Mixed);
    assert_eq!(libs[0].items, 7);

    // Persisted and reloadable.
    let again = Library::open(&f.data).unwrap();
    assert_eq!(again.items(Kind::Movie).len(), 2);
}

#[test]
fn library_kind_by_majority_per_root() {
    let f = fx();
    let (a, b) = (f.media.join("films"), f.media.join("songs"));
    put(&a, "A (2000).mkv");
    put(&a, "B (2001).mkv");
    put(&b, "X/Y/01 One.mp3");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&a, &b]);
    let kinds: Vec<_> = lib
        .libraries()
        .into_iter()
        .map(|l| (l.name, l.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("films".into(), LibraryKind::Movies),
            ("songs".into(), LibraryKind::Music)
        ]
    );
}

#[test]
fn nfo_beats_filename_and_sidecar_art_found() {
    let f = fx();
    let m = put(&f.media, "Heat (1995)/Heat (1995).mkv");
    fs::write(
        m.with_extension("nfo"),
        "<movie><title>Heat (Director's Cut)</title><year>1996</year><plot>Cops.</plot>\
         <genre>Crime</genre><runtime>170</runtime><rating>8.3</rating>\
         <uniqueid type=\"imdb\">tt0113277</uniqueid></movie>",
    )
    .unwrap();
    put(&f.media, "Heat (1995)/poster.jpg");
    put(&f.media, "Heat (1995)/Fanart.PNG");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let v = &lib.items(Kind::Movie)[0];
    assert_eq!(v.title, "Heat (Director's Cut)");
    assert_eq!(v.year, Some(1996));
    assert_eq!((v.runtime_min, v.rating), (Some(170), Some(8.3)));
    assert_eq!(v.genres, ["Crime"]);
    assert_eq!(
        lib.provider_ids(v.id).unwrap().imdb.as_deref(),
        Some("tt0113277")
    );
    let art = lib.art(v.id).unwrap();
    assert_eq!(art.poster.unwrap().file_name().unwrap(), "poster.jpg");
    assert_eq!(art.backdrop.unwrap().file_name().unwrap(), "Fanart.PNG");
}

#[test]
fn series_sidecars_and_episode_nfo() {
    let f = fx();
    let ep = put(&f.media, "Show/Season 01/Show - S01E01.mkv");
    fs::write(
        ep.with_extension("nfo"),
        "<episodedetails><title>Pilot</title></episodedetails>",
    )
    .unwrap();
    fs::write(
        f.media.join("Show/tvshow.nfo"),
        "<tvshow><title>The Show</title><plot>Plot.</plot><year>2008</year></tvshow>",
    )
    .unwrap();
    put(&f.media, "Show/folder.jpg");
    put(&f.media, "Show/season01-poster.jpg");
    put(&f.media, "Show/season-specials-poster.png");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let s = &lib.series()[0];
    assert_eq!((s.name.as_str(), s.show.as_str()), ("The Show", "Show"));
    assert_eq!(s.year, Some(2008));
    assert_eq!(
        s.art.poster.as_ref().unwrap().file_name().unwrap(),
        "folder.jpg"
    );
    assert_eq!(lib.episodes(&s.show, 1)[0].title, "Pilot");
    assert!(lib.season_poster("Show", 1).is_some());
    assert!(lib.season_poster("Show", 0).is_some());
    assert!(lib.season_poster("Show", 2).is_none());
}

#[test]
#[cfg(unix)]
fn symlink_loop_terminates_and_outside_links_are_followed() {
    let f = fx();
    put(&f.media, "a/One (2001).mkv");
    std::os::unix::fs::symlink(&f.media, f.media.join("a/loop")).unwrap();
    let outside = f.media.parent().unwrap().join("outside");
    put(&outside, "Two (2002).mkv");
    std::os::unix::fs::symlink(&outside, f.media.join("link")).unwrap();
    std::os::unix::fs::symlink(f.media.join("nowhere"), f.media.join("broken")).unwrap();
    let mut lib = Library::open(&f.data).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!(lib.items(Kind::Movie).len(), 2);
    assert_eq!(
        rep.warnings.len(),
        1,
        "broken link is a warning: {:?}",
        rep.warnings
    );
}

#[test]
fn hidden_nomedia_ignore_and_extras_are_skipped() {
    let f = fx();
    put(&f.media, "Keep (2000).mkv");
    put(&f.media, ".hidden/A (2001).mkv");
    put(&f.media, ".Dotfile (2002).mkv");
    put(&f.media, "nm/B (2003).mkv");
    put(&f.media, "nm/.nomedia");
    put(&f.media, "ig/sub/C (2004).mkv");
    put(&f.media, "ig/.ignore");
    put(&f.media, "Keep/Extras/D (2005).mkv");
    put(&f.media, "Keep/Trailers/E (2006).mkv");
    put(&f.media, "Keep/Behind The Scenes/F (2007).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let t: Vec<_> = lib
        .items(Kind::Movie)
        .into_iter()
        .map(|v| v.title)
        .collect();
    assert_eq!(t, ["Keep"]);
}

#[test]
#[cfg(unix)]
fn unreadable_dir_is_a_warning_not_fatal() {
    use std::os::unix::fs::PermissionsExt;
    let f = fx();
    put(&f.media, "ok/A (2000).mkv");
    let locked = f.media.join("locked");
    put(&f.media, "locked/B (2001).mkv");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let root_user = fs::read_dir(&locked).is_ok();
    let mut lib = Library::open(&f.data).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if root_user {
        return; // permissions do not apply to root
    }
    assert_eq!(lib.items(Kind::Movie).len(), 1);
    assert!(
        rep.warnings.iter().any(|w| w.contains("locked")),
        "{:?}",
        rep.warnings
    );
}

#[test]
fn rename_and_move_keep_id_and_state() {
    let f = fx();
    let p = put(&f.media, "Old/Heat (1995).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let id = id_of(&lib, "Heat");
    lib.set_progress(id, 600).unwrap();

    // same inode, new name and folder
    fs::create_dir_all(f.media.join("New")).unwrap();
    fs::rename(&p, f.media.join("New/Heat Renamed (1996).mkv")).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!((rep.moved, rep.added, rep.missing), (1, 0, 0));
    let v = lib.view(id).unwrap();
    assert_eq!((v.title.as_str(), v.year), ("Heat Renamed", Some(1996)));
    assert_eq!(lib.state(id).resume_secs, 600);
    assert_eq!(lib.items(Kind::Movie).len(), 1);

    // copy then delete: new inode, same size and content
    let from = f.media.join("New/Heat Renamed (1996).mkv");
    let to = f.media.join("Copied/Heat Again (1997).mkv");
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(&from, &to).unwrap();
    fs::remove_file(&from).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!((rep.moved, rep.added), (1, 0));
    assert_eq!(lib.view(id).unwrap().title, "Heat Again");
    assert_eq!(lib.state(id).resume_secs, 600);
}

#[test]
fn unrelated_new_file_gets_new_id() {
    let f = fx();
    let p = put(&f.media, "A (2000).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let a = id_of(&lib, "A");
    fs::remove_file(p).unwrap();
    put(&f.media, "B (2001).mkv");
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!((rep.added, rep.moved, rep.missing), (1, 0, 1));
    assert_ne!(id_of(&lib, "B"), a);
}

#[test]
fn delete_marks_missing_then_purges() {
    let f = fx();
    let p = put(&f.media, "A (2000).mkv");
    put(&f.media, "B (2001).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let a = id_of(&lib, "A");
    lib.set_progress(a, 5).unwrap();
    fs::remove_file(&p).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!(rep.missing, 1);
    assert_eq!(
        lib.items(Kind::Movie).len(),
        1,
        "missing items leave browse lists"
    );
    assert_eq!(lib.view(a).unwrap().status, Status::Missing);
    assert_eq!(lib.resolve_path(a), Err(ResolveError::Missing));
    assert_eq!(lib.state(a).resume_secs, 5, "state kept while missing");

    // comes back (e.g. restored from trash): same id, available again
    put(&f.media, "A (2000).mkv");
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!(rep.missing, 0);
    assert_eq!(lib.view(a).unwrap().status, Status::Available);

    // explicit purge drops the entry, its override and its state
    fs::remove_file(&p).unwrap();
    scan(&mut lib, &[&f.media]);
    assert_eq!(lib.purge_missing().unwrap(), 1);
    assert!(lib.view(a).is_none());
    assert_eq!(lib.state(a), Default::default());
}

#[test]
fn unmounted_root_is_unavailable_not_missing() {
    let f = fx();
    put(&f.media, "A (2000).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let a = id_of(&lib, "A");
    let backup = f.media.with_file_name("media-away");
    fs::rename(&f.media, &backup).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!(rep.missing, 0);
    assert_eq!(rep.unavailable_roots, std::slice::from_ref(&f.media));
    assert_eq!(lib.view(a).unwrap().status, Status::Unavailable);
    assert_eq!(lib.resolve_path(a), Err(ResolveError::Unavailable));
    assert_eq!(lib.items(Kind::Movie).len(), 1, "still listed, flagged");
    assert!(!lib.libraries()[0].available);

    fs::rename(&backup, &f.media).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!((rep.unchanged, rep.missing), (1, 0));
    assert_eq!(lib.view(a).unwrap().status, Status::Available);
    assert!(lib.resolve_path(a).is_ok());
}

#[test]
fn empty_mountpoint_is_unavailable_and_never_purged() {
    let f = fx();
    put(&f.media, "A (2000).mkv");
    let gone = put(&f.media, "B (2001).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let (a, b) = (id_of(&lib, "A"), id_of(&lib, "B"));
    lib.set_progress(a, 42).unwrap();
    fs::remove_file(&gone).unwrap();
    scan(&mut lib, &[&f.media]); // B is genuinely missing now
    assert_eq!(lib.view(b).unwrap().status, Status::Missing);

    // drive unplugged: the mountpoint dir exists but is empty
    fs::remove_file(f.media.join("A (2000).mkv")).unwrap();
    let rep = scan(&mut lib, &[&f.media]);
    assert_eq!(rep.missing, 0);
    assert_eq!(rep.unavailable_roots, std::slice::from_ref(&f.media));
    assert_eq!(lib.view(a).unwrap().status, Status::Unavailable);
    assert_eq!(lib.resolve_path(a), Err(ResolveError::Unavailable));
    assert!(!lib.libraries()[0].available);
    assert_eq!(
        lib.purge_missing().unwrap(),
        0,
        "unavailable roots are never purged"
    );
    assert_eq!(lib.state(a).resume_secs, 42);

    // plugged back in
    put(&f.media, "A (2000).mkv");
    let rep = scan(&mut lib, &[&f.media]);
    assert!(rep.unavailable_roots.is_empty());
    assert_eq!(lib.view(a).unwrap().status, Status::Available);
    assert_eq!(lib.state(a).resume_secs, 42);
}

#[test]
fn overrides_survive_rescan_and_apply() {
    let f = fx();
    put(&f.media, "random clip.mp4");
    put(&f.media, "B (2001).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let u = lib.items(Kind::Unmatched)[0].id;
    lib.set_override(
        u,
        Override {
            kind: Some(Kind::Episode),
            title: Some("Fixed".into()),
            show: Some("Manual Show".into()),
            season: Some(2),
            episode: Some(5),
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..2 {
        scan(&mut lib, &[&f.media]);
        let eps = lib.episodes("manual show", 2);
        assert_eq!(
            (eps.len(), eps[0].id, eps[0].title.as_str()),
            (1, u, "Fixed")
        );
    }
    // survives a restart too, and a scan never touches overrides.json
    let before = fs::read(f.data.join("overrides.json")).unwrap();
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    assert_eq!(fs::read(f.data.join("overrides.json")).unwrap(), before);
    assert_eq!(lib.items(Kind::Unmatched).len(), 0);
    // clearing the override restores parsed values
    lib.set_override(u, Override::default()).unwrap();
    assert_eq!(lib.items(Kind::Unmatched).len(), 1);
}

#[test]
fn incremental_rescan_skips_unchanged() {
    let f = fx();
    for i in 0..5 {
        put(&f.media, &format!("M{i} (200{i}).mkv"));
    }
    let mut lib = Library::open(&f.data).unwrap();
    let r1 = scan(&mut lib, &[&f.media]);
    assert_eq!((r1.added, r1.unchanged), (5, 0));
    let r2 = scan(&mut lib, &[&f.media]);
    assert_eq!((r2.added, r2.updated, r2.moved, r2.unchanged), (0, 0, 0, 5));

    // editing a file re-parses just that one; a new sidecar is picked up without touching the media file
    let p = f.media.join("M0 (2000).mkv");
    fs::write(&p, "changed content!").unwrap();
    fs::write(
        f.media.join("M1 (2001).nfo"),
        "<movie><title>Renamed By Nfo</title></movie>",
    )
    .unwrap();
    let r3 = scan(&mut lib, &[&f.media]);
    assert_eq!((r3.updated, r3.unchanged), (1, 4));
    assert!(lib.search("renamed by nfo").len() == 1);
}

#[test]
fn search_is_case_and_diacritic_insensitive() {
    let f = fx();
    put(&f.media, "Amélie (2001).mkv");
    put(&f.media, "Léon (1994).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    assert_eq!(lib.search("amelie")[0].title, "Amélie");
    assert_eq!(lib.search("LEON")[0].title, "Léon");
    assert_eq!(lib.search("Élie")[0].title, "Amélie");
    assert!(lib.search("").is_empty());
}

#[test]
fn recent_continue_and_resolve() {
    let f = fx();
    put(&f.media, "A (2000).mkv");
    put(&f.media, "B (2001).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let (a, b) = (id_of(&lib, "A"), id_of(&lib, "B"));
    assert_eq!(lib.recently_added(1).len(), 1);
    assert_eq!(lib.recently_added(10).len(), 2);

    lib.set_progress(a, 100).unwrap();
    lib.set_progress(b, 50).unwrap();
    assert_eq!(lib.continue_watching(10).len(), 2);
    lib.set_played(a, true).unwrap();
    let cw = lib.continue_watching(10);
    assert_eq!(cw.len(), 1);
    assert_eq!(cw[0].id, b);

    let path = lib.resolve_path(a).unwrap();
    assert!(path.is_absolute() && path.exists());
    assert_eq!(lib.resolve_path(LocalId(999)), Err(ResolveError::Unknown));
    // state survives reopen
    assert_eq!(Library::open(&f.data).unwrap().state(b).resume_secs, 50);
}

#[test]
fn cancelled_scan_marks_nothing_missing() {
    let f = fx();
    put(&f.media, "A (2000).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    fs::remove_file(f.media.join("A (2000).mkv")).unwrap();
    let cancel = AtomicBool::new(true);
    let rep = lib
        .scan(std::slice::from_ref(&f.media), &cancel, |_| {})
        .unwrap();
    assert!(rep.cancelled);
    assert_eq!(lib.items(Kind::Movie).len(), 1);
}

#[test]
fn reports_progress() {
    let f = fx();
    put(&f.media, "A (2000).mkv");
    put(&f.media, "B (2001).mkv");
    let mut lib = Library::open(&f.data).unwrap();
    let mut last = None;
    lib.scan(
        std::slice::from_ref(&f.media),
        &AtomicBool::new(false),
        |p| last = Some(p),
    )
    .unwrap();
    let p = last.unwrap();
    assert_eq!((p.done, p.total), (2, 2));
}

#[test]
fn opens_v0_index_from_disk() {
    let f = fx();
    fs::create_dir_all(&f.data).unwrap();
    fs::write(f.data.join("index.json"), r#"{"items": []}"#).unwrap();
    let mut lib = Library::open(&f.data).unwrap();
    put(&f.media, "A (2000).mkv");
    scan(&mut lib, &[&f.media]);
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(f.data.join("index.json")).unwrap()).unwrap();
    assert_eq!(saved["version"], 1);
    assert_eq!(saved["items"].as_array().unwrap().len(), 1);
}

#[test]
fn no_temp_files_left_and_media_tree_untouched() {
    let f = fx();
    put(&f.media, "A (2000).mkv");
    let snapshot = |p: &Path| {
        let mut v: Vec<_> = fs::read_dir(p)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), e.metadata().unwrap().len())
            })
            .collect();
        v.sort();
        v
    };
    let before = snapshot(&f.media);
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    scan(&mut lib, &[&f.media]);
    assert_eq!(snapshot(&f.media), before);
    let names: Vec<_> = snapshot(&f.data).into_iter().map(|(n, _)| n).collect();
    assert!(
        names.iter().all(|n| !n.to_string_lossy().ends_with(".tmp")),
        "{names:?}"
    );
}

#[test]
fn online_layer_sits_between_filename_and_nfo() {
    use nocturne_local::{OnlineMeta, online};
    let f = fx();
    put(&f.media, "Movies/Heat (1995)/Heat (1995).mkv");
    put(&f.media, "Movies/Alien (1979)/Alien (1979).mkv");
    fs::write(
        f.media.join("Movies/Alien (1979)/Alien (1979).nfo"),
        "<movie><title>Alien</title><plot>From the nfo.</plot></movie>",
    )
    .unwrap();
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let heat = id_of(&lib, "Heat");
    let alien = id_of(&lib, "Alien");
    assert_eq!(lib.online_targets(0).len(), 2, "both want a lookup");

    for (id, name) in [(heat, "h.jpg"), (alien, "a.jpg")] {
        let m = OnlineMeta {
            provider: "tmdb".into(),
            fetched: 10,
            title: Some("Online Title".into()),
            plot: Some("From online.".into()),
            genres: vec!["Crime".into()],
            poster: Some(name.into()),
            ..Default::default()
        };
        lib.put_online(
            &online::item_key(id),
            "x",
            Some(m),
            vec![],
            &[(name.into(), vec![1, 2, 3])],
        )
        .unwrap();
    }
    let h = lib.view(heat).unwrap();
    assert_eq!(
        (h.title.as_str(), h.plot.as_deref()),
        ("Online Title", Some("From online."))
    );
    assert_eq!(h.details_from, Some("TMDB"));
    assert!(
        h.art.poster.unwrap().starts_with(&f.data),
        "art lives in the data dir"
    );
    // .nfo beats online per field; the online genres still fill what the nfo left empty.
    let a = lib.view(alien).unwrap();
    assert_eq!(
        (a.title.as_str(), a.plot.as_deref()),
        ("Alien", Some("From the nfo."))
    );
    assert_eq!(a.genres, vec!["Crime".to_string()]);
    // user override beats both
    lib.set_override(
        heat,
        Override {
            plot: Some("Mine.".into()),
            title: Some("Mine".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let h = lib.view(heat).unwrap();
    assert_eq!(
        (h.title.as_str(), h.plot.as_deref()),
        ("Mine", Some("Mine."))
    );
    // looked up once: nothing left to do, and it survives a reopen
    assert!(lib.online_targets(100).is_empty());
    let lib2 = Library::open(&f.data).unwrap();
    assert_eq!(lib2.view(alien).unwrap().details_from, Some("TMDB"));
    // six months on, TMDB data is dropped with its art
    let mut lib = lib2;
    let day = 24 * 3600;
    assert_eq!(lib.online_prune(10 + 181 * day).unwrap(), 2);
    assert_eq!(lib.view(heat).unwrap().details_from, None);
    assert!(!f.data.join("online-art/h.jpg").exists());
    assert!(!f.media.join("online-art").exists());
}

#[test]
fn stale_online_entries_never_attach_to_new_files() {
    use nocturne_local::{OnlineMeta, online};
    let f = fx();
    let a = put(&f.media, "Movies/Heat (1995)/Heat (1995).mkv");
    let b = put(&f.media, "Movies/Alien (1979)/Alien (1979).mkv");
    put(&f.media, "Movies/Keep (2000)/Keep (2000).mkv"); // a root that is not empty counts as mounted
    let mut lib = Library::open(&f.data).unwrap();
    scan(&mut lib, &[&f.media]);
    let old = [id_of(&lib, "Heat"), id_of(&lib, "Alien")];
    for id in old {
        let m = OnlineMeta {
            provider: "tmdb".into(),
            fetched: 10,
            title: Some("Stale".into()),
            poster: Some(format!("p{}.jpg", id.0)),
            ..Default::default()
        };
        let art = [(format!("p{}.jpg", id.0), vec![1])];
        lib.put_online(&online::item_key(id), "x", Some(m), vec![], &art)
            .unwrap();
    }
    fs::remove_file(a).unwrap();
    fs::remove_file(b).unwrap();
    scan(&mut lib, &[&f.media]);
    lib.purge_missing().unwrap();
    assert!(
        !f.data
            .join(format!("online-art/p{}.jpg", old[0].0))
            .exists(),
        "art of purged items is swept"
    );
    // A scan killed before its index save leaves the old counter on disk while online.json has the ids.
    let mut ix: serde_json::Value =
        serde_json::from_slice(&fs::read(f.data.join("index.json")).unwrap()).unwrap();
    ix["next_id"] = 1.into();
    fs::write(f.data.join("index.json"), serde_json::to_vec(&ix).unwrap()).unwrap();
    // The stale entries come back (as if the purge was lost too).
    let mut lib = Library::open(&f.data).unwrap();
    for id in old {
        lib.put_online(&online::item_key(id), "x", None, vec![], &[])
            .unwrap();
    }
    let mut lib = Library::open(&f.data).unwrap();
    put(&f.media, "Movies/Ronin (1998)/Ronin (1998).mkv");
    put(&f.media, "Movies/Fargo (1996)/Fargo (1996).mkv");
    scan(&mut lib, &[&f.media]);
    for t in ["Ronin", "Fargo"] {
        let id = id_of(&lib, t);
        assert!(!old.contains(&id), "ids are never reused");
        assert!(lib.online_entry(&online::item_key(id)).is_none());
        assert_eq!(lib.view(id).unwrap().details_from, None);
    }
    let keep = id_of(&lib, "Keep");
    for t in ["Ronin", "Fargo"] {
        assert_ne!(
            id_of(&lib, t),
            keep,
            "a new file never takes an existing item's id"
        );
    }
    // Keep was never looked up either, so three items wait for a lookup on every platform.
    assert_eq!(
        lib.online_targets(0).len(),
        3,
        "new files get a real lookup"
    );
}
