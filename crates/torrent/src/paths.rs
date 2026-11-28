//! Input checks at the trust boundary: magnet links, save paths and torrent names come from indexers and users.

use std::path::{Component, Path, PathBuf};

use crate::Error;

/// Largest .torrent file accepted. librqbit separately caps metadata fetched from peers for magnets at 32 MiB.
pub const MAX_TORRENT_BYTES: usize = 16 * 1024 * 1024;

/// Accepts only `magnet:?xt=urn:btih:...` links whose trackers are http, https or udp. Anything that could make the engine read a local file (`file://`, a bare path, a bare info hash that librqbit would also try as a filename) is rejected. Returns the lowercase info-hash hex.
pub fn check_magnet(uri: &str) -> Result<String, Error> {
    let bad = |m: &str| Error::InvalidMagnet(m.to_owned());
    if !uri
        .get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("magnet:?"))
    {
        return Err(bad("not a magnet: link"));
    }
    let magnet =
        librqbit::Magnet::parse(uri).map_err(|e| Error::InvalidMagnet(format!("{e:#}")))?;
    let hash = magnet
        .as_id20()
        .ok_or_else(|| bad("no BitTorrent v1 info hash (xt=urn:btih:)"))?;
    for t in &magnet.trackers {
        let scheme = t.split_once("://").map(|(s, _)| s.to_ascii_lowercase());
        if !matches!(scheme.as_deref(), Some("http" | "https" | "udp")) {
            return Err(bad("tracker is not http, https or udp"));
        }
    }
    Ok(hash.as_string())
}

/// Resolves a save path against `root` (the download dir, already canonical): relative paths are joined to it, absolute ones must lie inside it. `..` is never allowed. The directory is created, then its canonical form (symlinks resolved) must still be inside `root`.
pub fn resolve_save_path(root: &Path, save: Option<&Path>) -> Result<PathBuf, Error> {
    let Some(save) = save else {
        return Ok(root.to_owned());
    };
    let escape = || Error::PathEscape(save.to_owned());
    if save.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(escape());
    }
    let joined = if save.is_absolute() {
        save.to_owned()
    } else {
        root.join(save)
    };
    if !joined.starts_with(root) {
        return Err(escape());
    }
    std::fs::create_dir_all(&joined)?;
    let canonical = joined.canonicalize()?;
    if !canonical.starts_with(root) {
        return Err(escape());
    }
    Ok(canonical)
}

/// Where a torrent's files go: multi-file torrents get a sub-folder named after the torrent (like qBittorrent's default layout), single files land directly in the save path. The name must be one plain path component.
pub fn content_dir(
    save: &Path,
    name: Option<&str>,
    file_count: usize,
    hash: &str,
) -> Result<PathBuf, Error> {
    if file_count < 2 {
        return Ok(save.to_owned());
    }
    let name = name.filter(|n| !n.is_empty()).unwrap_or(hash);
    let mut comps = Path::new(name).components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => Ok(save.join(name)),
        _ => Err(Error::PathEscape(PathBuf::from(name))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn magnet_accepts_plain_link() {
        let m = format!("magnet:?xt=urn:btih:{HASH}&dn=x&tr=udp%3A%2F%2Ft.example%3A80");
        assert_eq!(check_magnet(&m).unwrap(), HASH);
        let upper = format!("MAGNET:?xt=urn:btih:{}", HASH.to_uppercase());
        assert_eq!(check_magnet(&upper).unwrap(), HASH);
    }

    #[test]
    fn magnet_rejects_local_sources() {
        for bad in [
            "file:///etc/passwd".to_owned(),
            "/home/user/x.torrent".to_owned(),
            HASH.to_owned(),
            format!("magnet:?xt=urn:btih:{HASH}&tr=file%3A%2F%2F%2Fetc%2Fpasswd"),
            format!("magnet:?xt=urn:btih:{HASH}&tr=%2Ftmp%2Fx"),
            "magnet:?dn=nohash".to_owned(),
        ] {
            assert!(check_magnet(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn save_path_stays_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert_eq!(resolve_save_path(&root, None).unwrap(), root);
        assert_eq!(
            resolve_save_path(&root, Some(Path::new("tv"))).unwrap(),
            root.join("tv")
        );
        assert_eq!(
            resolve_save_path(&root, Some(&root.join("movies"))).unwrap(),
            root.join("movies")
        );
        for bad in ["../x", "tv/../../x", "/tmp"] {
            assert!(
                matches!(
                    resolve_save_path(&root, Some(Path::new(bad))),
                    Err(Error::PathEscape(_))
                ),
                "{bad}"
            );
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
            assert!(matches!(
                resolve_save_path(&root, Some(Path::new("link/x"))),
                Err(Error::PathEscape(_))
            ));
        }
    }

    #[test]
    fn content_dir_layout_and_name_checks() {
        let save = Path::new("/d");
        assert_eq!(content_dir(save, Some("a"), 1, HASH).unwrap(), save);
        assert_eq!(
            content_dir(save, Some("Show S01"), 3, HASH).unwrap(),
            save.join("Show S01")
        );
        assert_eq!(content_dir(save, None, 3, HASH).unwrap(), save.join(HASH));
        for bad in ["..", "a/b", "/etc", "."] {
            assert!(content_dir(save, Some(bad), 2, HASH).is_err(), "{bad}");
        }
    }
}
