//! Smart collections: saved local rules over the unified catalog ("Unwatched 4K movies"). A rule
//! filters the merged entries client-side with the fields every list query already returns; the one
//! exception is resolution, which a server answers (`Is4K`/`IsHD`) and a local folder cannot, so
//! the UI leaves local sources out of such a rule. Stored in `$XDG_DATA_HOME/nocturne/smart.json`,
//! never written to a server. Pure: no UI, no network.
use crate::admin::fmt::parse_iso;
use crate::catalog::data_dir;
use crate::libview::{Res, SortKey};
use crate::merge::Entry;
use crate::source::SourceId;
use crate::unified::{UKind, sort_entries};
use nocturne_local::fold;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Watch {
    #[default]
    Any,
    Unwatched,
    InProgress,
    Watched,
}

impl Watch {
    pub const ALL: [Watch; 4] = [Self::Any, Self::Unwatched, Self::InProgress, Self::Watched];

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Unwatched => "Unwatched",
            Self::InProgress => "In progress",
            Self::Watched => "Watched",
        }
    }
}

/// All conditions must hold; an empty / `None` condition does not narrow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub kind: UKind,
    pub watch: Watch,
    /// Any of these (case and accents ignored).
    pub genres: Vec<String>,
    pub year_from: Option<u32>,
    pub year_to: Option<u32>,
    pub min_rating: Option<f32>,
    /// Added to its source within this many days.
    pub added_days: Option<u32>,
    pub res: Option<Res>,
    pub min_runtime: Option<u32>,
    pub max_runtime: Option<u32>,
    /// Empty = every source.
    pub sources: Vec<SourceId>,
    pub sort: SortKey,
    pub desc: bool,
    pub limit: Option<u32>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            kind: UKind::Movies,
            watch: Watch::Any,
            genres: vec![],
            year_from: None,
            year_to: None,
            min_rating: None,
            added_days: None,
            res: None,
            min_runtime: None,
            max_runtime: None,
            sources: vec![],
            sort: SortKey::Name,
            desc: false,
            limit: None,
        }
    }
}

impl Rule {
    fn matches(&self, e: &Entry, now: i64) -> bool {
        let m = &e.primary;
        let played = e.all().any(|c| c.played);
        let started = e.all().any(|c| c.resume_s.is_some_and(|s| s > 0.0));
        let watch = match self.watch {
            Watch::Any => true,
            Watch::Unwatched => !played,
            Watch::InProgress => !played && started,
            Watch::Watched => played,
        };
        let genre = self.genres.is_empty()
            || m.genres
                .iter()
                .any(|g| self.genres.iter().any(|w| fold(w) == fold(g)));
        let year = |bound: Option<u32>, ok: fn(u32, u32) -> bool| {
            bound.is_none_or(|b| m.year.is_some_and(|y| ok(y, b)))
        };
        let runtime = |bound: Option<u32>, ok: fn(u32, u32) -> bool| {
            bound.is_none_or(|b| m.runtime_min.is_some_and(|r| ok(r, b)))
        };
        let added = self.added_days.is_none_or(|d| {
            m.added
                .as_deref()
                .and_then(parse_iso)
                .is_some_and(|t| t >= now - i64::from(d) * 86_400)
        });
        watch
            && genre
            && year(self.year_from, |y, b| y >= b)
            && year(self.year_to, |y, b| y <= b)
            && self
                .min_rating
                .is_none_or(|r| m.rating.is_some_and(|x| x >= r))
            && added
            && runtime(self.min_runtime, |r, b| r >= b)
            && runtime(self.max_runtime, |r, b| r <= b)
            && (self.sources.is_empty() || e.all().any(|c| self.sources.contains(&c.origin)))
    }

    /// The entries this rule selects, in its order, cut to its limit. `now` is unix seconds.
    pub fn apply(&self, entries: Vec<Entry>, now: i64) -> Vec<Entry> {
        let mut v: Vec<Entry> = entries
            .into_iter()
            .filter(|e| self.matches(e, now))
            .collect();
        sort_entries(&mut v, self.sort, self.desc);
        if let Some(n) = self.limit {
            v.truncate(n as usize);
        }
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Smart {
    pub id: u64,
    pub name: String,
    pub rule: Rule,
}

/// The saved list, in the user's order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    version: u32,
    /// The starter set was added once; after that deleting one keeps it gone.
    seeded: bool,
    pub items: Vec<Smart>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: VERSION,
            seeded: false,
            items: vec![],
        }
    }
}

fn starter(name: &str, rule: Rule) -> (String, Rule) {
    (name.into(), rule)
}

/// Starter set: useful without any editing.
pub fn builtins() -> Vec<(String, Rule)> {
    let new = Rule {
        sort: SortKey::DateAdded,
        desc: true,
        ..Rule::default()
    };
    vec![
        starter(
            "Unwatched 4K movies",
            Rule {
                watch: Watch::Unwatched,
                res: Some(Res::Uhd4k),
                ..new.clone()
            },
        ),
        starter(
            "Added this week",
            Rule {
                added_days: Some(7),
                ..new.clone()
            },
        ),
        starter(
            "Unwatched shows",
            Rule {
                kind: UKind::Shows,
                watch: Watch::Unwatched,
                ..new.clone()
            },
        ),
        starter(
            "Continue watching",
            Rule {
                watch: Watch::InProgress,
                ..new
            },
        ),
        starter(
            "Top rated, not watched",
            Rule {
                watch: Watch::Unwatched,
                min_rating: Some(8.0),
                sort: SortKey::Rating,
                desc: true,
                limit: Some(50),
                ..Rule::default()
            },
        ),
    ]
}

impl Store {
    fn seeded(mut self) -> Self {
        if !self.seeded {
            for (name, rule) in builtins() {
                self.add(name, rule);
            }
            self.seeded = true;
        }
        self
    }

    pub fn default_path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("smart.json"))
    }

    /// Missing or corrupt gives the starter set. Older files (no version, or never seeded) are
    /// brought up to date; a newer file's unknown fields are ignored.
    pub fn load_from(path: &Path) -> Self {
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.version = s.version.max(VERSION);
        s.seeded()
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("path has a parent"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&tmp, path)
    }

    pub fn get(&self, id: u64) -> Option<&Smart> {
        self.items.iter().find(|s| s.id == id)
    }

    pub fn add(&mut self, name: String, rule: Rule) -> u64 {
        let id = self.items.iter().map(|s| s.id).max().map_or(1, |m| m + 1);
        self.items.push(Smart { id, name, rule });
        id
    }

    pub fn update(&mut self, id: u64, name: String, rule: Rule) {
        if let Some(s) = self.items.iter_mut().find(|s| s.id == id) {
            (s.name, s.rule) = (name, rule);
        }
    }

    pub fn delete(&mut self, id: u64) {
        self.items.retain(|s| s.id != id);
    }

    /// Move one step toward the front (`up`) or the back.
    pub fn shift(&mut self, id: u64, up: bool) {
        let Some(i) = self.items.iter().position(|s| s.id == id) else {
            return;
        };
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        if let Some(j) = j.filter(|j| *j < self.items.len()) {
            self.items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::Entry;
    use crate::source::{Kind, Media};

    const A: SourceId = SourceId(1);
    const B: SourceId = SourceId(2);
    const NOW: i64 = 1_700_000_000;

    fn e(id: &str, f: impl FnOnce(&mut Media)) -> Entry {
        let mut m = Media {
            origin: A,
            id: id.into(),
            title: id.into(),
            kind: Kind::Movie,
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        };
        f(&mut m);
        Entry {
            primary: m,
            alternates: vec![],
        }
    }

    fn ids(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.primary.id.as_str()).collect()
    }

    fn sample() -> Vec<Entry> {
        vec![
            e("a", |m| {
                m.played = true;
                m.year = Some(1994);
                m.genres = vec!["Drama".into()];
                m.rating = Some(8.5);
                m.runtime_min = Some(140);
            }),
            e("b", |m| {
                m.resume_s = Some(120.0);
                m.year = Some(2005);
                m.genres = vec!["Comedy".into(), "Drama".into()];
                m.rating = Some(6.0);
                m.runtime_min = Some(90);
                m.added = Some("2023-11-14T22:13:20Z".into()); // = NOW
            }),
            e("c", |m| {
                m.origin = B;
                m.year = Some(1991);
                m.rating = Some(9.0);
                m.added = Some("2023-01-01T00:00:00Z".into());
            }),
        ]
    }

    fn run(r: Rule) -> Vec<String> {
        r.apply(sample(), NOW)
            .iter()
            .map(|e| e.primary.id.clone())
            .collect()
    }

    #[test]
    fn watch_states() {
        let w = |watch| {
            run(Rule {
                watch,
                ..Rule::default()
            })
        };
        assert_eq!(w(Watch::Any), ["a", "b", "c"]);
        assert_eq!(w(Watch::Watched), ["a"]);
        assert_eq!(w(Watch::Unwatched), ["b", "c"]);
        assert_eq!(w(Watch::InProgress), ["b"]);
    }

    #[test]
    fn a_title_watched_in_any_source_counts_as_watched() {
        let mut v = vec![e("x", |_| {})];
        let mut other = v[0].primary.clone();
        (other.origin, other.played) = (B, true);
        v[0].alternates.push(other);
        let r = Rule {
            watch: Watch::Watched,
            ..Rule::default()
        };
        assert_eq!(ids(&r.apply(v, NOW)), ["x"]);
    }

    #[test]
    fn genre_year_rating_runtime_and_added() {
        let g = Rule {
            genres: vec!["drama".into()],
            ..Rule::default()
        };
        assert_eq!(run(g), ["a", "b"]);
        let nineties = Rule {
            year_from: Some(1990),
            year_to: Some(1999),
            ..Rule::default()
        };
        assert_eq!(run(nineties), ["a", "c"]);
        let hi = Rule {
            min_rating: Some(8.0),
            ..Rule::default()
        };
        assert_eq!(run(hi), ["a", "c"]);
        let short = Rule {
            max_runtime: Some(100),
            ..Rule::default()
        };
        assert_eq!(run(short), ["b"], "unknown runtime does not pass a bound");
        let long = Rule {
            min_runtime: Some(120),
            ..Rule::default()
        };
        assert_eq!(run(long), ["a"]);
        let recent = Rule {
            added_days: Some(7),
            ..Rule::default()
        };
        assert_eq!(run(recent), ["b"], "no date added does not pass");
    }

    #[test]
    fn sources_and_combinations() {
        let only_b = Rule {
            sources: vec![B],
            ..Rule::default()
        };
        assert_eq!(run(only_b), ["c"]);
        let all = Rule {
            watch: Watch::Unwatched,
            genres: vec!["Drama".into()],
            year_from: Some(2000),
            min_rating: Some(5.0),
            ..Rule::default()
        };
        assert_eq!(run(all), ["b"]);
    }

    #[test]
    fn sort_and_limit() {
        let r = Rule {
            sort: SortKey::Rating,
            desc: true,
            limit: Some(2),
            ..Rule::default()
        };
        assert_eq!(run(r), ["c", "a"]);
    }

    #[test]
    fn starter_set_is_seeded_once_and_deletions_persist() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("smart.json");
        let mut s = Store::load_from(&p);
        assert_eq!(s.items.len(), builtins().len());
        let id = s.items[0].id;
        s.delete(id);
        s.save_to(&p).unwrap();
        let again = Store::load_from(&p);
        assert_eq!(again.items.len(), builtins().len() - 1);
        assert!(again.get(id).is_none());
    }

    #[test]
    fn crud_reorder_and_migration() {
        let mut s = Store::load_from(Path::new("/nonexistent/smart.json"));
        let id = s.add("Mine".into(), Rule::default());
        s.update(
            id,
            "Renamed".into(),
            Rule {
                watch: Watch::Watched,
                ..Rule::default()
            },
        );
        assert_eq!(s.get(id).unwrap().name, "Renamed");
        s.shift(id, true);
        assert_eq!(s.items[s.items.len() - 2].id, id);
        s.shift(id, false);
        assert_eq!(s.items.last().unwrap().id, id);
        // An old file with no version and a partial rule loads, fills defaults and gets the starters.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("smart.json");
        std::fs::write(
            &p,
            r#"{"items":[{"id":7,"name":"Old","rule":{"kind":"shows","future_field":1}}]}"#,
        )
        .unwrap();
        let s = Store::load_from(&p);
        assert_eq!(s.items[0].rule.kind, UKind::Shows);
        assert_eq!(s.items.len(), 1 + builtins().len());
        assert!(s.items.iter().skip(1).all(|x| x.id > 7));
        // Corrupt gives the starters.
        std::fs::write(&p, "{").unwrap();
        assert_eq!(Store::load_from(&p).items.len(), builtins().len());
    }
}
