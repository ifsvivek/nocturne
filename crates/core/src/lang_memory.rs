//! Audio and subtitle language remembered per show (or movie) across restarts, in a local file
//! (`$XDG_DATA_HOME/nocturne/languages.json`, versioned, atomic write). Never on a server.
//!
//! Keys are source-neutral: a show is its series provider ids when known (so the same show on two
//! servers or in the local library shares one choice), plus `origin + series id` which always
//! works. Writes go to every key of the show; reads take the newest entry among them.
use crate::catalog::data_dir;
use crate::merge::id_keys;
use crate::player_logic::TrackChoice;
use crate::source::{Media, ProviderIds};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const VERSION: u32 = 1;

/// Store keys for `m`, best first. `series` = the provider ids of its series (episodes only; read
/// from the series item, since an episode carries just the series' id).
pub fn keys(m: &Media, series: Option<&ProviderIds>) -> Vec<String> {
    let o = m.origin.0;
    match &m.series_id {
        Some(sid) => series
            .map(|ids| id_keys("show", ids))
            .unwrap_or_default()
            .into_iter()
            .chain([format!("show:{o}:{sid}")])
            .collect(),
        None => id_keys("movie", &m.ids)
            .into_iter()
            .chain([format!("item:{o}:{}", m.id)])
            .collect(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LangStore {
    version: u32,
    /// Counts updates; the newest entry among a show's keys wins (a write made before the series
    /// ids were known only reaches the origin key).
    seq: u64,
    map: BTreeMap<String, Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Entry {
    seq: u64,
    #[serde(flatten)]
    choice: TrackChoice,
}

impl LangStore {
    pub fn default_path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("languages.json"))
    }

    /// Missing, unreadable or corrupt gives an empty store (it only holds preferences).
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

    pub fn get(&self, keys: &[String]) -> TrackChoice {
        keys.iter()
            .filter_map(|k| self.map.get(k))
            .max_by_key(|e| e.seq)
            .map(|e| e.choice.clone())
            .unwrap_or_default()
    }

    /// Applies `f` to the newest choice among `keys` (else a fresh one), and stores the result under
    /// every key. No keys, nothing stored.
    pub fn update(&mut self, keys: &[String], f: impl FnOnce(&mut TrackChoice)) {
        if keys.is_empty() {
            return;
        }
        let mut c = self.get(keys);
        f(&mut c);
        self.version = VERSION;
        self.seq += 1;
        for k in keys {
            let entry = Entry {
                seq: self.seq,
                choice: c.clone(),
            };
            self.map.insert(k.clone(), entry);
        }
    }
}

/// The process-wide store, read from disk on first use.
static STORE: Mutex<Option<LangStore>> = Mutex::new(None);

fn with_store<R>(f: impl FnOnce(&mut LangStore, Option<&Path>) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let path = LangStore::default_path();
    let s = g.get_or_insert_with(|| {
        path.as_deref()
            .map(LangStore::load_from)
            .unwrap_or_default()
    });
    f(s, path.as_deref())
}

pub fn recall(keys: &[String]) -> TrackChoice {
    with_store(|s, _| s.get(keys))
}

/// Updates and saves (a failed save only loses the preference for the next run).
pub fn remember(keys: &[String], f: impl FnOnce(&mut TrackChoice)) {
    with_store(|s, path| {
        s.update(keys, f);
        if let Some(p) = path {
            let _ = s.save_to(p);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_logic::{AudioPick, SubPick};
    use crate::source::{Kind, SourceId};
    use nocturne_jellyfin as jf;

    fn ep(origin: u32, series: &str) -> Media {
        Media {
            series_id: Some(series.into()),
            ..Media::from_item_in(
                SourceId(origin),
                &jf::Item {
                    id: format!("e{origin}"),
                    kind: "Episode".into(),
                    ..Default::default()
                },
            )
        }
    }

    fn tmdb(v: &str) -> ProviderIds {
        ProviderIds {
            tmdb: Some(v.into()),
            ..Default::default()
        }
    }

    #[test]
    fn show_keys_are_source_neutral_when_ids_are_known() {
        let (a, b) = (ep(1, "s-a"), ep(2, "s-b"));
        let ids = tmdb("42");
        let (ka, kb) = (keys(&a, Some(&ids)), keys(&b, Some(&ids)));
        assert_eq!(ka[0], kb[0], "same show on two sources");
        assert_ne!(ka.last(), kb.last());
        // Without ids only the origin key exists, and it is per series, not per episode.
        assert_eq!(keys(&a, None), vec!["show:1:s-a".to_string()]);
        assert_eq!(keys(&ep(1, "s-a"), None), keys(&a, None));
        let mut movie = ep(1, "x");
        movie.series_id = None;
        movie.kind = Kind::Movie;
        assert_eq!(keys(&movie, None), vec!["item:1:e1".to_string()]);
    }

    #[test]
    fn update_writes_all_keys_and_reads_the_newest() {
        let mut s = LangStore::default();
        let (a, b) = (ep(1, "s-a"), ep(2, "s-b"));
        let ids = tmdb("42");
        s.update(&keys(&a, Some(&ids)), |c| {
            c.audio = Some(AudioPick::Lang("jpn".into()))
        });
        // The other source finds it through the shared provider key.
        assert_eq!(
            s.get(&keys(&b, Some(&ids))).audio,
            Some(AudioPick::Lang("jpn".into()))
        );
        // Without ids it is only found on its own origin key.
        assert_eq!(s.get(&keys(&b, None)), TrackChoice::default());
        assert!(s.get(&keys(&a, None)).audio.is_some());
        // A second change keeps the first one's other fields.
        s.update(&keys(&a, None), |c| c.sub = Some(SubPick::Off));
        let c = s.get(&keys(&a, Some(&ids)));
        assert!(c.audio.is_some() && c.sub == Some(SubPick::Off));
        s.update(&[], |c| c.sub = Some(SubPick::Off)); // no keys: no-op
    }

    #[test]
    fn survives_a_restart_atomically_and_tolerates_junk() {
        let dir = std::env::temp_dir().join(format!("nocturne-lang-{}", std::process::id()));
        let p = dir.join("languages.json");
        let mut s = LangStore::default();
        let k = keys(&ep(1, "s"), None);
        s.update(&k, |c| c.sub2 = Some(SubPick::Lang("eng".into())));
        s.save_to(&p).unwrap();
        assert!(
            !p.with_extension("json.tmp").exists(),
            "temp file renamed away"
        );
        assert_eq!(LangStore::load_from(&p), s);
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(LangStore::load_from(&p), LangStore::default());
        // A newer file with an unknown field still loads.
        std::fs::write(&p, r#"{"version":9,"future":1,"map":{}}"#).unwrap();
        assert_eq!(LangStore::load_from(&p).map.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
