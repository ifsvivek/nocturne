//! Global search, minus the window: debounce + stale-response guard, grouping of mixed results,
//! recent searches. Pure so it is unit-tested offline.
use crate::source::{Kind, Media};
use std::time::Duration;

pub const DEBOUNCE: Duration = Duration::from_millis(250);
pub const RECENT_MAX: usize = 8;

/// Result sections, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Movies,
    Shows,
    Episodes,
    People,
    Collections,
    Playlists,
    Genres,
    Music,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Self::Movies => "Movies",
            Self::Shows => "Shows",
            Self::Episodes => "Episodes",
            Self::People => "People",
            Self::Collections => "Collections",
            Self::Playlists => "Playlists",
            Self::Genres => "Genres",
            Self::Music => "Music",
        }
    }
}

fn group_of(k: Kind) -> Option<Group> {
    Some(match k {
        Kind::Movie => Group::Movies,
        Kind::Series => Group::Shows,
        Kind::Episode => Group::Episodes,
        Kind::Person => Group::People,
        Kind::BoxSet => Group::Collections,
        Kind::Playlist => Group::Playlists,
        Kind::Genre | Kind::Studio => Group::Genres,
        Kind::MusicAlbum | Kind::MusicArtist | Kind::Audio => Group::Music,
        _ => return None,
    })
}

/// Bucket a ranked mixed list by type, keeping server order inside each group.
/// Unknown types are dropped (nothing to open them with).
pub fn group(items: Vec<Media>) -> Vec<(Group, Vec<Media>)> {
    let mut out: Vec<(Group, Vec<Media>)> = vec![];
    for m in items {
        let Some(g) = group_of(m.kind) else { continue };
        match out.iter_mut().find(|(k, _)| *k == g) {
            Some((_, v)) => v.push(m),
            None => out.push((g, vec![m])),
        }
    }
    out.sort_by_key(|(g, _)| *g);
    out
}

/// Typing state. `edited` starts a debounce window; `due` fires only for the newest edit;
/// `arrived` accepts only the newest request's answer.
#[derive(Default)]
pub struct Search {
    pub query: String,
    pub results: Vec<(Group, Vec<Media>)>,
    pub sel: usize,
    pub loading: bool,
    /// The last request failed (not the same as "no results").
    pub failed: bool,
    epoch: u64,
    /// Query the current `results` answer.
    answered: String,
}

impl Search {
    /// The text changed: returns the epoch the debounce timer must hand back to `due`.
    pub fn edited(&mut self) -> u64 {
        self.epoch += 1;
        self.sel = 0;
        if self.query.trim().is_empty() {
            self.results.clear();
            self.answered.clear();
            self.loading = false;
        }
        self.epoch
    }

    /// After the quiet period: the term to ask the server about, if this is still the newest edit.
    pub fn due(&mut self, epoch: u64) -> Option<String> {
        let q = self.query.trim().to_string();
        if epoch != self.epoch || q.is_empty() || q == self.answered {
            return None;
        }
        self.loading = true;
        self.failed = false;
        Some(q)
    }

    /// A response: dropped when the user typed again since it was requested.
    pub fn arrived(&mut self, epoch: u64, q: &str, items: Vec<Media>) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.results = group(items);
        self.answered = q.to_string();
        self.loading = false;
        self.sel = 0;
        true
    }

    /// The request for the newest edit failed: say so instead of showing an empty list.
    pub fn failed(&mut self, epoch: u64) {
        if epoch == self.epoch {
            self.loading = false;
            self.failed = true;
            self.results.clear();
            self.answered.clear();
        }
    }

    /// Results in display order, for keyboard selection.
    pub fn flat(&self) -> Vec<&Media> {
        self.results.iter().flat_map(|(_, v)| v).collect()
    }

    pub fn step(&mut self, d: i32) {
        let n = self.flat().len();
        if n > 0 {
            self.sel = (self.sel as i32 + d).rem_euclid(n as i32) as usize;
        }
    }

    /// Nothing found for a finished, non-empty query.
    pub fn empty(&self) -> bool {
        !self.loading && !self.failed && !self.answered.is_empty() && self.results.is_empty()
    }
}

/// Most recent first, case-insensitive de-dup, bounded. Blank queries are ignored.
pub fn push_recent(list: &mut Vec<String>, q: &str) {
    let q = q.trim();
    if q.is_empty() {
        return;
    }
    list.retain(|r| !r.eq_ignore_ascii_case(q));
    list.insert(0, q.to_string());
    list.truncate(RECENT_MAX);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_jellyfin as jf;

    fn m(kind: &str, name: &str) -> Media {
        Media::from_item(&jf::Item {
            id: name.into(),
            name: name.into(),
            kind: kind.into(),
            ..Default::default()
        })
    }

    #[test]
    fn groups_in_fixed_order_and_keep_server_order() {
        let g = group(vec![
            m("Person", "p1"),
            m("Movie", "m1"),
            m("Episode", "e1"),
            m("Movie", "m2"),
            m("Series", "s1"),
            m("Playlist", "pl"),
            m("BoxSet", "c1"),
            m("Genre", "g1"),
            m("Trailer", "dropped"),
        ]);
        let order: Vec<_> = g.iter().map(|(k, _)| k.label()).collect();
        assert_eq!(
            order,
            [
                "Movies",
                "Shows",
                "Episodes",
                "People",
                "Collections",
                "Playlists",
                "Genres"
            ]
        );
        let movies: Vec<_> = g[0].1.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(movies, ["m1", "m2"]);
        assert_eq!(g.iter().map(|(_, v)| v.len()).sum::<usize>(), 8);
    }

    #[test]
    fn debounce_fires_only_for_newest_edit() {
        let mut s = Search {
            query: "a".into(),
            ..Default::default()
        };
        let e1 = s.edited();
        s.query = "ab".into();
        let e2 = s.edited();
        assert_eq!(s.due(e1), None, "superseded by the second keystroke");
        assert_eq!(s.due(e2).as_deref(), Some("ab"));
        assert!(s.loading);
    }

    #[test]
    fn stale_responses_are_dropped() {
        let mut s = Search {
            query: "ab".into(),
            ..Default::default()
        };
        let e = s.edited();
        assert!(s.due(e).is_some());
        s.query = "abc".into();
        let e2 = s.edited();
        assert!(
            !s.arrived(e, "ab", vec![m("Movie", "old")]),
            "late answer ignored"
        );
        assert!(s.results.is_empty());
        assert!(s.due(e2).is_some());
        assert!(s.arrived(e2, "abc", vec![m("Movie", "new")]));
        assert_eq!(s.flat()[0].title, "new");
        assert!(!s.loading);
        assert_eq!(s.due(e2), None, "same query is not asked twice");
    }

    #[test]
    fn empty_states_and_selection() {
        let mut s = Search::default();
        assert!(!s.empty(), "nothing typed is not 'no results'");
        s.query = "zz".into();
        let e = s.edited();
        s.due(e);
        s.arrived(e, "zz", vec![]);
        assert!(s.empty());
        s.query = "q".into();
        let e = s.edited();
        s.due(e);
        s.arrived(e, "q", vec![m("Movie", "a"), m("Series", "b")]);
        s.step(1);
        assert_eq!(s.sel, 1);
        s.step(1);
        assert_eq!(s.sel, 0, "wraps");
        s.step(-1);
        assert_eq!(s.sel, 1);
        s.query.clear();
        s.edited();
        assert!(s.results.is_empty() && !s.empty());
    }

    #[test]
    fn recent_searches_dedupe_and_bound() {
        let mut r = vec![];
        for q in ["a", "b", "  ", "A", "c"] {
            push_recent(&mut r, q);
        }
        assert_eq!(r, ["c", "A", "b"]);
        for i in 0..20 {
            push_recent(&mut r, &format!("q{i}"));
        }
        assert_eq!(r.len(), RECENT_MAX);
        assert_eq!(r[0], "q19");
    }
}
