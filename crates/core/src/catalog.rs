//! The catalog: one slot per configured source, each with its own backend and status. The UI asks each
//! slot directly (so a slow or failing source never delays the others) and passes the answers to
//! [`crate::merge::merge`]. Nothing here talks to the network on construction.
use crate::backend::{Backend, LocalBackend};
use crate::imgcache::DiskCache;
use crate::merge::SeriesIds;
use crate::profile::{Profile, ProfileKind};
use crate::source::{Media, Source, SourceId};
use nocturne_jellyfin::Config;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Online,
    /// Unreachable; try again at `retry_at` (backoff grows with `tries`).
    Offline {
        tries: u32,
        retry_at: Instant,
    },
    /// No usable token (never signed in here, or the server said 401): sign in again for this source only.
    SignedOut,
    /// The local library is scanning its folders.
    Indexing {
        done: usize,
        total: usize,
    },
    Disabled,
}

impl Status {
    /// Offline after `tries` failed attempts: retry in 5 s, doubling up to a minute.
    pub fn offline(tries: u32, now: Instant) -> Self {
        let wait = Duration::from_secs((5u64 << tries.min(4)).min(60));
        Self::Offline {
            tries,
            retry_at: now + wait,
        }
    }

    /// Can be asked for content now (a source that is still connecting is asked; it may fail).
    pub fn usable(&self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Online | Self::Indexing { .. }
        )
    }
}

pub struct Slot {
    pub id: SourceId,
    pub profile: Profile,
    /// `None` for a disabled or signed-out source (and a library that failed to open).
    pub backend: Option<Backend>,
    pub status: Status,
}

/// Everything construction needs from outside: the token lookup (the keyring in the app, a map in
/// tests) and where files live.
pub struct Env<'a> {
    /// `(server url, user id)` to access token.
    pub token: &'a dyn Fn(&str, &str) -> Option<String>,
    /// `$XDG_DATA_HOME/nocturne`; each local source keeps its index under `local/<source id>`.
    pub data_dir: &'a Path,
    pub disk: Option<Arc<DiskCache>>,
}

#[derive(Default)]
pub struct Catalog {
    /// In priority order.
    pub slots: Vec<Slot>,
}

/// Why a write has nowhere to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    UnknownSource,
    /// Disabled, signed out, or its library did not open.
    SourceUnavailable,
    /// `allow_server_writes` is off.
    WritesOff,
    /// The item's source is a server, not the local library.
    NotLocal,
}

/// Per-user data dir (`$XDG_DATA_HOME/nocturne` on Linux).
pub fn data_dir() -> Option<PathBuf> {
    nocturne_session::paths::data_dir()
}

/// Where a local source keeps its index, overrides and state (never next to the media).
pub fn local_dir(data_dir: &Path, id: SourceId) -> PathBuf {
    data_dir.join("local").join(id.0.to_string())
}

/// Delete a local source's own index and state. The folders it indexed are never touched.
pub fn drop_local_state(data_dir: &Path, id: SourceId) -> std::io::Result<()> {
    match std::fs::remove_dir_all(local_dir(data_dir, id)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

impl Catalog {
    pub fn from_profiles(profiles: &[Profile], env: &Env) -> Self {
        Self {
            slots: profiles.iter().map(|p| Slot::build(p, env)).collect(),
        }
    }

    /// Replace the slot with the same id (keeping its place), or append.
    pub fn upsert(&mut self, slot: Slot) {
        match self.slots.iter_mut().find(|s| s.id == slot.id) {
            Some(old) => *old = slot,
            None => self.slots.push(slot),
        }
    }

    pub fn remove(&mut self, id: SourceId) {
        self.slots.retain(|s| s.id != id);
    }

    /// Put the slots in `ids` order (the profile list); slots not named keep their order, last.
    pub fn reorder(&mut self, ids: &[SourceId]) {
        self.slots
            .sort_by_key(|s| ids.iter().position(|i| *i == s.id).unwrap_or(ids.len()));
    }

    pub fn slot(&self, id: SourceId) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    pub fn slot_mut(&mut self, id: SourceId) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.id == id)
    }

    pub fn backend(&self, id: SourceId) -> Option<&Backend> {
        self.slot(id)?.backend.as_ref()
    }

    pub fn set_status(&mut self, id: SourceId, status: Status) {
        if let Some(s) = self.slot_mut(id) {
            s.status = status;
        }
    }

    /// Sources that can be asked now, in priority order.
    pub fn online(&self) -> impl Iterator<Item = &Slot> {
        self.slots
            .iter()
            .filter(|s| s.backend.is_some() && s.status.usable())
    }

    /// Priority for choosing the preferred copy of a duplicate: the profile order, with sources that
    /// are unavailable (offline, signed out, disabled) moved behind the usable ones.
    pub fn priority(&self) -> Vec<SourceId> {
        let (up, down): (Vec<&Slot>, Vec<&Slot>) = self
            .slots
            .iter()
            .partition(|s| s.backend.is_some() && s.status.usable());
        up.into_iter().chain(down).map(|s| s.id).collect()
    }

    /// The slot a write for an item of `origin` goes to, and only that one. A server source needs
    /// `allow_server_writes`; the local library's own state is not a server and does not.
    pub fn write_slot(
        &self,
        origin: SourceId,
        allow_server_writes: bool,
    ) -> Result<&Slot, WriteError> {
        let s = self.slot(origin).ok_or(WriteError::UnknownSource)?;
        if s.backend.is_none() || matches!(s.status, Status::Disabled | Status::SignedOut) {
            return Err(WriteError::SourceUnavailable);
        }
        if matches!(s.profile.kind, ProfileKind::Jellyfin { .. }) && !allow_server_writes {
            return Err(WriteError::WritesOff);
        }
        Ok(s)
    }

    /// The local library behind a local item (progress and played state live there, never on a server).
    pub fn local_for(&self, m: &Media) -> Result<&LocalBackend, WriteError> {
        match self
            .slot(m.origin)
            .ok_or(WriteError::UnknownSource)?
            .backend
            .as_ref()
        {
            Some(Backend::Local(l)) => Ok(l),
            Some(Backend::Jellyfin(_)) => Err(WriteError::NotLocal),
            None => Err(WriteError::SourceUnavailable),
        }
    }

    /// Learn the provider ids of the series behind `media`'s episodes (one batched read per source
    /// that has any missing), so episodes of different sources can be matched.
    pub async fn fill_series_ids(&self, cache: &mut SeriesIds, media: &[Media]) {
        for (origin, ids) in cache.missing(media) {
            let Some(b) = self
                .slot(origin)
                .filter(|s| s.status.usable())
                .and_then(|s| s.backend.as_ref())
            else {
                continue;
            };
            if let Ok(found) = b.provider_ids(&ids).await {
                for id in ids {
                    // Ids the source did not answer get an empty record, so they are not asked again.
                    cache.set(
                        origin,
                        id.clone(),
                        found.get(&id).cloned().unwrap_or_default(),
                    );
                }
            }
        }
    }
}

impl Slot {
    pub fn build(p: &Profile, env: &Env) -> Self {
        let mut s = Slot {
            id: p.id,
            profile: p.clone(),
            backend: None,
            status: Status::Disabled,
        };
        if !p.enabled {
            return s;
        }
        match &p.kind {
            ProfileKind::Jellyfin { url, user_id, .. } => {
                let Some(token) =
                    (env.token)(url.trim_end_matches('/'), user_id).filter(|t| !t.is_empty())
                else {
                    s.status = Status::SignedOut;
                    return s;
                };
                let cfg = Config {
                    base_url: url.trim_end_matches('/').to_string(),
                    token,
                };
                match Source::with_user(cfg, user_id) {
                    Ok(src) => {
                        let src = src.with_origin(p.id);
                        let src = match &env.disk {
                            Some(d) => src.with_disk(d.clone()),
                            None => src,
                        };
                        s.backend = Some(Backend::Jellyfin(src));
                        s.status = Status::Connecting;
                    }
                    // An address that is not a URL never gets better by retrying.
                    Err(_) => s.status = Status::SignedOut,
                }
            }
            ProfileKind::Local { .. } => {
                match LocalBackend::open(p.id, &local_dir(env.data_dir, p.id)) {
                    Ok(l) => {
                        s.backend = Some(Backend::Local(l));
                        s.status = Status::Online;
                    }
                    Err(_) => s.status = Status::offline(0, Instant::now()),
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::fixture::build;
    use crate::merge::{Preferred, merge};

    fn jf(id: u32, url: &str) -> Profile {
        Profile {
            id: SourceId(id),
            label: format!("s{id}"),
            enabled: true,
            online: Default::default(),
            kind: ProfileKind::Jellyfin {
                url: url.into(),
                user_id: format!("u{id}"),
                user_name: "n".into(),
            },
        }
    }

    fn env<'a>(token: &'a dyn Fn(&str, &str) -> Option<String>, dir: &'a Path) -> Env<'a> {
        Env {
            token,
            data_dir: dir,
            disk: None,
        }
    }

    #[test]
    fn slots_follow_profiles_token_and_enabled_state() {
        let d = tempfile::tempdir().unwrap();
        let tokens = |url: &str, _: &str| (url == "http://a.test").then(|| "T".to_string());
        let mut off = jf(3, "http://a.test");
        off.enabled = false;
        let profiles = [
            jf(1, "http://a.test/"),
            jf(2, "http://b.test"),
            off,
            Profile {
                id: SourceId(4),
                label: "disk".into(),
                enabled: true,
                online: Default::default(),
                kind: ProfileKind::Local {
                    roots: vec!["/x".into()],
                },
            },
            jf(5, "not a url"),
        ];
        let c = Catalog::from_profiles(&profiles, &env(&tokens, d.path()));
        let st: Vec<_> = c.slots.iter().map(|s| s.status.clone()).collect();
        assert_eq!(
            st,
            [
                Status::Connecting,
                Status::SignedOut,
                Status::Disabled,
                Status::Online,
                Status::SignedOut
            ]
        );
        assert_eq!(c.backend(SourceId(1)).unwrap().origin(), SourceId(1));
        assert!(c.backend(SourceId(2)).is_none() && c.backend(SourceId(3)).is_none());
        assert!(c.backend(SourceId(4)).unwrap().is_local());
        let online: Vec<_> = c.online().map(|s| s.id.0).collect();
        assert_eq!(online, [1, 4]);
    }

    #[test]
    fn an_offline_source_is_demoted_in_priority() {
        let d = tempfile::tempdir().unwrap();
        let tokens = |_: &str, _: &str| Some("T".to_string());
        let mut c = Catalog::from_profiles(
            &[
                jf(1, "http://a.test"),
                jf(2, "http://b.test"),
                jf(3, "http://c.test"),
            ],
            &env(&tokens, d.path()),
        );
        let ids = |c: &Catalog| c.priority().iter().map(|i| i.0).collect::<Vec<_>>();
        assert_eq!(ids(&c), [1, 2, 3]);
        c.set_status(SourceId(1), Status::offline(1, Instant::now()));
        c.set_status(SourceId(2), Status::SignedOut);
        assert_eq!(
            ids(&c),
            [3, 1, 2],
            "usable first, the rest keep their order"
        );
        assert!(matches!(
            c.slot(SourceId(1)).unwrap().status,
            Status::Offline { tries: 1, .. }
        ));
        // and the merge honours it: the same title prefers the source that is up.
        let m = |o, id: &str| Media {
            ids: nocturne_local::ProviderIds {
                imdb: Some("tt1".into()),
                ..Default::default()
            },
            ..Media::from_item_in(
                SourceId(o),
                &nocturne_jellyfin::Item {
                    id: id.into(),
                    kind: "Movie".into(),
                    ..Default::default()
                },
            )
        };
        let e = merge(
            vec![
                (SourceId(1), vec![m(1, "a")]),
                (SourceId(3), vec![m(3, "c")]),
            ],
            &c.priority(),
            &Preferred::default(),
            &SeriesIds::default(),
        );
        assert_eq!(e[0].primary.origin, SourceId(3));
    }

    #[test]
    fn slot_ops_keep_ids_and_order_and_dropping_local_state_spares_the_media() {
        let d = tempfile::tempdir().unwrap();
        let tokens = |_: &str, _: &str| Some("T".to_string());
        let e = env(&tokens, d.path());
        let mut c = Catalog::from_profiles(&[jf(1, "http://a.test"), jf(2, "http://b.test")], &e);
        c.reorder(&[SourceId(2), SourceId(1)]);
        assert_eq!(c.slots[0].id, SourceId(2));
        let mut p = jf(2, "http://b.test");
        p.enabled = false;
        c.upsert(Slot::build(&p, &e));
        assert_eq!(c.slots.len(), 2);
        assert_eq!(c.slot(SourceId(2)).unwrap().status, Status::Disabled);
        c.remove(SourceId(1));
        assert_eq!(c.slots.len(), 1);
        // A local source's state dir goes; the media folder next to it stays.
        let media = tempfile::tempdir().unwrap();
        std::fs::write(media.path().join("Heat (1995).mkv"), b"x").unwrap();
        std::fs::create_dir_all(local_dir(d.path(), SourceId(7))).unwrap();
        drop_local_state(d.path(), SourceId(7)).unwrap();
        drop_local_state(d.path(), SourceId(7)).unwrap(); // already gone is fine
        assert!(!local_dir(d.path(), SourceId(7)).exists());
        assert!(media.path().join("Heat (1995).mkv").exists());
    }

    #[test]
    fn backoff_grows_to_a_minute() {
        let now = Instant::now();
        let wait = |n| match Status::offline(n, now) {
            Status::Offline { retry_at, .. } => retry_at - now,
            _ => unreachable!(),
        };
        assert_eq!(wait(0), Duration::from_secs(5));
        assert_eq!(wait(2), Duration::from_secs(20));
        assert_eq!(wait(9), Duration::from_secs(60));
    }

    #[test]
    fn writes_route_to_the_origin_slot_only_and_servers_need_the_switch() {
        let d = tempfile::tempdir().unwrap();
        let tokens = |url: &str, _: &str| (url != "http://nosession.test").then(|| "T".into());
        let mut c = Catalog::from_profiles(
            &[
                jf(1, "http://a.test"),
                jf(2, "http://b.test"),
                jf(3, "http://nosession.test"),
            ],
            &env(&tokens, d.path()),
        );
        for origin in [1, 2] {
            let s = c.write_slot(SourceId(origin), true).unwrap();
            assert_eq!(s.id, SourceId(origin), "never the other server's slot");
            assert_eq!(s.backend.as_ref().unwrap().origin(), SourceId(origin));
        }
        assert_eq!(
            c.write_slot(SourceId(1), false).err(),
            Some(WriteError::WritesOff)
        );
        assert_eq!(
            c.write_slot(SourceId(3), true).err(),
            Some(WriteError::SourceUnavailable)
        );
        assert_eq!(
            c.write_slot(SourceId(9), true).err(),
            Some(WriteError::UnknownSource)
        );
        c.set_status(SourceId(2), Status::Disabled);
        assert!(c.write_slot(SourceId(2), true).is_err());
    }

    #[tokio::test]
    async fn a_local_write_for_origin_a_never_touches_origin_b() {
        const FILES: &[&str] = &["Films/Heat (1995).mkv"];
        let (_ta, a) = build(SourceId(1), FILES);
        let (_tb, b) = build(SourceId(2), FILES);
        let c = Catalog {
            slots: [(1, a.clone()), (2, b.clone())]
                .map(|(id, l)| Slot {
                    id: SourceId(id),
                    profile: Profile {
                        id: SourceId(id),
                        label: "disk".into(),
                        enabled: true,
                        online: Default::default(),
                        kind: ProfileKind::Local { roots: vec![] },
                    },
                    backend: Some(Backend::Local(l)),
                    status: Status::Online,
                })
                .into(),
        };
        let lib_a = a.libraries().await.unwrap().remove(0);
        let ma = a
            .browse(&nocturne_jellyfin::BrowseQuery {
                parent_id: Some(lib_a.id),
                ..Default::default()
            })
            .await
            .unwrap()
            .0
            .remove(0);
        // Both libraries hold an item with the same local id; only the origin tells them apart.
        let mb = Media {
            origin: SourceId(2),
            ..ma.clone()
        };
        assert_eq!(ma.id, mb.id);
        // The write goes where the item's origin says, with writes off (local state is not a server).
        c.local_for(&ma)
            .unwrap()
            .set_played(&ma, true)
            .await
            .unwrap();
        c.local_for(&ma)
            .unwrap()
            .set_progress(&ma, 0)
            .await
            .unwrap();
        let played = |l: &LocalBackend, m: &Media| {
            l.with(|lib| {
                lib.state(nocturne_local::LocalId(m.id.parse().unwrap()))
                    .played
            })
        };
        assert!(played(&a, &ma));
        assert!(!played(&b, &mb), "origin B's state is untouched");
        c.local_for(&mb)
            .unwrap()
            .set_played(&mb, true)
            .await
            .unwrap();
        assert!(played(&b, &mb));
        // A server item has no local route.
        let tokens = |_: &str, _: &str| Some("T".into());
        let d = tempfile::tempdir().unwrap();
        let sc = Catalog::from_profiles(&[jf(1, "http://a.test")], &env(&tokens, d.path()));
        assert_eq!(sc.local_for(&ma).err(), Some(WriteError::NotLocal));
    }
}
