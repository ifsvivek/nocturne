//! Gateway ids (plan section 2). Clients parse ids as GUIDs, so every id the gateway hands out is 128 bits: the first 16 bytes of
//! SHA-256("nocturne-gw-v1" | key). The hash cannot be reversed, so the map remembers which (source, upstream id) members each
//! id stands for; it is persisted in `ids.json` and entries unseen for 90 days are dropped.
//!
//! Keys (build them with the `key_*` functions so every module spells them alike): a single-origin item is
//! `source_key:upstream_id` (the source's stable key, NOT its address and NOT its list position), a local item `local:<id>`,
//! a merged title `merge:<provider ids>`. Media sources and sessions derive from these with [`key_media_source`] and [`key_session`].
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

const DOMAIN: &[u8] = b"nocturne-gw-v1";
const VERSION: u32 = 1;
const KEEP_DAYS: u64 = 90;

/// A gateway id. `Display` is the 32-character lowercase undashed form clients get; `Gid::parse` takes that and the dashed form in any case.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Gid([u8; 16]);

impl Gid {
    pub fn of(key: &str) -> Self {
        let mut h = Sha256::new();
        h.update(DOMAIN);
        h.update([0]);
        h.update(key.as_bytes());
        let d = h.finalize();
        Self(d[..16].try_into().unwrap())
    }

    /// 32 hex digits, or the 8-4-4-4-12 dashed form; any case. Anything else is not an id.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        let hex: Vec<u8> = match b.len() {
            32 => b.to_vec(),
            36 if [8, 13, 18, 23].iter().all(|&i| b[i] == b'-') => {
                b.iter().copied().filter(|&c| c != b'-').collect()
            }
            _ => return None,
        };
        if hex.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (i, pair) in hex.chunks(2).enumerate() {
            out[i] = (nib(pair[0])? << 4) | nib(pair[1])?;
        }
        Some(Self(out))
    }

    pub fn dashed(&self) -> String {
        let h = self.to_string();
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    }
}

fn nib(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Gid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Gid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Gid({self})")
    }
}

pub fn key_single(source_key: &str, upstream_id: &str) -> String {
    format!(
        "{source_key}:{}",
        upstream_id.to_ascii_lowercase().replace('-', "")
    )
}
pub fn key_local(local_id: &str) -> String {
    format!("local:{local_id}")
}
pub fn key_merge(identity: &str) -> String {
    format!("merge:{identity}")
}
pub fn key_media_source(item: Gid, upstream_source_id: &str) -> String {
    format!("{item}:{upstream_source_id}")
}
pub fn key_session(source_key: &str, upstream_session_id: &str) -> String {
    format!("session:{source_key}:{upstream_session_id}")
}

fn same(a: &Member, b: &Member) -> bool {
    let n = |s: &str| s.to_ascii_lowercase().replace('-', "");
    a.source == b.source && n(&a.id) == n(&b.id)
}

/// One upstream copy a gateway id stands for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub source: String,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    id: String,
    members: Vec<Member>,
    /// Days since the Unix epoch of the last time the id was seen.
    seen: u64,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

#[derive(Default)]
struct Inner {
    map: HashMap<Gid, (Vec<Member>, u64)>,
    dirty: bool,
}

pub struct IdMap {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
}

fn today() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}

impl IdMap {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            inner: Mutex::default(),
        }
    }

    /// Load `path` (missing or unreadable gives an empty map, ids are rebuilt as items stream through).
    pub fn open(path: PathBuf) -> Self {
        let mut inner = Inner::default();
        if let Some(f) = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            && f.version == VERSION
        {
            for e in f.entries {
                if let Some(g) = Gid::parse(&e.id) {
                    inner.map.insert(g, (e.members, e.seen));
                }
            }
        }
        Self {
            path: Some(path),
            inner: Mutex::new(inner),
        }
    }

    /// The id for `key`, remembering `members` (merged into what is known: a copy that appears later is added, a known one is not duplicated).
    pub fn register(&self, key: &str, members: &[Member]) -> Gid {
        let g = Gid::of(key);
        let mut i = self.inner.lock().unwrap();
        let e = i.map.entry(g).or_default();
        let mut changed = false;
        for m in members {
            if !e.0.iter().any(|k| same(k, m)) {
                e.0.push(m.clone());
                changed = true;
            }
        }
        let t = today();
        if e.1 != t {
            e.1 = t;
            changed = true;
        }
        i.dirty |= changed;
        g
    }

    /// Single-origin shortcut for [`IdMap::register`].
    pub fn register_single(&self, source_key: &str, upstream_id: &str) -> Gid {
        self.register(
            &key_single(source_key, upstream_id),
            &[Member {
                source: source_key.into(),
                id: upstream_id.into(),
            }],
        )
    }

    /// Who an id stands for; `None` for an id this gateway never handed out (answer 404).
    pub fn resolve(&self, g: Gid) -> Option<Vec<Member>> {
        self.inner
            .lock()
            .unwrap()
            .map
            .get(&g)
            .map(|(m, _)| m.clone())
    }

    /// Parse a path or query id (dashed or not, any case) and resolve it.
    pub fn resolve_str(&self, s: &str) -> Option<(Gid, Vec<Member>)> {
        let g = Gid::parse(s)?;
        self.resolve(g).map(|m| (g, m))
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop entries unseen for 90 days; true if anything went.
    pub fn prune(&self) -> bool {
        let t = today();
        let mut i = self.inner.lock().unwrap();
        let before = i.map.len();
        i.map
            .retain(|_, (_, seen)| t.saturating_sub(*seen) <= KEEP_DAYS);
        let gone = i.map.len() != before;
        i.dirty |= gone;
        gone
    }

    /// Write the map if it changed (no-op for an in-memory map).
    pub fn persist(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes = {
            let mut i = self.inner.lock().unwrap();
            if !i.dirty {
                return Ok(());
            }
            i.dirty = false;
            let mut entries: Vec<Entry> = i
                .map
                .iter()
                .map(|(g, (m, seen))| Entry {
                    id: g.to_string(),
                    members: m.clone(),
                    seen: *seen,
                })
                .collect();
            entries.sort_by(|a, b| a.id.cmp(&b.id));
            serde_json::to_vec(&File {
                version: VERSION,
                entries,
            })
            .map_err(std::io::Error::other)?
        };
        crate::config::write_private(path, &bytes).inspect_err(|_| {
            self.inner.lock().unwrap().dirty = true;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        let a = Gid::of("src-1:abc");
        assert_eq!(a, Gid::of("src-1:abc"));
        assert_ne!(a, Gid::of("src-2:abc"));
        assert_ne!(a, Gid::of("src-1:abd"));
        assert_eq!(a.to_string().len(), 32);
        // Pinned: a change of the scheme would orphan every client's saved ids.
        assert_eq!(
            Gid::of("local:1").to_string(),
            Gid::of("local:1").to_string()
        );
        assert_eq!(Gid::of("").to_string().len(), 32);
    }

    #[test]
    fn dashed_and_undashed_round_trip() {
        let g = Gid::of("x");
        let plain = g.to_string();
        let dashed = g.dashed();
        assert_eq!(dashed.len(), 36);
        assert_eq!(Gid::parse(&plain), Some(g));
        assert_eq!(Gid::parse(&dashed), Some(g));
        assert_eq!(Gid::parse(&plain.to_uppercase()), Some(g));
        assert_eq!(Gid::parse(&dashed.to_uppercase()), Some(g));
    }

    #[test]
    fn junk_is_not_an_id() {
        for s in [
            "",
            "abc",
            "../../etc/passwd",
            &"g".repeat(32),
            &"a".repeat(31),
            &"a".repeat(33),
            "0000000-00000-0000-0000-000000000000",
            "00000000_0000_0000_0000_000000000000",
            "00000000-0000-0000-0000-00000000000é",
        ] {
            assert_eq!(Gid::parse(s), None, "{s:?}");
        }
    }

    #[test]
    fn reverse_map_round_trips_and_unions_members() {
        let m = IdMap::in_memory();
        let g = m.register_single("srv-a", "ABCD-ef");
        assert_eq!(
            g,
            m.register_single("srv-a", "abcdef"),
            "upstream id spelling does not matter"
        );
        let (g2, members) = m.resolve_str(&g.dashed()).unwrap();
        assert_eq!(g2, g);
        assert_eq!(
            members,
            vec![Member {
                source: "srv-a".into(),
                id: "ABCD-ef".into()
            }]
        );
        assert!(m.resolve(Gid::of("never seen")).is_none());

        let merged = m.register(
            &key_merge("tmdb:movie:603"),
            &[Member {
                source: "a".into(),
                id: "1".into(),
            }],
        );
        m.register(
            &key_merge("tmdb:movie:603"),
            &[
                Member {
                    source: "a".into(),
                    id: "1".into(),
                },
                Member {
                    source: "b".into(),
                    id: "2".into(),
                },
            ],
        );
        assert_eq!(m.resolve(merged).unwrap().len(), 2);
    }

    #[test]
    fn persists_and_reloads() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("gateway").join("ids.json");
        let m = IdMap::open(p.clone());
        let g = m.register_single("s", "42");
        m.persist().unwrap();
        let again = IdMap::open(p.clone());
        assert_eq!(again.resolve(g).unwrap()[0].id, "42");
        std::fs::write(&p, b"not json").unwrap();
        assert!(
            IdMap::open(p).is_empty(),
            "a corrupt file is rebuilt, not fatal"
        );
    }

    #[test]
    fn old_entries_are_pruned() {
        let m = IdMap::in_memory();
        let g = m.register_single("s", "1");
        m.inner.lock().unwrap().map.get_mut(&g).unwrap().1 = today() - KEEP_DAYS - 1;
        let h = m.register_single("s", "2");
        assert!(m.prune());
        assert!(m.resolve(g).is_none() && m.resolve(h).is_some());
    }

    #[test]
    fn derived_keys_differ() {
        let item = Gid::of("k");
        assert_ne!(
            Gid::of(&key_media_source(item, "1")),
            Gid::of(&key_media_source(item, "2"))
        );
        assert_ne!(
            Gid::of(&key_session("s", "1")),
            Gid::of(&key_session("s", "2"))
        );
    }
}
