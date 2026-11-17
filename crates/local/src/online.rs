//! The online provider layer: what a metadata provider (TMDB, TVmaze, MusicBrainz) told us about an item, a
//! series or an album, kept in `online.json` and `online-art/` inside the library's own data directory (never next
//! to media). Fetching lives in `nocturne-core`; this module only stores, ages and layers the results.
//!
//! Precedence (see `lib.rs`): user override > `.nfo` > sidecar art > online provider > file name.

use crate::{Kind, Library, ProviderIds, fold, now_secs, store};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// TMDB's terms forbid keeping its data longer than six months: refresh at ~5, drop at 6.
pub const TMDB_REFRESH_SECS: u64 = 150 * 24 * 3600;
pub const TMDB_MAX_AGE_SECS: u64 = 180 * 24 * 3600;
/// A lookup that found nothing is tried again after this long.
pub const RETRY_SECS: u64 = 30 * 24 * 3600;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OnlineMeta {
    /// `tmdb`, `tvmaze` or `musicbrainz`.
    pub provider: String,
    pub fetched: u64,
    pub title: Option<String>,
    pub year: Option<u16>,
    pub plot: Option<String>,
    pub genres: Vec<String>,
    pub runtime_min: Option<u32>,
    pub rating: Option<f32>,
    pub ids: ProviderIds,
    /// The provider's own id of the show / album (TVmaze id, MusicBrainz release-group id); what episode lookups use.
    #[serde(default)]
    pub native_id: Option<String>,
    /// File names inside `online-art/`.
    pub poster: Option<String>,
    pub backdrop: Option<String>,
}

/// One possible match offered to the user when the search was not clear enough.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub provider: String,
    pub id: String,
    pub title: String,
    pub year: Option<u16>,
    pub overview: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OnlineEntry {
    /// When a lookup last ran (even if it found nothing).
    pub tried: u64,
    pub meta: Option<OnlineMeta>,
    /// Ambiguous search results waiting for the user.
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// The candidate the user picked; the next run fetches exactly this one.
    #[serde(default)]
    pub chosen: Option<Candidate>,
    /// Display name for the "Choose the right match" list.
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct OnlineStore {
    version: u32,
    entries: BTreeMap<String, OnlineEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Movie,
    Show,
    Episode,
    Album,
}

/// Something to look up, with only what may be sent (title, year, ids, numbers; never a path).
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub key: String,
    pub kind: TargetKind,
    pub title: String,
    pub year: Option<u16>,
    pub ids: ProviderIds,
    /// Episodes: the show's own key (its entry carries the provider id), season and episode.
    pub show_key: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// Albums: the artist.
    pub artist: Option<String>,
    pub chosen: Option<Candidate>,
}

pub fn item_key(id: crate::LocalId) -> String {
    format!("i{}", id.0)
}
pub fn show_key(show: &str) -> String {
    format!("s{}", fold(show))
}
pub fn album_key(artist: &str, album: &str) -> String {
    format!("a{}|{}", fold(artist), fold(album))
}

/// Does this entry want (another) lookup at `now`?
pub fn stale(e: Option<&OnlineEntry>, now: u64) -> bool {
    let Some(e) = e else { return true };
    if e.chosen.is_some() && e.meta.is_none() {
        return true;
    }
    match &e.meta {
        Some(m) if m.provider == "tmdb" => now.saturating_sub(m.fetched) > TMDB_REFRESH_SECS,
        Some(_) => false,
        // Waiting for the user, or nothing found: leave it be for a while.
        None => e.candidates.is_empty() && now.saturating_sub(e.tried) > RETRY_SECS,
    }
}

const ART_DIR: &str = "online-art";

impl Library {
    pub(crate) fn load_online(dir: &std::path::Path) -> Result<OnlineStore, crate::Error> {
        Ok(store::load(&dir.join("online.json"))?.unwrap_or_default())
    }

    fn save_online(&mut self) -> Result<(), crate::Error> {
        self.online.version = 1;
        store::save(&self.dir.join("online.json"), &self.online)
    }

    pub fn online_entry(&self, key: &str) -> Option<&OnlineEntry> {
        self.online.entries.get(key)
    }

    pub(crate) fn online_meta(&self, key: &str) -> Option<&OnlineMeta> {
        self.online.entries.get(key)?.meta.as_ref()
    }

    pub(crate) fn online_art_path(&self, name: &str) -> PathBuf {
        self.dir.join(ART_DIR).join(name)
    }

    /// Everything that wants a lookup now: shows first (their id is needed by the episodes), then movies,
    /// episodes and albums.
    pub fn online_targets(&self, now: u64) -> Vec<Target> {
        let mut out = vec![];
        let mut seen = std::collections::HashSet::new();
        let chosen = |k: &str| self.online.entries.get(k).and_then(|e| e.chosen.clone());
        let mut push = |out: &mut Vec<Target>, t: Target| {
            if stale(self.online.entries.get(&t.key), now) && seen.insert(t.key.clone()) {
                out.push(t);
            }
        };
        let base = |key: String, kind, title: String| Target {
            key,
            kind,
            title,
            year: None,
            ids: ProviderIds::default(),
            show_key: None,
            season: None,
            episode: None,
            artist: None,
            chosen: None,
        };
        for s in self.series() {
            let mut t = base(show_key(&s.show), TargetKind::Show, s.name.clone());
            (t.year, t.ids, t.chosen) = (s.year, s.ids.clone(), chosen(&t.key));
            push(&mut out, t);
        }
        for v in self.views() {
            let t = match v.kind {
                Kind::Movie => {
                    let mut t = base(item_key(v.id), TargetKind::Movie, v.title.clone());
                    (t.year, t.ids, t.chosen) = (v.year, v.ids.clone(), chosen(&t.key));
                    t
                }
                Kind::Episode => {
                    let (Some(show), Some(s), Some(e)) = (&v.show, v.season, v.episode) else {
                        continue;
                    };
                    let mut t = base(item_key(v.id), TargetKind::Episode, v.title.clone());
                    (t.show_key, t.season, t.episode) = (Some(show_key(show)), Some(s), Some(e));
                    t
                }
                Kind::Track => {
                    let (Some(artist), Some(album)) = (&v.artist, &v.album) else {
                        continue;
                    };
                    let mut t = base(album_key(artist, album), TargetKind::Album, album.clone());
                    (t.year, t.artist, t.chosen) = (v.year, Some(artist.clone()), chosen(&t.key));
                    t
                }
                Kind::Unmatched => continue,
            };
            push(&mut out, t);
        }
        out.sort_by_key(|t| (t.kind != TargetKind::Show, t.key.clone()));
        out
    }

    /// Store the outcome of one lookup. `art` are `(file name, bytes)` already downloaded for `meta`.
    pub fn put_online(
        &mut self,
        key: &str,
        label: &str,
        meta: Option<OnlineMeta>,
        candidates: Vec<Candidate>,
        art: &[(String, Vec<u8>)],
    ) -> Result<(), crate::Error> {
        for (name, bytes) in art {
            if name.contains('/') || name.starts_with('.') {
                continue;
            }
            store::write_atomic(&self.dir.join(ART_DIR).join(name), bytes)?;
        }
        self.online.entries.insert(
            key.to_string(),
            OnlineEntry {
                tried: now_secs(),
                meta,
                candidates,
                chosen: None,
                label: label.to_string(),
            },
        );
        self.save_online()
    }

    /// The user picked one of an entry's candidates: the next run fetches it.
    pub fn choose_online(&mut self, key: &str, idx: usize) -> Result<bool, crate::Error> {
        let Some(e) = self.online.entries.get_mut(key) else {
            return Ok(false);
        };
        let Some(c) = e.candidates.get(idx).cloned() else {
            return Ok(false);
        };
        e.chosen = Some(c);
        e.candidates.clear();
        self.save_online()?;
        Ok(true)
    }

    /// Entries waiting for a choice: `(key, label, candidates)`.
    pub fn online_pending(&self) -> Vec<(String, String, Vec<Candidate>)> {
        self.online
            .entries
            .iter()
            .filter(|(_, e)| !e.candidates.is_empty())
            .map(|(k, e)| (k.clone(), e.label.clone(), e.candidates.clone()))
            .collect()
    }

    /// Drop TMDB data older than its six-month limit (run it every time, even with TMDB switched off), and the
    /// art files nothing refers to any more. Returns how many entries were dropped.
    pub fn online_prune(&mut self, now: u64) -> Result<usize, crate::Error> {
        let before = self.online.entries.len();
        self.online.entries.retain(|_, e| {
            e.meta.as_ref().is_none_or(|m| {
                m.provider != "tmdb" || now.saturating_sub(m.fetched) <= TMDB_MAX_AGE_SECS
            })
        });
        let dropped = before - self.online.entries.len();
        if dropped > 0 {
            self.save_online()?;
            self.sweep_art();
        }
        Ok(dropped)
    }

    /// Forget everything fetched from providers (details and artwork).
    pub fn online_clear(&mut self) -> Result<(), crate::Error> {
        self.online.entries.clear();
        self.save_online()?;
        self.sweep_art();
        Ok(())
    }

    fn sweep_art(&self) {
        let used: std::collections::HashSet<&str> = self
            .online
            .entries
            .values()
            .filter_map(|e| e.meta.as_ref())
            .flat_map(|m| [m.poster.as_deref(), m.backdrop.as_deref()])
            .flatten()
            .collect();
        store::remove_unused(&self.dir.join(ART_DIR), &used);
    }

    pub(crate) fn forget_online_item(&mut self, id: crate::LocalId) {
        self.online.entries.remove(&item_key(id));
    }
}

/// Provider display name for the source label.
pub fn provider_label(p: &str) -> &'static str {
    match p {
        "tmdb" => "TMDB",
        "tvmaze" => "TVmaze",
        "musicbrainz" => "MusicBrainz",
        _ => "online",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmdb(fetched: u64) -> OnlineEntry {
        OnlineEntry {
            tried: fetched,
            meta: Some(OnlineMeta {
                provider: "tmdb".into(),
                fetched,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn refresh_policy() {
        let day = 24 * 3600;
        assert!(stale(None, 0));
        assert!(!stale(Some(&tmdb(0)), 100 * day));
        assert!(
            stale(Some(&tmdb(0)), 151 * day),
            "TMDB data is refreshed before six months"
        );
        let mut mb = tmdb(0);
        mb.meta.as_mut().unwrap().provider = "tvmaze".into();
        assert!(
            !stale(Some(&mb), 400 * day),
            "keyless providers have no age limit"
        );
        // Nothing found: quiet for a month; waiting for the user: never auto-retried.
        let none = OnlineEntry {
            tried: 0,
            ..Default::default()
        };
        assert!(!stale(Some(&none), 29 * day) && stale(Some(&none), 31 * day));
        let ask = OnlineEntry {
            candidates: vec![Candidate::default()],
            ..Default::default()
        };
        assert!(!stale(Some(&ask), 400 * day));
        let picked = OnlineEntry {
            chosen: Some(Candidate::default()),
            ..Default::default()
        };
        assert!(stale(Some(&picked), 0));
    }
}
