//! Duplicate detection across sources, on strong identity only. A movie or series matches on any
//! shared provider id; an episode on its series' provider id plus season and episode number. A title
//! and year with no ids never merges (see [`possible_duplicate`], a hint for the UI). Pure: no I/O.
use crate::catalog::data_dir;
use crate::source::{Kind, Media, ProviderIds, SourceId};
use nocturne_local::fold;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// One title as the user sees it: the preferred copy and the others as alternate sources.
#[derive(Clone, Debug)]
pub struct Entry {
    pub primary: Media,
    pub alternates: Vec<Media>,
}

impl Entry {
    /// Every copy, primary first.
    pub fn all(&self) -> impl Iterator<Item = &Media> {
        std::iter::once(&self.primary).chain(&self.alternates)
    }
}

/// `(origin, series id)` to the series' provider ids; episodes carry only their series' id.
#[derive(Clone, Debug, Default)]
pub struct SeriesIds {
    map: HashMap<(SourceId, String), ProviderIds>,
}

impl SeriesIds {
    /// Remember the ids of every series item in `media` that has some.
    pub fn learn(&mut self, media: &[Media]) {
        for m in media
            .iter()
            .filter(|m| m.kind == Kind::Series && !m.ids.is_empty())
        {
            self.set(m.origin, m.id.clone(), m.ids.clone());
        }
    }

    pub fn set(&mut self, origin: SourceId, id: String, ids: ProviderIds) {
        self.map.insert((origin, id), ids);
    }

    pub fn get(&self, origin: SourceId, id: &str) -> Option<&ProviderIds> {
        self.map.get(&(origin, id.to_string()))
    }

    /// Series ids behind `media`'s episodes that are not known yet, per source (for a batched read).
    pub fn missing(&self, media: &[Media]) -> Vec<(SourceId, Vec<String>)> {
        let mut by: BTreeMap<SourceId, Vec<String>> = BTreeMap::new();
        for m in media.iter().filter(|m| m.kind == Kind::Episode) {
            if let Some(sid) = &m.series_id
                && self.get(m.origin, sid).is_none()
            {
                let v = by.entry(m.origin).or_default();
                if !v.contains(sid) {
                    v.push(sid.clone());
                }
            }
        }
        by.into_iter().collect()
    }
}

fn id_keys(prefix: &str, ids: &ProviderIds) -> Vec<String> {
    [
        ("imdb", &ids.imdb),
        ("tmdb", &ids.tmdb),
        ("tvdb", &ids.tvdb),
    ]
    .into_iter()
    .filter_map(|(n, v)| {
        let v = v.as_deref()?.trim().to_lowercase();
        (!v.is_empty()).then(|| format!("{prefix}:{n}:{v}"))
    })
    .collect()
}

/// The strong identities of an item (two items sharing any one are the same title). Empty when it has
/// none (no ids, an episode whose series ids are not known, or a kind that is never merged).
pub fn identity_keys(m: &Media, series: &SeriesIds) -> Vec<String> {
    match m.kind {
        Kind::Movie => id_keys("movie", &m.ids),
        Kind::Series => id_keys("series", &m.ids),
        Kind::Episode => {
            let (Some(sid), Some(s), Some(e)) = (&m.series_id, m.season_number, m.number) else {
                return vec![];
            };
            series
                .get(m.origin, sid)
                .map(|ids| {
                    id_keys("series", ids)
                        .into_iter()
                        .map(|k| format!("ep:{k}:{s}:{e}"))
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => vec![],
    }
}

/// Group `results` (one list per source) into entries. `priority` lists sources best first (use
/// `Catalog::priority`, which already puts unavailable sources last); the first copy by priority is
/// primary unless `prefs` names another. Entries appear in order of first appearance across the
/// priority-ordered inputs. Two items of one source are never merged with each other.
pub fn merge(
    mut results: Vec<(SourceId, Vec<Media>)>,
    priority: &[SourceId],
    prefs: &Preferred,
    series: &SeriesIds,
) -> Vec<Entry> {
    let rank = |o: &SourceId| {
        priority
            .iter()
            .position(|p| p == o)
            .unwrap_or(priority.len())
    };
    results.sort_by_key(|(o, _)| rank(o)); // stable: ties keep the caller's order
    let mut groups: Vec<Vec<Media>> = vec![];
    let mut index: HashMap<String, usize> = HashMap::new();
    for (_, items) in results {
        for m in items {
            let keys = identity_keys(&m, series);
            let into = keys
                .iter()
                .filter_map(|k| index.get(k).copied())
                .find(|g| groups[*g].iter().all(|x| x.origin != m.origin));
            let g = into.unwrap_or(groups.len());
            for k in keys {
                index.entry(k).or_insert(g);
            }
            if g == groups.len() {
                groups.push(vec![m]);
            } else {
                groups[g].push(m);
            }
        }
    }
    groups
        .into_iter()
        .map(|mut g| {
            let pick = g
                .iter()
                .flat_map(|m| identity_keys(m, series))
                .find_map(|k| prefs.map.get(&k).copied())
                .and_then(|o| g.iter().position(|m| m.origin == o))
                // No explicit choice: the best-ranked copy that can actually be played.
                .unwrap_or_else(|| g.iter().position(|m| m.available).unwrap_or(0));
            let primary = g.remove(pick);
            Entry {
                primary,
                alternates: g,
            }
        })
        .collect()
}

/// A title + year match with different sources and no shared id: the same title probably, never
/// merged automatically. The UI may offer to link them.
pub fn possible_duplicate(a: &Media, b: &Media) -> bool {
    let plain = |m: &Media| {
        fold(&m.title)
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let same_kind = a.kind == b.kind && matches!(a.kind, Kind::Movie | Kind::Series);
    let no_shared_id = {
        let none = SeriesIds::default();
        let (ka, kb) = (identity_keys(a, &none), identity_keys(b, &none));
        !ka.iter().any(|k| kb.contains(k))
    };
    same_kind
        && a.origin != b.origin
        && a.year.is_some()
        && a.year == b.year
        && !plain(a).is_empty()
        && plain(a) == plain(b)
        && no_shared_id
}

/// The user's per-title choice of which source to show first, remembered locally
/// (`$XDG_DATA_HOME/nocturne/preferred.json`). Keyed by the title's provider-id identities, so it
/// survives a source being offline or the entry changing shape.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferred {
    version: u32,
    map: BTreeMap<String, SourceId>,
}

impl Preferred {
    pub fn default_path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("preferred.json"))
    }

    /// Missing, unreadable or corrupt gives an empty choice list (it is only a preference).
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

    /// Make `m`'s source the preferred one for this title. Does nothing for items with no identity.
    pub fn choose(&mut self, m: &Media, series: &SeriesIds) {
        self.version = 1;
        for k in identity_keys(m, series) {
            self.map.insert(k, m.origin);
        }
    }

    /// Back to "first by priority" for this title.
    pub fn clear(&mut self, m: &Media, series: &SeriesIds) {
        for k in identity_keys(m, series) {
            self.map.remove(&k);
        }
    }

    pub fn get(&self, m: &Media, series: &SeriesIds) -> Option<SourceId> {
        identity_keys(m, series)
            .iter()
            .find_map(|k| self.map.get(k).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_jellyfin as jf;

    const A: SourceId = SourceId(1);
    const B: SourceId = SourceId(2);
    const C: SourceId = SourceId(3);

    fn ids(imdb: Option<&str>, tmdb: Option<&str>) -> ProviderIds {
        ProviderIds {
            imdb: imdb.map(Into::into),
            tmdb: tmdb.map(Into::into),
            tvdb: None,
        }
    }

    fn item(o: SourceId, id: &str, kind: &str, title: &str, year: Option<u32>) -> Media {
        Media::from_item_in(
            o,
            &jf::Item {
                id: id.into(),
                name: title.into(),
                kind: kind.into(),
                production_year: year,
                ..Default::default()
            },
        )
    }

    fn movie(o: SourceId, id: &str, title: &str, p: ProviderIds) -> Media {
        Media {
            ids: p,
            ..item(o, id, "Movie", title, Some(1999))
        }
    }

    fn episode(o: SourceId, id: &str, series: &str, s: u32, e: u32) -> Media {
        Media {
            series_id: Some(series.into()),
            season_number: Some(s),
            number: Some(e),
            ..item(o, id, "Episode", "Ep", None)
        }
    }

    fn series(o: SourceId, id: &str, p: ProviderIds) -> Media {
        Media {
            ids: p,
            ..item(o, id, "Series", "Show", Some(2001))
        }
    }

    fn run(res: Vec<(SourceId, Vec<Media>)>, prio: &[SourceId]) -> Vec<Entry> {
        merge(res, prio, &Preferred::default(), &SeriesIds::default())
    }

    #[test]
    fn a_shared_imdb_id_merges_across_sources_and_priority_picks_primary() {
        let a = movie(A, "a1", "Heat", ids(Some("tt0113277"), None));
        let b = movie(B, "b1", "Heat (1995)", ids(Some("TT0113277"), Some("949")));
        let e = run(vec![(B, vec![b]), (A, vec![a])], &[A, B]);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].primary.origin, e[0].alternates.len()), (A, 1));
        assert_eq!(e[0].alternates[0].origin, B);
    }

    #[test]
    fn any_one_shared_id_is_enough_but_unrelated_ids_stay_apart() {
        let a = movie(A, "a1", "X", ids(Some("tt1"), Some("5")));
        let b = movie(B, "b1", "X", ids(None, Some("5")));
        let c = movie(C, "c1", "Y", ids(Some("tt2"), Some("6")));
        let e = run(vec![(A, vec![a]), (B, vec![b]), (C, vec![c])], &[A, B, C]);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].alternates.len(), 1);
        assert_eq!(e[1].primary.origin, C);
    }

    #[test]
    fn same_title_and_year_without_ids_never_merges_but_is_hinted() {
        let a = movie(A, "a1", "Heat", ProviderIds::default());
        let b = movie(B, "b1", "heat", ProviderIds::default());
        assert!(possible_duplicate(&a, &b));
        let e = run(vec![(A, vec![a.clone()]), (B, vec![b.clone()])], &[A, B]);
        assert_eq!(e.len(), 2);
        assert!(e.iter().all(|x| x.alternates.is_empty()));
        // different year, same source, or a shared id: no hint.
        let mut c = b.clone();
        c.year = Some(2000);
        assert!(!possible_duplicate(&a, &c));
        assert!(!possible_duplicate(
            &a,
            &Media {
                origin: A,
                ..b.clone()
            }
        ));
        let (x, y) = (
            movie(A, "x", "Heat", ids(Some("tt1"), None)),
            movie(B, "y", "Heat", ids(Some("tt1"), None)),
        );
        assert!(
            !possible_duplicate(&x, &y),
            "that is a real duplicate, not a possible one"
        );
    }

    #[test]
    fn an_unavailable_copy_is_demoted_but_an_explicit_choice_still_wins() {
        let p = ids(Some("tt5"), None);
        let gone = Media {
            available: false,
            ..movie(A, "a", "X", p.clone())
        };
        let ok = movie(B, "b", "X", p);
        let e = run(vec![(A, vec![gone.clone()]), (B, vec![ok.clone()])], &[A, B]);
        assert_eq!((e[0].primary.origin, e[0].alternates[0].origin), (B, A));
        // Nothing playable anywhere: the priority order decides.
        let e = run(vec![(A, vec![gone.clone()]), (B, vec![gone.clone()])], &[A, B]);
        assert_eq!(e[0].primary.origin, A);
        let mut prefs = Preferred::default();
        prefs.choose(&gone, &SeriesIds::default());
        let e = merge(
            vec![(A, vec![gone]), (B, vec![ok])],
            &[A, B],
            &prefs,
            &SeriesIds::default(),
        );
        assert_eq!(e[0].primary.origin, A, "the user's pick is kept");
    }

    #[test]
    fn two_items_of_one_source_are_not_merged_with_each_other() {
        let p = ids(Some("tt9"), None);
        let a1 = movie(A, "a1", "X", p.clone());
        let a2 = movie(A, "a2", "X", p.clone());
        let b = movie(B, "b1", "X", p);
        let e = run(vec![(A, vec![a1, a2]), (B, vec![b])], &[A, B]);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].alternates[0].origin, B, "B joins the first A item");
        assert!(e[1].alternates.is_empty());
    }

    #[test]
    fn episodes_merge_through_their_series_ids_only() {
        let p = ids(Some("tt77"), None);
        let (ea, eb) = (episode(A, "ea", "sa", 1, 2), episode(B, "eb", "sb", 1, 2));
        let other = episode(B, "ec", "sb", 1, 3);
        let all = |sid: &SeriesIds| {
            merge(
                vec![(A, vec![ea.clone()]), (B, vec![eb.clone(), other.clone()])],
                &[A, B],
                &Preferred::default(),
                sid,
            )
        };
        assert_eq!(
            all(&SeriesIds::default()).len(),
            3,
            "no series ids: not merged"
        );
        let mut sid = SeriesIds::default();
        sid.set(A, "sa".into(), p.clone());
        sid.set(B, "sb".into(), ids(None, Some("1")));
        assert_eq!(all(&sid).len(), 3, "series share no id");
        sid.set(B, "sb".into(), p);
        let e = all(&sid);
        assert_eq!(e.len(), 2, "S1E2 merges, S1E3 stays");
        assert_eq!((e[0].primary.id.as_str(), e[0].alternates.len()), ("ea", 1));
        assert_eq!(e[1].primary.id, "ec");
    }

    #[test]
    fn series_ids_are_learned_from_series_items_and_missing_ones_batched() {
        let mut sid = SeriesIds::default();
        sid.learn(&[
            series(A, "sa", ids(Some("tt1"), None)),
            series(A, "noids", ProviderIds::default()),
        ]);
        assert!(sid.get(A, "sa").is_some() && sid.get(A, "noids").is_none());
        let eps = [
            episode(A, "1", "sa", 1, 1),
            episode(A, "2", "x", 1, 2),
            episode(A, "3", "x", 1, 3),
            episode(B, "4", "x", 1, 1),
        ];
        assert_eq!(
            sid.missing(&eps),
            [(A, vec!["x".to_string()]), (B, vec!["x".to_string()])]
        );
    }

    #[test]
    fn unavailable_sources_go_last_by_priority_and_never_lead() {
        let p = ids(Some("tt5"), None);
        let a = movie(A, "a1", "X", p.clone()); // A is offline: priority puts it last
        let b = movie(B, "b1", "X", p);
        let e = run(vec![(A, vec![a]), (B, vec![b])], &[B, A]);
        assert_eq!((e[0].primary.origin, e[0].alternates[0].origin), (B, A));
        // and the order of the whole list follows the demoted priority too
        let only_a = movie(A, "a2", "Solo", ids(Some("tt6"), None));
        let only_b = movie(B, "b2", "Other", ids(Some("tt7"), None));
        let e = run(vec![(A, vec![only_a]), (B, vec![only_b])], &[B, A]);
        assert_eq!(e[0].primary.title, "Other");
    }

    #[test]
    fn preferred_choice_wins_and_survives_a_round_trip() {
        let p = ids(Some("tt8"), None);
        let (a, b) = (
            movie(A, "a1", "X", p.clone()),
            movie(
                B,
                "b1",
                "X",
                ProviderIds {
                    tmdb: Some("8".into()),
                    ..p
                },
            ),
        );
        let sid = SeriesIds::default();
        let mut prefs = Preferred::default();
        prefs.choose(&b, &sid);
        let e = merge(
            vec![(A, vec![a.clone()]), (B, vec![b.clone()])],
            &[A, B],
            &prefs,
            &sid,
        );
        assert_eq!((e[0].primary.origin, e[0].alternates[0].origin), (B, A));
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("x/preferred.json");
        prefs.save_to(&path).unwrap();
        let back = Preferred::load_from(&path);
        assert_eq!(back, prefs);
        assert_eq!(
            back.get(&a, &sid),
            Some(B),
            "found through the shared imdb id"
        );
        let mut cleared = back;
        cleared.clear(&b, &sid);
        let e = merge(vec![(A, vec![a]), (B, vec![b])], &[A, B], &cleared, &sid);
        assert_eq!(e[0].primary.origin, A);
        assert_eq!(
            Preferred::load_from(&d.path().join("none.json")),
            Preferred::default()
        );
    }

    #[test]
    fn items_without_identity_and_other_kinds_pass_through_in_order() {
        let lib = |o, id: &str| item(o, id, "CollectionFolder", "Lib", None);
        let e = run(
            vec![
                (B, vec![lib(B, "l2")]),
                (A, vec![lib(A, "l1"), lib(A, "l3")]),
            ],
            &[A, B],
        );
        let order: Vec<_> = e.iter().map(|x| x.primary.id.as_str()).collect();
        assert_eq!(order, ["l1", "l3", "l2"]);
    }
}
