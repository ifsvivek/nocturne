//! Native requests: the list of movies and shows the user asked for without a Seerr server. It lives
//! in one file under `$XDG_DATA_HOME/nocturne/` (written atomically) and never leaves this computer;
//! only when Radarr or Sonarr is connected and allowed is the title also added there (the glue in
//! the UI crate, after the dialog's confirmation). Pure logic, no I/O besides the file. A request
//! turns Available by itself when the title (or the requested seasons) shows up in any source.
use crate::catalog::data_dir;
use nocturne_arr::Progress;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Asked for, nothing known yet.
    #[default]
    Requested,
    /// Radarr / Sonarr has it in its queue.
    Queued,
    Downloading,
    /// On one of your sources (or the service has the file).
    Available,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Requested => "Requested",
            Self::Queued => "Queued",
            Self::Downloading => "Downloading",
            Self::Available => "Available",
        }
    }
}

/// One request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    /// A show (else a movie).
    pub tv: bool,
    pub tmdb: u64,
    pub tvdb: Option<u64>,
    pub imdb: Option<String>,
    pub title: String,
    pub year: Option<u32>,
    /// A TMDB poster path (`/abc.jpg`) or a full URL.
    pub poster: Option<String>,
    /// The requested seasons; empty = all of them (or a movie).
    pub seasons: Vec<u32>,
    /// Unix seconds.
    pub requested: u64,
    pub status: Status,
    /// `Radarr` / `Sonarr` when the request was also added there.
    pub via: Option<String>,
}

impl Entry {
    pub fn key(&self) -> (bool, u64) {
        (self.tv, self.tmdb)
    }

    /// `Season 2`, `Seasons 1, 3`, `All seasons`; empty for a movie.
    pub fn seasons_text(&self) -> String {
        if !self.tv {
            return String::new();
        }
        match self.seasons.as_slice() {
            [] => "All seasons".into(),
            [one] => format!("Season {one}"),
            many => format!(
                "Seasons {}",
                many.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// What the catalog has of a requested title.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub present: bool,
    /// The season numbers some source has (shows only; empty when it could not be read).
    pub seasons: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wishlist {
    pub version: u32,
    pub entries: Vec<Entry>,
}

impl Wishlist {
    pub fn default_path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("wishlist.json"))
    }

    /// Missing, unreadable or corrupt gives an empty list (nothing was asked yet as far as we know).
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("path has a parent"))?;
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn get(&self, tv: bool, tmdb: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key() == (tv, tmdb))
    }

    /// Add a request. A title already on the list is not added twice: a show's new seasons are
    /// merged into its entry (and the entry reopens when it had been Available). `false` when nothing changed.
    pub fn add(&mut self, mut e: Entry) -> bool {
        e.status = Status::Requested;
        let Some(old) = self.entries.iter_mut().find(|x| x.key() == e.key()) else {
            self.entries.insert(0, e);
            self.version = 1;
            return true;
        };
        if !e.tv || (old.status != Status::Available && covers(&old.seasons, &e.seasons)) {
            return false;
        }
        old.seasons = if old.seasons.is_empty() || e.seasons.is_empty() {
            vec![]
        } else {
            let s: BTreeSet<u32> = old.seasons.iter().chain(&e.seasons).copied().collect();
            s.into_iter().collect()
        };
        old.status = Status::Requested;
        old.requested = e.requested;
        true
    }

    /// Remove a request (cancel). Nothing else is touched: a title already added to Radarr / Sonarr stays there.
    pub fn cancel(&mut self, tv: bool, tmdb: u64) -> bool {
        let n = self.entries.len();
        self.entries.retain(|e| e.key() != (tv, tmdb));
        self.entries.len() != n
    }

    /// Requests that are not Available yet.
    pub fn open(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| e.status != Status::Available)
    }

    /// The catalog was searched for this request: mark it Available when the title (and for a show
    /// the requested seasons) is there. `true` when it newly became Available.
    pub fn apply_found(&mut self, tv: bool, tmdb: u64, f: &Found) -> bool {
        let Some(e) = self.entries.iter_mut().find(|e| e.key() == (tv, tmdb)) else {
            return false;
        };
        if e.status == Status::Available || !is_there(e, f) {
            return false;
        }
        e.status = Status::Available;
        true
    }

    /// Radarr / Sonarr reported on this request. Never moves an Available request back.
    /// `true` when it newly became Available.
    pub fn apply_progress(&mut self, tv: bool, tmdb: u64, p: Progress) -> bool {
        let Some(e) = self.entries.iter_mut().find(|e| e.key() == (tv, tmdb)) else {
            return false;
        };
        let next = match p {
            Progress::Unknown => return false,
            Progress::Wanted => Status::Requested,
            Progress::Queued => Status::Queued,
            Progress::Downloading => Status::Downloading,
            Progress::Available => Status::Available,
        };
        if e.status == Status::Available || e.status == next {
            return false;
        }
        e.status = next;
        next == Status::Available
    }
}

fn covers(have: &[u32], want: &[u32]) -> bool {
    have.is_empty() || (!want.is_empty() && want.iter().all(|s| have.contains(s)))
}

/// A movie is there when found; a show when found and, if seasons were requested, every one of them
/// is on some source. A show asked for as a whole counts once the show itself is there.
fn is_there(e: &Entry, f: &Found) -> bool {
    if !f.present {
        return false;
    }
    !e.tv || e.seasons.is_empty() || e.seasons.iter().all(|s| f.seasons.contains(s))
}

/// The seasons Sonarr is told about: every season of the show, wanted only when the user picked it
/// (none picked = every real season; Specials, season 0, only when picked).
pub fn arr_seasons(
    all: &[nocturne_arr::SeasonInfo],
    chosen: &[u32],
) -> Vec<nocturne_arr::SeasonInfo> {
    all.iter()
        .map(|s| nocturne_arr::SeasonInfo {
            season_number: s.season_number,
            monitored: if chosen.is_empty() {
                s.season_number > 0
            } else {
                chosen.contains(&s.season_number)
            },
        })
        .collect()
}

/// How long to wait before looking at the requests again (the catalog, and Radarr / Sonarr): often
/// while the Requests page is open, every 15 minutes otherwise.
pub fn poll_every(page_open: bool) -> std::time::Duration {
    std::time::Duration::from_secs(if page_open { 20 } else { 15 * 60 })
}

/// `2026-11-27` for unix seconds (UTC).
pub fn date_text(secs: u64) -> String {
    crate::calendar::Day((secs / 86_400) as i64).iso()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(id: u64) -> Entry {
        Entry {
            tmdb: id,
            title: format!("M{id}"),
            requested: 1_700_000_000,
            ..Default::default()
        }
    }

    fn show(id: u64, seasons: &[u32]) -> Entry {
        Entry {
            tv: true,
            tmdb: id,
            seasons: seasons.to_vec(),
            ..movie(id)
        }
    }

    fn found(seasons: &[u32]) -> Found {
        Found {
            present: true,
            seasons: seasons.iter().copied().collect(),
        }
    }

    #[test]
    fn add_cancel_and_no_duplicates() {
        let mut w = Wishlist::default();
        assert!(w.add(movie(1)));
        assert!(!w.add(movie(1)), "a movie is requested once");
        assert!(w.add(movie(2)));
        assert_eq!(w.entries[0].tmdb, 2, "newest first");
        assert!(w.add(show(1, &[1])), "same id, other kind");
        assert!(w.cancel(false, 1) && !w.cancel(false, 1));
        assert_eq!(w.entries.len(), 2);
        assert!(w.get(true, 1).is_some() && w.get(false, 1).is_none());
    }

    #[test]
    fn a_show_merges_new_seasons_and_reopens() {
        let mut w = Wishlist::default();
        w.add(show(5, &[1]));
        assert!(!w.add(show(5, &[1])), "already asked for");
        assert!(w.add(show(5, &[2])));
        assert_eq!(w.entries[0].seasons, [1, 2]);
        assert!(w.add(show(5, &[])), "all seasons widens it");
        assert!(w.entries[0].seasons.is_empty());
        assert!(!w.add(show(5, &[3])), "everything is already requested");
        // after it turned Available, asking again reopens it
        w.entries[0].status = Status::Available;
        assert!(w.add(show(5, &[4])));
        assert_eq!(w.entries[0].status, Status::Requested);
    }

    #[test]
    fn becomes_available_when_the_title_or_the_seasons_appear() {
        let mut w = Wishlist::default();
        w.add(movie(1));
        w.add(show(2, &[2, 3]));
        w.add(show(3, &[]));
        assert!(!w.apply_found(false, 1, &Found::default()));
        assert!(w.apply_found(false, 1, &found(&[])), "a movie is there");
        assert!(!w.apply_found(false, 1, &found(&[])), "only once");
        assert!(!w.apply_found(true, 2, &found(&[1, 2])), "season 3 missing");
        assert!(!w.apply_found(true, 2, &found(&[])), "seasons unreadable");
        assert!(w.apply_found(true, 2, &found(&[1, 2, 3, 4])));
        assert!(
            w.apply_found(true, 3, &found(&[])),
            "a whole show: the show suffices"
        );
        assert_eq!(w.open().count(), 0);
        assert!(!w.apply_found(false, 99, &found(&[])), "unknown entry");
    }

    #[test]
    fn the_service_moves_a_request_forward_never_back() {
        let mut w = Wishlist::default();
        w.add(movie(1));
        assert!(!w.apply_progress(false, 1, Progress::Queued));
        assert_eq!(w.entries[0].status, Status::Queued);
        w.apply_progress(false, 1, Progress::Downloading);
        assert_eq!(w.entries[0].status, Status::Downloading);
        assert!(!w.apply_progress(false, 1, Progress::Unknown));
        assert_eq!(w.entries[0].status, Status::Downloading);
        assert!(w.apply_progress(false, 1, Progress::Available));
        assert!(
            !w.apply_progress(false, 1, Progress::Wanted),
            "Available stays"
        );
        assert_eq!(w.entries[0].status, Status::Available);
    }

    #[test]
    fn sonarr_monitors_only_the_picked_seasons() {
        let all: Vec<_> = (0..=3)
            .map(|n| nocturne_arr::SeasonInfo {
                season_number: n,
                monitored: true,
            })
            .collect();
        let m = |v: Vec<nocturne_arr::SeasonInfo>| -> Vec<bool> {
            v.iter().map(|s| s.monitored).collect()
        };
        assert_eq!(m(arr_seasons(&all, &[2])), [false, false, true, false]);
        assert_eq!(m(arr_seasons(&all, &[])), [false, true, true, true]);
        assert_eq!(m(arr_seasons(&all, &[0, 3])), [true, false, false, true]);
    }

    #[test]
    fn the_file_round_trips_and_survives_damage() {
        let d = std::env::temp_dir().join(format!("nocturne-wish-{}", std::process::id()));
        let p = d.join("nested").join("wishlist.json");
        let mut w = Wishlist::default();
        w.add(movie(7));
        w.add(show(8, &[1, 2]));
        w.entries[0].via = Some("Sonarr".into());
        w.save_to(&p).unwrap();
        assert!(!p.with_extension("json.tmp").exists(), "no temp file left");
        assert_eq!(Wishlist::load_from(&p), w);
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(Wishlist::load_from(&p), Wishlist::default());
        assert_eq!(
            Wishlist::load_from(&d.join("missing.json")),
            Wishlist::default()
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn polling_is_slow_unless_the_page_is_open() {
        assert!(poll_every(true) <= std::time::Duration::from_secs(30));
        assert_eq!(poll_every(false), std::time::Duration::from_secs(900));
    }

    #[test]
    fn texts() {
        assert_eq!(show(1, &[]).seasons_text(), "All seasons");
        assert_eq!(show(1, &[2]).seasons_text(), "Season 2");
        assert_eq!(show(1, &[1, 3]).seasons_text(), "Seasons 1, 3");
        assert_eq!(movie(1).seasons_text(), "");
        assert_eq!(date_text(1_700_000_000), "2023-11-14");
    }
}
