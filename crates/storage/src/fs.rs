//! File-system side of a root: path safety, free space, health, listings and HTTP ranges. All blocking.
use crate::mounts::{self, Mount};
use crate::{Entry, Health, Root, Space};
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

/// `rel` inside `root`, canonical. A leading `/` counts from the root. Refuses `..`, and a target that is (or whose folder is) a symbolic link leading out of the root. The target need not exist (a write); its folder must.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let base = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let full = base.join(plain(rel)?);
    let canon = match full.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // A dangling link must not be written through.
            if full.symlink_metadata().is_ok() {
                return Err("the path leaves the root".into());
            }
            let (Some(dir), Some(name)) = (full.parent(), full.file_name()) else {
                return Err("no such file".into());
            };
            dir.canonicalize()
                .map_err(|_| "no such folder".to_string())?
                .join(name)
        }
        Err(e) => return Err(e.to_string()),
    };
    if canon.starts_with(&base) {
        Ok(canon)
    } else {
        Err("the path leaves the root".into())
    }
}

/// `rel` relative to its root, made only of plain names.
fn plain(rel: &str) -> Result<&Path, String> {
    let rel = Path::new(rel.trim_start_matches('/'));
    if rel
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        Ok(rel)
    } else {
        Err("the path leaves the root".into())
    }
}

/// [`resolve`] for a file to create, making its missing folders first (only below an existing folder inside the root).
pub fn resolve_new(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_p = plain(rel)?;
    let want = rel_p.parent().unwrap_or(Path::new(""));
    let mut have = want;
    while !have.as_os_str().is_empty() && root.join(have).symlink_metadata().is_err() {
        have = have.parent().unwrap_or(Path::new(""));
    }
    if have != want {
        let dir = resolve(root, &have.to_string_lossy())?;
        let rest = want.strip_prefix(have).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir.join(rest)).map_err(|e| e.to_string())?;
    }
    resolve(root, rel)
}

fn c_path(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes()).map_err(io::Error::other)
}

/// Free (for this user) and total bytes of the file system holding `p`.
// The field types are u32 on some 32-bit targets.
#[allow(clippy::unnecessary_cast)]
pub fn space(p: &Path) -> io::Result<Space> {
    let c = c_path(p)?;
    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a valid C string and a buffer of the right type; read only after success.
    if unsafe { libc::statvfs(c.as_ptr(), s.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statvfs succeeded and filled it.
    let s = unsafe { s.assume_init() };
    let unit = s.f_frsize as u64;
    Ok(Space {
        free: s.f_bavail as u64 * unit,
        total: s.f_blocks as u64 * unit,
    })
}

/// This user may create files in `p` (permission bits and read-only mounts).
pub fn writable(p: &Path) -> bool {
    // SAFETY: a valid C string.
    c_path(p).is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
}

/// A local, SMB or NFS root's health. Blocking; may hang on a dead share.
pub fn health(root: &Root, mounts: &[Mount]) -> Health {
    let mounted = mounts::mounted(root, mounts);
    let meta = std::fs::metadata(&root.path);
    let reachable = meta.as_ref().is_ok_and(|m| m.is_dir());
    let error = match &meta {
        Err(e) => Some(format!("{}: {e}", root.path.display())),
        Ok(m) if !m.is_dir() => Some(format!("{} is not a folder", root.path.display())),
        Ok(_) if !mounted => Some("the share or disk is not mounted".into()),
        Ok(_) => None,
    };
    let usable = reachable && mounted;
    Health {
        reachable,
        writable: usable && writable(&root.path),
        mounted,
        space: usable.then(|| space(&root.path).ok()).flatten(),
        error,
    }
}

fn entry(name: String, m: &std::fs::Metadata) -> Entry {
    Entry {
        name,
        dir: m.is_dir(),
        size: if m.is_dir() { 0 } else { m.len() },
        modified: m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs()),
    }
}

pub fn stat(p: &Path) -> io::Result<Entry> {
    let name = p
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    Ok(entry(name, &std::fs::metadata(p)?))
}

/// A folder's entries, folders first, then by name. Unreadable entries are skipped.
pub fn list(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut out: Vec<Entry> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let m = std::fs::metadata(e.path()).ok()?;
            Some(entry(e.file_name().to_string_lossy().into_owned(), &m))
        })
        .collect();
    out.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}
/// A `Range` that cannot be served (HTTP 416).
#[derive(Debug, PartialEq, Eq)]
pub struct Unsatisfiable;

/// A `Range` header against a length: `Ok(None)` = send it all (no header, or several ranges), `Ok(Some((first, last)))` inclusive, `Err` = 416.
pub fn parse_range(h: &str, len: u64) -> Result<Option<(u64, u64)>, Unsatisfiable> {
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None);
    }
    let (a, b) = spec.split_once('-').ok_or(Unsatisfiable)?;
    let num = |s: &str| s.trim().parse::<u64>().map_err(|_| Unsatisfiable);
    let (first, last) = match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, false) => {
            let n = num(b)?.min(len);
            if n == 0 {
                return Err(Unsatisfiable);
            }
            (len - n, len - 1)
        }
        (false, true) => (num(a)?, len.saturating_sub(1)),
        (false, false) => (num(a)?, num(b)?.min(len.saturating_sub(1))),
        (true, true) => return Err(Unsatisfiable),
    };
    if first >= len || first > last {
        return Err(Unsatisfiable);
    }
    Ok(Some((first, last)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_stays_inside() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(d.path().join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(d.path().join("secret"), root.join("out")).unwrap();
        std::os::unix::fs::symlink(d.path(), root.join("outdir")).unwrap();
        std::os::unix::fs::symlink(d.path().join("nope"), root.join("dangling")).unwrap();
        let base = root.canonicalize().unwrap();
        assert_eq!(resolve(&root, "a").unwrap(), base.join("a"));
        assert_eq!(
            resolve(&root, "/a/new.mkv").unwrap(),
            base.join("a/new.mkv")
        );
        assert_eq!(resolve(&root, "").unwrap(), base);
        for bad in [
            "../secret",
            "a/../../secret",
            "out",
            "outdir/secret",
            "outdir/new",
            "dangling",
        ] {
            assert!(resolve(&root, bad).is_err(), "{bad}");
        }
        assert!(resolve(&root, "missing/new").is_err());
        assert_eq!(resolve_new(&root, "x/y/new").unwrap(), base.join("x/y/new"));
        assert!(base.join("x/y").is_dir());
        assert!(resolve_new(&root, "outdir/made/new").is_err());
        assert!(!d.path().join("made").exists());
        assert!(resolve_new(&root, "x/../../made/new").is_err());
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Ok(Some((0, 9))));
        assert_eq!(parse_range("bytes=90-", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=50-500", 100), Ok(Some((50, 99))));
        assert_eq!(parse_range("bytes=0-1,5-6", 100), Ok(None));
        assert_eq!(parse_range("items=0-1", 100), Ok(None));
        assert!(parse_range("bytes=100-", 100).is_err());
        assert!(parse_range("bytes=5-1", 100).is_err());
        assert!(parse_range("bytes=-0", 100).is_err());
    }

    #[test]
    fn health_of_a_folder() {
        let d = tempfile::tempdir().unwrap();
        let mounts = mounts::read(Path::new(mounts::MOUNTINFO));
        let mut root = Root {
            path: d.path().into(),
            ..Root::default()
        };
        let h = health(&root, &mounts);
        assert!(h.reachable && h.writable && h.mounted && h.error.is_none());
        assert!(h.space.unwrap().total > 0);
        root.mount_point = "/no/such/mount".into();
        let h = health(&root, &mounts);
        assert!(!h.mounted && !h.writable && h.error.is_some());
        root.path = d.path().join("gone");
        assert!(!health(&root, &mounts).reachable);
    }
}
