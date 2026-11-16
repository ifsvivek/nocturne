//! What the local folders screen needs from a local source: folder availability, counts, the
//! unmatched list with manual edits, and a scan that reports progress. GPUI/Iced-free; every call
//! blocks (run it on a worker thread). Edits go to `overrides.json` under the source's data dir;
//! nothing here ever writes to, renames or deletes a media file.
use crate::local::LocalBackend;
use nocturne_local::{self as nl, Kind, LocalId, Override, Progress, ScanReport};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub movies: usize,
    pub shows: usize,
    pub episodes: usize,
    pub music: usize,
    pub unmatched: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub path: PathBuf,
    /// Present and readable right now (an unplugged drive is not).
    pub available: bool,
    pub items: usize,
}

/// A file the name parser could not place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmatched {
    pub id: u64,
    pub file: String,
    /// Where it sits, relative to its folder (shown to help recognise it).
    pub dir: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overview {
    pub folders: Vec<Folder>,
    pub counts: Counts,
    pub unmatched: Vec<Unmatched>,
}

/// Unmatched rows shown at once (the rest wait their turn as these are sorted out).
pub const UNMATCHED_SHOWN: usize = 50;

fn readable(p: &Path) -> bool {
    std::fs::read_dir(p).is_ok()
}

impl LocalBackend {
    /// Folder state, counts and the first unmatched files, for `roots` (the profile's folders).
    pub fn overview(&self, roots: &[PathBuf]) -> Overview {
        self.with(|lib| {
            let infos = lib.libraries();
            let folders = roots
                .iter()
                .map(|r| Folder {
                    path: r.clone(),
                    available: readable(r),
                    items: infos.iter().find(|i| &i.root == r).map_or(0, |i| i.items),
                })
                .collect();
            let count = |k| lib.items(k).len();
            let unmatched = lib.items(Kind::Unmatched);
            Overview {
                folders,
                counts: Counts {
                    movies: count(Kind::Movie),
                    shows: lib.series().len(),
                    episodes: count(Kind::Episode),
                    music: count(Kind::Track),
                    unmatched: unmatched.len(),
                },
                unmatched: unmatched
                    .into_iter()
                    .take(UNMATCHED_SHOWN)
                    .map(|v| {
                        let root = roots.iter().find(|r| v.path.starts_with(r));
                        let rel = root.and_then(|r| v.path.strip_prefix(r).ok());
                        Unmatched {
                            id: v.id.0,
                            file: v
                                .path
                                .file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into()),
                            dir: rel
                                .and_then(|p| p.parent())
                                .map_or_else(String::new, |p| p.display().to_string()),
                        }
                    })
                    .collect(),
            }
        })
    }

    /// Scan `roots` into the index. Holds the library for writing until it ends, so reads of this
    /// source wait; progress goes to `on`.
    // ponytail: one write lock for the whole scan; scan a copy and swap it in if big libraries make reads wait too long.
    pub fn scan(
        &self,
        roots: &[PathBuf],
        cancel: &AtomicBool,
        on: impl FnMut(Progress),
    ) -> Result<ScanReport, nl::Error> {
        self.with_mut(|lib| lib.scan(roots, cancel, on))
    }

    /// Apply a manual match. Merged into any earlier edit of the same file.
    pub fn set_match(&self, id: u64, m: &MatchEdit) -> Result<(), nl::Error> {
        self.with_mut(|lib| {
            let mut o = lib.override_of(LocalId(id)).cloned().unwrap_or_default();
            m.apply(&mut o);
            lib.set_override(LocalId(id), o)
        })
    }
}

/// The edit form of an unmatched file, after validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchEdit {
    pub kind: Kind,
    /// A movie's or track's title; a show's name for an episode.
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl MatchEdit {
    /// Check the form's text fields. Movies and music need a title (music: and nothing else is
    /// asked); episodes need a show name, a season and an episode number (season 0 is specials).
    pub fn parse(
        kind: Kind,
        title: &str,
        year: &str,
        season: &str,
        episode: &str,
    ) -> Result<Self, &'static str> {
        let title = title.trim();
        if title.is_empty() || matches!(kind, Kind::Unmatched) {
            return Err("Give it a title.");
        }
        let year = match year.trim() {
            "" => None,
            y => Some(
                y.parse::<u16>()
                    .ok()
                    .filter(|y| (1850..=2200).contains(y))
                    .ok_or("The year looks wrong.")?,
            ),
        };
        let (mut s, mut e) = (None, None);
        if kind == Kind::Episode {
            s = Some(
                season
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "Season is a number.")?,
            );
            e = Some(
                episode
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|e| *e > 0)
                    .ok_or("Episode is a number, from 1.")?,
            );
        }
        Ok(Self {
            kind,
            title: title.to_string(),
            year,
            season: s,
            episode: e,
        })
    }

    fn apply(&self, o: &mut Override) {
        o.kind = Some(self.kind);
        o.year = self.year;
        if self.kind == Kind::Episode {
            o.show = Some(self.title.clone());
            (o.season, o.episode) = (self.season, self.episode);
        } else {
            o.title = Some(self.title.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceId;

    fn backend(files: &[&str]) -> (tempfile::TempDir, LocalBackend, Vec<PathBuf>) {
        let t = tempfile::tempdir().unwrap();
        let media = t.path().join("media");
        for f in files {
            let p = media.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, format!("content of {f}")).unwrap();
        }
        let roots = vec![media.clone()];
        let b = LocalBackend::open(SourceId(1), &t.path().join("data")).unwrap();
        (t, b, roots)
    }

    #[test]
    fn counts_unmatched_and_folder_state_after_a_scan() {
        let (t, b, mut roots) = backend(&[
            "Heat (1995).mkv",
            "Show/Season 01/Show - S01E01 - Pilot.mkv",
            "Show/Season 01/Show - S01E02 - Two.mkv",
            "holiday clip.mkv",
        ]);
        roots.push(t.path().join("unplugged"));
        let mut seen = vec![];
        b.scan(&roots, &AtomicBool::new(false), |p| seen.push(p))
            .unwrap();
        assert!(!seen.is_empty(), "progress is reported");
        let o = b.overview(&roots);
        assert_eq!(
            o.counts,
            Counts {
                movies: 1,
                shows: 1,
                episodes: 2,
                music: 0,
                unmatched: 1
            }
        );
        assert!(o.folders[0].available && o.folders[0].items == 4);
        assert!(!o.folders[1].available, "a missing folder is flagged");
        assert_eq!(o.unmatched[0].file, "holiday clip.mkv");
    }

    #[test]
    fn a_manual_match_moves_the_file_out_of_unmatched_and_touches_no_file() {
        let (t, b, roots) = backend(&["holiday clip.mkv", "weird name.mkv"]);
        b.scan(&roots, &AtomicBool::new(false), |_| {}).unwrap();
        let o = b.overview(&roots);
        assert_eq!(o.counts.unmatched, 2);
        let before = std::fs::read(t.path().join("media/holiday clip.mkv")).unwrap();
        let movie = MatchEdit::parse(Kind::Movie, " Home Video ", "2019", "", "").unwrap();
        b.set_match(o.unmatched[0].id, &movie).unwrap();
        let ep = MatchEdit::parse(Kind::Episode, "Some Show", "", "2", "5").unwrap();
        b.set_match(o.unmatched[1].id, &ep).unwrap();
        let o = b.overview(&roots);
        assert_eq!(o.counts.unmatched, 0);
        assert_eq!((o.counts.movies, o.counts.episodes), (1, 1));
        assert_eq!(
            std::fs::read(t.path().join("media/holiday clip.mkv")).unwrap(),
            before
        );
        assert!(t.path().join("media/weird name.mkv").exists());
    }

    #[test]
    fn the_form_is_validated() {
        let p = |k, t, y, s, e| MatchEdit::parse(k, t, y, s, e);
        assert!(p(Kind::Movie, "  ", "", "", "").is_err());
        assert!(p(Kind::Movie, "X", "19x", "", "").is_err());
        assert!(p(Kind::Movie, "X", "1066", "", "").is_err());
        assert_eq!(p(Kind::Movie, "X", "", "9", "9").unwrap().season, None);
        assert!(p(Kind::Episode, "X", "", "", "1").is_err());
        assert!(p(Kind::Episode, "X", "", "1", "0").is_err());
        let e = p(Kind::Episode, "X", "2001", "0", "3").unwrap();
        assert_eq!(
            (e.season, e.episode, e.year),
            (Some(0), Some(3), Some(2001))
        );
        assert!(p(Kind::Unmatched, "X", "", "", "").is_err());
    }
}
