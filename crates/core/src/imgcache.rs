//! Bounded on-disk image cache: `$XDG_CACHE_HOME/nocturne/images`, least recently used files
//! evicted first when the total size passes the cap. File names are opaque hashes of
//! (server, item, tag, width), so nothing about the library is readable from the directory.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

pub struct DiskCache {
    dir: PathBuf,
    max: u64,
    /// Bytes on disk, counted once at open and then tracked.
    total: Mutex<u64>,
}

/// Two FNV-1a passes with different seeds -> 32 hex chars (not cryptographic, only opaque).
pub fn name_for(key: &str) -> String {
    let fnv = |seed: u64| {
        key.bytes().fold(seed, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    format!(
        "{:016x}{:016x}",
        fnv(0xcbf2_9ce4_8422_2325),
        fnv(0x84222325cbf29ce4)
    )
}

impl DiskCache {
    pub fn open(dir: PathBuf, max_bytes: u64) -> Self {
        let _ = fs::create_dir_all(&dir);
        let total = entries(&dir).iter().map(|e| e.1).sum();
        Self {
            dir,
            max: max_bytes,
            total: Mutex::new(total),
        }
    }

    /// `$XDG_CACHE_HOME/nocturne/images` (or `~/.cache/...`).
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
        Some(base.join("nocturne").join("images"))
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let p = self.dir.join(name_for(key));
        let b = fs::read(&p).ok()?;
        // Touch: a hit makes the file the most recently used.
        if let Ok(f) = fs::OpenOptions::new().append(true).open(&p) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(b)
    }

    pub fn put(&self, key: &str, bytes: &[u8]) {
        let p = self.dir.join(name_for(key));
        let old = fs::metadata(&p).map_or(0, |m| m.len());
        let tmp = p.with_extension("tmp");
        if fs::write(&tmp, bytes).is_err() || fs::rename(&tmp, &p).is_err() {
            let _ = fs::remove_file(&tmp);
            return;
        }
        let mut total = self.total.lock().unwrap();
        *total = total.saturating_sub(old) + bytes.len() as u64;
        if *total > self.max {
            *total = evict(&self.dir, self.max);
        }
    }
}

/// (path, size, last-used) of every cache file.
fn entries(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            Some((
                e.path(),
                m.len(),
                m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            ))
        })
        .collect()
}

/// Delete oldest files until the total is within `max`; returns the new total.
fn evict(dir: &Path, max: u64) -> u64 {
    let mut all = entries(dir);
    all.sort_by_key(|e| e.2);
    let mut total: u64 = all.iter().map(|e| e.1).sum();
    for (p, size, _) in all {
        if total <= max {
            break;
        }
        if fs::remove_file(p).is_ok() {
            total -= size;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nocturne-imgcache-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn age(c: &DiskCache, key: &str, secs: u64) {
        let f = fs::OpenOptions::new()
            .append(true)
            .open(c.dir.join(name_for(key)))
            .unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn names_are_opaque_and_stable() {
        let a = name_for("srv|item-1|tag|300");
        assert_eq!(a, name_for("srv|item-1|tag|300"));
        assert_ne!(a, name_for("srv|item-1|tag|301"));
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn round_trip_and_miss() {
        let d = tmp("rt");
        let c = DiskCache::open(d.clone(), 1000);
        assert!(c.get("k").is_none());
        c.put("k", b"hello");
        assert_eq!(c.get("k").unwrap(), b"hello");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn evicts_least_recently_used_by_size() {
        let d = tmp("lru");
        let c = DiskCache::open(d.clone(), 250);
        for (k, secs) in [("a", 300), ("b", 200), ("c", 100)] {
            c.put(k, &[0u8; 100]);
            age(&c, k, secs);
        }
        // 300 > 250 after the third put already evicted the oldest ("a").
        assert!(c.get("a").is_none());
        // Reading "b" makes it newest, so the next eviction takes "c".
        age(&c, "c", 50);
        assert!(c.get("b").is_some());
        c.put("d", &[0u8; 100]);
        assert!(c.get("c").is_none(), "c was least recently used");
        assert!(c.get("b").is_some() && c.get("d").is_some());
        assert!(entries(&d).iter().map(|e| e.1).sum::<u64>() <= 250);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn reopen_counts_existing_files() {
        let d = tmp("reopen");
        DiskCache::open(d.clone(), 1000).put("x", &[0u8; 400]);
        let c = DiskCache::open(d.clone(), 500);
        c.put("y", &[0u8; 400]);
        assert!(entries(&d).iter().map(|e| e.1).sum::<u64>() <= 500);
        fs::remove_dir_all(d).unwrap();
    }
}
