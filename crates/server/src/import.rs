//! Import (docs/dev/ARR_PLAN.md section 5): which files of a finished download belong to which units, where they go under the naming templates, and the placing itself. Placing hardlinks when the download and the library share a file system and copies otherwise, so the original keeps seeding. It writes only below the library root, never over a file NOCTURNE did not import, and removes a replaced file only after its successor is in place.
use crate::naming;
use nocturne_arr_types::{MediaKind, ParsedRelease};
use nocturne_core::server::{Library, Title, Unit};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

const VIDEO: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "ts", "m2ts", "webm", "wmv", "mov", "mpg",
];
const AUDIO: &[&str] = &[
    "flac", "mp3", "m4a", "ogg", "opus", "wav", "aac", "alac", "ape", "wv",
];
const BOOK: &[&str] = &[
    "epub", "mobi", "azw3", "azw", "pdf", "cbz", "cbr", "m4b", "djvu",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Move {
    pub unit: u64,
    pub src: PathBuf,
    pub dest: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub moves: Vec<Move>,
    /// Grabbed units no file was found for.
    pub missing: Vec<u64>,
}

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn is_sample(p: &Path) -> bool {
    stem(p).to_lowercase().contains("sample")
        || p.components()
            .any(|c| c.as_os_str().eq_ignore_ascii_case("sample"))
}

/// Plan the import of `files` (absolute paths and sizes) for the grabbed `units` of `title`. `parse` is the release-name parser (for `S01E02` in file names).
pub fn plan(
    title: &Title,
    lib: &Library,
    units: &[Unit],
    rel: &ParsedRelease,
    files: &[(PathBuf, u64)],
    parse: &dyn Fn(&str) -> ParsedRelease,
) -> Plan {
    let of = |exts: &[&str]| -> Vec<&(PathBuf, u64)> {
        let mut v: Vec<_> = files
            .iter()
            .filter(|(p, _)| exts.contains(&ext(p).as_str()) && !is_sample(p))
            .collect();
        v.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
        v
    };
    let mut pairs: Vec<(&Unit, &PathBuf)> = Vec::new();
    match title.kind {
        // ponytail: one file per movie / book (the largest); multi-part audiobooks import their largest part only. Import every part when a profile asks for it.
        MediaKind::Movie | MediaKind::Book => {
            let exts = if title.kind == MediaKind::Movie {
                VIDEO
            } else {
                BOOK
            };
            if let (Some(u), Some((p, _))) = (units.first(), of(exts).first()) {
                pairs.push((u, p));
            }
        }
        MediaKind::Series => {
            let vids = of(VIDEO);
            for u in units {
                let hit = vids.iter().find(|(p, _)| {
                    let pr = parse(&stem(p));
                    pr.season == u.season && u.episode.is_some_and(|e| pr.episodes.contains(&e))
                });
                // A single-episode download whose file name says nothing: its one video file.
                let hit = hit.or_else(|| (units.len() == 1 && vids.len() == 1).then(|| &vids[0]));
                if let Some((p, _)) = hit {
                    pairs.push((u, p));
                }
            }
        }
        MediaKind::Music => {
            if let Some(u) = units.first() {
                pairs.extend(of(AUDIO).into_iter().map(|(p, _)| (u, p)));
            }
        }
    }
    let mut out = Plan::default();
    let mut used = HashSet::new();
    for (u, src) in &pairs {
        let mut dest = lib.root.join(target(title, lib, u, rel, src));
        if !used.insert(dest.clone()) {
            // Two discs with the same track names: keep the disc folder's name in front.
            let disc = src
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("2");
            let name = dest
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            dest.set_file_name(format!("{} - {name}", naming::sanitize(disc)));
            used.insert(dest.clone());
        }
        out.moves.push(Move {
            unit: u.id,
            src: (*src).clone(),
            dest,
        });
    }
    out.missing = units
        .iter()
        .filter(|u| !out.moves.iter().any(|m| m.unit == u.id))
        .map(|u| u.id)
        .collect();
    out
}

/// The path of one file below the library root.
fn target(title: &Title, lib: &Library, u: &Unit, rel: &ParsedRelease, src: &Path) -> PathBuf {
    let year = u.year.or(title.year).map(|y| y.to_string());
    let mut full = rel.quality.label().to_string();
    if rel.proper {
        full.push_str(" Proper");
    } else if rel.repack {
        full.push_str(" Repack");
    }
    let value = |k: &str| -> Option<String> {
        Some(match k {
            "Movie Title" | "Series Title" | "Artist Name" | "Author Name" => title.name.clone(),
            "Release Year" => year.clone()?,
            "Series Year" => title.year?.to_string(),
            "Episode Title" | "Album Title" | "Book Title" => u.name.clone(),
            "season" => u.season?.to_string(),
            "episode" => u.episode?.to_string(),
            "Quality Full" => full.clone(),
            "Quality Title" => rel.quality.label().to_string(),
            "Release Group" => rel.group.clone()?,
            "Original Name" => stem(src),
            "ImdbId" => title.ids.imdb.clone()?,
            _ => return None,
        })
    };
    let mut p = naming::render(&lib.folder_template, &value);
    p.push(naming::render(&lib.file_template, &value));
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    p.set_file_name(format!("{name}.{}", ext(src)));
    p
}

/// `dest` is strictly below `root` through plain names only.
fn inside(root: &Path, dest: &Path) -> bool {
    dest.strip_prefix(root).is_ok_and(|rel| {
        rel.components().count() > 0 && rel.components().all(|c| matches!(c, Component::Normal(_)))
    })
}

/// Hardlink `src` to `dest`, or copy when that fails (another file system). Never replaces `dest` unless it is in `managed` (a file NOCTURNE imported); goes through a temporary name in the target folder and a rename, so a reader never sees half a file.
pub fn place(root: &Path, m: &Move, managed: &HashSet<PathBuf>) -> Result<(), String> {
    if !inside(root, &m.dest) {
        return Err(format!(
            "{} is outside the library folder",
            m.dest.display()
        ));
    }
    if m.dest.symlink_metadata().is_ok() && !managed.contains(&m.dest) {
        return Err(format!(
            "{} already exists and was not imported by NOCTURNE; it was left alone",
            m.dest.display()
        ));
    }
    let dir = m.dest.parent().ok_or("no parent folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = m
        .dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = dir.join(format!(".{name}.nocturne-part"));
    let _ = std::fs::remove_file(&tmp); // our own leftover from an interrupted import
    if std::fs::hard_link(&m.src, &tmp).is_err() {
        std::fs::copy(&m.src, &tmp).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("copy {}: {e}", m.src.display())
        })?;
    }
    std::fs::rename(&tmp, &m.dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", m.dest.display())
    })
}

/// After an upgrade: delete the unit's previous files (all imported by NOCTURNE, from its `FileRecord`) that the new import did not overwrite. Errors are ignored (a file the user already removed is fine).
pub fn retire(root: &Path, old: &[PathBuf], new: &[PathBuf]) {
    for p in old {
        if !new.contains(p) && inside(root, p) {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_arr_types::{Ids, Quality};
    use nocturne_core::server::LibraryMode;

    fn title(kind: MediaKind, name: &str) -> Title {
        Title {
            id: 1,
            kind,
            name: name.into(),
            year: Some(2020),
            ids: Ids::default(),
            profile: 1,
            library: 1,
            monitored: true,
            added: 0,
        }
    }

    fn lib(kind: MediaKind, root: &Path) -> Library {
        let (f, n) = Library::default_templates(kind);
        Library {
            id: 1,
            name: "L".into(),
            kind,
            mode: LibraryMode::Existing,
            root: root.into(),
            folder_template: f.into(),
            file_template: n.into(),
        }
    }

    fn unit(id: u64, name: &str, s: Option<u32>, e: Option<u32>) -> Unit {
        Unit {
            id,
            title: 1,
            name: name.into(),
            season: s,
            episode: e,
            year: None,
            air_date: None,
            ids: Ids::default(),
            monitored: true,
            file: None,
        }
    }

    /// Enough of a parser for these tests: `SxxEyy` anywhere.
    fn parse(n: &str) -> ParsedRelease {
        let l = n.to_lowercase();
        let mut p = ParsedRelease::default();
        if let Some(i) = l.find("s0")
            && let (Ok(s), Some(e)) = (l[i + 1..i + 3].parse(), l[i + 3..].strip_prefix('e'))
        {
            p.season = Some(s);
            p.episodes = vec![e[..2].parse().unwrap()];
        }
        p
    }

    fn q(qu: Quality) -> ParsedRelease {
        ParsedRelease {
            quality: qu,
            ..Default::default()
        }
    }

    #[test]
    fn movie_takes_the_largest_video_and_skips_samples() {
        let root = Path::new("/lib");
        let files = vec![
            ("/dl/M/m.sample.mkv".into(), 900),
            ("/dl/M/Sample/x.mkv".into(), 800),
            ("/dl/M/movie.mkv".into(), 500),
            ("/dl/M/movie.nfo".into(), 9000),
        ];
        let t = title(MediaKind::Movie, "Night: Train");
        let p = plan(
            &t,
            &lib(MediaKind::Movie, root),
            &[unit(5, "Night: Train", None, None)],
            &q(Quality::Bluray1080),
            &files,
            &parse,
        );
        assert_eq!(
            p.moves,
            [Move {
                unit: 5,
                src: "/dl/M/movie.mkv".into(),
                dest: "/lib/Night - Train (2020)/Night - Train (2020) Bluray-1080p.mkv".into(),
            }]
        );
        assert!(p.missing.is_empty());
    }

    #[test]
    fn season_pack_matches_episodes_by_number_and_reports_missing() {
        let root = Path::new("/lib");
        let files = vec![
            ("/dl/P/Show.S01E02.mkv".into(), 5),
            ("/dl/P/Show.S01E01.mkv".into(), 5),
        ];
        let units = [
            unit(1, "Pilot", Some(1), Some(1)),
            unit(2, "Two", Some(1), Some(2)),
            unit(3, "Three", Some(1), Some(3)),
        ];
        let mut rel = q(Quality::WebDl1080);
        rel.proper = true;
        let p = plan(
            &title(MediaKind::Series, "Show"),
            &lib(MediaKind::Series, root),
            &units,
            &rel,
            &files,
            &parse,
        );
        assert_eq!(p.moves.len(), 2);
        assert_eq!(p.moves[0].src, PathBuf::from("/dl/P/Show.S01E01.mkv"));
        assert_eq!(
            p.moves[0].dest,
            PathBuf::from(
                "/lib/Show (2020)/Season 01/Show - S01E01 - Pilot WEBDL-1080p Proper.mkv"
            )
        );
        assert_eq!(p.missing, [3]);
        // One episode, one unnamed file: that file.
        let one = [("/dl/x/video.mp4".into(), 5)];
        let p = plan(
            &title(MediaKind::Series, "Show"),
            &lib(MediaKind::Series, root),
            &units[1..2],
            &rel,
            &one,
            &parse,
        );
        assert_eq!(p.moves.len(), 1);
    }

    #[test]
    fn album_keeps_track_names_and_separates_discs() {
        let root = Path::new("/m");
        let files = vec![
            ("/dl/A/CD1/01 Intro.flac".into(), 5),
            ("/dl/A/CD2/01 Intro.flac".into(), 4),
            ("/dl/A/cover.jpg".into(), 4),
        ];
        let mut u = unit(9, "Bookends", None, None);
        u.year = Some(1968);
        let p = plan(
            &title(MediaKind::Music, "Simon & Garfunkel"),
            &lib(MediaKind::Music, root),
            &[u],
            &q(Quality::Flac),
            &files,
            &parse,
        );
        let d: Vec<_> = p.moves.iter().map(|m| m.dest.clone()).collect();
        assert_eq!(
            d,
            [
                PathBuf::from("/m/Simon & Garfunkel/Bookends (1968)/01 Intro.flac"),
                PathBuf::from("/m/Simon & Garfunkel/Bookends (1968)/CD2 - 01 Intro.flac"),
            ]
        );
    }

    #[test]
    fn book_picks_the_ebook() {
        let files = vec![
            ("/dl/B/book.epub".into(), 5),
            ("/dl/B/cover.jpg".into(), 50),
        ];
        let p = plan(
            &title(MediaKind::Book, "Ursula K. Le Guin"),
            &lib(MediaKind::Book, Path::new("/b")),
            &[unit(4, "The Dispossessed", None, None)],
            &q(Quality::Epub),
            &files,
            &parse,
        );
        assert_eq!(
            p.moves[0].dest,
            PathBuf::from("/b/Ursula K. Le Guin/The Dispossessed (2020)/The Dispossessed.epub")
        );
    }

    #[test]
    fn placing_links_never_overwrites_foreign_files_and_upgrades_after_the_new_file() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("lib");
        let src = d.path().join("dl/new.mkv");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"new").unwrap();
        let dest = root.join("M (2020)/M.mkv");
        let m = Move {
            unit: 1,
            src: src.clone(),
            dest: dest.clone(),
        };
        place(&root, &m, &HashSet::new()).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(src.exists(), "the download keeps seeding");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(&src).unwrap().ino(),
                std::fs::metadata(&dest).unwrap().ino(),
                "same file system: a hardlink"
            );
        }
        // Now a foreign file sits there: refused, untouched.
        std::fs::remove_file(&dest).unwrap();
        std::fs::write(&dest, b"user's").unwrap();
        assert!(
            place(&root, &m, &HashSet::new())
                .unwrap_err()
                .contains("left alone")
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"user's");
        // The same path as a managed file: replaced.
        let managed: HashSet<_> = [dest.clone()].into();
        place(&root, &m, &managed).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        // Upgrade to another name: the old managed file goes only after the new one is in.
        let old = root.join("M (2020)/M.avi");
        std::fs::write(&old, b"old").unwrap();
        retire(
            &root,
            &[old.clone(), d.path().join("outside.mkv")],
            std::slice::from_ref(&dest),
        );
        assert!(!old.exists() && dest.exists());
        // Escapes are refused before anything is written.
        let bad = Move {
            unit: 1,
            src,
            dest: root.join("../x.mkv"),
        };
        assert!(
            place(&root, &bad, &HashSet::new())
                .unwrap_err()
                .contains("outside")
        );
        let leftovers = std::fs::read_dir(root.join("M (2020)")).unwrap().count();
        assert_eq!(leftovers, 1, "no temp file left");
    }
}
