//! Walk the roots, reconcile with the index. Read-only on media paths: only `read_dir`, `metadata` and reads.

use crate::{
    Art, Index, Item, LocalId, Media, Meta, Root, ShowInfo,
    naming::{self, Parsed},
    nfo,
};
use regex::Regex;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

const VIDEO: [&str; 13] = [
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "flv", "webm", "ts", "m2ts", "mpg", "mpeg", "ogv",
];
const AUDIO: [&str; 10] = [
    "mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma", "aiff", "ape",
];
const IMAGE: [&str; 4] = ["jpg", "jpeg", "png", "webp"];
const HASH_CHUNK: u64 = 64 * 1024;
const NFO_MAX: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    /// 0 while the folders are still being walked (`done` is then the files found so far).
    pub total: usize,
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub moved: usize,
    pub missing: usize,
    pub unchanged: usize,
    /// Unreadable entries and similar; never fatal.
    pub warnings: Vec<String>,
    pub unavailable_roots: Vec<PathBuf>,
    pub cancelled: bool,
}

fn classify(path: &Path) -> Option<Media> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    if VIDEO.contains(&ext.as_str()) {
        Some(Media::Video)
    } else if AUDIO.contains(&ext.as_str()) {
        Some(Media::Audio)
    } else {
        None
    }
}

struct Dir {
    /// lower-case name -> real name, for case-insensitive sidecar lookup
    names: HashMap<String, String>,
    path: PathBuf,
    videos: usize,
}

struct Found {
    path: PathBuf,
    root: usize,
    dir: usize,
    media: Media,
    size: u64,
    mtime_ns: i64,
    dev: u64,
    ino: u64,
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x100000001b3);
    }
}

/// Size plus the first and last 64 KiB; enough to recognise a moved file without reading it all.
fn content_hash(path: &Path, size: u64) -> std::io::Result<u64> {
    let mut f = fs::File::open(path)?;
    let mut h = 0xcbf29ce484222325u64;
    fnv(&mut h, &size.to_le_bytes());
    let mut buf = Vec::new();
    f.by_ref().take(HASH_CHUNK).read_to_end(&mut buf)?;
    fnv(&mut h, &buf);
    if size > 2 * HASH_CHUNK {
        buf.clear();
        f.seek(SeekFrom::Start(size - HASH_CHUNK))?;
        f.take(HASH_CHUNK).read_to_end(&mut buf)?;
        fnv(&mut h, &buf);
    }
    Ok(h)
}

fn read_text(path: &Path) -> std::io::Result<String> {
    let mut s = String::new();
    fs::File::open(path)?.take(NFO_MAX).read_to_string(&mut s)?;
    Ok(s)
}

fn pick(dir: &Dir, bases: &[String], exts: &[&str]) -> Option<PathBuf> {
    bases.iter().find_map(|b| {
        exts.iter()
            .find_map(|e| dir.names.get(&format!("{b}.{e}")))
            .map(|n| dir.path.join(n))
    })
}

/// Sidecar nfo and art for one media file, found by name in its directory listing (no I/O).
fn sidecars(file: &Path, dir: &Dir, parsed: &Parsed) -> (Option<PathBuf>, Art) {
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let sole = dir.videos == 1;
    let dir_level =
        |names: &[&str]| -> Vec<String> { names.iter().map(|s| s.to_string()).collect() };
    let (mut nfos, mut poster, mut back) = (vec![stem.clone()], vec![], vec![]);
    match parsed {
        Parsed::Movie { .. } | Parsed::Unmatched { .. } => {
            poster.push(format!("{stem}-poster"));
            back.extend([format!("{stem}-fanart"), format!("{stem}-backdrop")]);
            if sole && matches!(parsed, Parsed::Movie { .. }) {
                poster.extend(dir_level(&["poster", "folder", "cover"]));
                back.extend(dir_level(&["fanart", "backdrop"]));
            }
        }
        Parsed::Episode { .. } => {
            poster.extend([format!("{stem}-thumb"), format!("{stem}-poster")])
        }
        Parsed::Track { .. } => {
            nfos.clear();
            poster.extend(dir_level(&["cover", "folder", "poster", "front"]));
        }
    }
    if sole && matches!(parsed, Parsed::Movie { .. }) {
        nfos.push("movie".into());
    }
    (
        pick(dir, &nfos, &["nfo"]),
        Art {
            poster: pick(dir, &poster, &IMAGE),
            backdrop: pick(dir, &back, &IMAGE),
        },
    )
}

/// Signature of the sidecar files in use: changes when one is added, removed or edited.
fn side_sig(nfo: &Option<PathBuf>, art: &Art) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for p in [nfo, &art.poster, &art.backdrop] {
        match p {
            Some(p) => {
                fnv(&mut h, p.as_os_str().as_encoded_bytes());
                if let Ok(m) = fs::metadata(p) {
                    fnv(&mut h, &m.len().to_le_bytes());
                    fnv(&mut h, &m.mtime().to_le_bytes());
                    fnv(&mut h, &m.mtime_nsec().to_le_bytes());
                }
            }
            None => fnv(&mut h, &[0]),
        }
    }
    h
}

struct Walk {
    found: Vec<Found>,
    dirs: Vec<Dir>,
    /// root index -> readable
    root_ok: Vec<bool>,
    root_dev: Vec<Option<u64>>,
}

fn walk(
    roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
    warnings: &mut Vec<String>,
) -> Walk {
    let mut w = Walk {
        found: vec![],
        dirs: vec![],
        root_ok: vec![false; roots.len()],
        root_dev: vec![None; roots.len()],
    };
    let mut visited: HashSet<(u64, u64)> = HashSet::new();
    for (ri, root) in roots.iter().enumerate() {
        let rm = match fs::metadata(root) {
            Ok(m) if m.is_dir() && fs::read_dir(root).is_ok() => m,
            _ => {
                warnings.push(format!("root unavailable: {}", root.display()));
                continue;
            }
        };
        w.root_ok[ri] = true;
        w.root_dev[ri] = Some(rm.dev());
        visited.insert((rm.dev(), rm.ino()));
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return w;
            }
            let rd = match fs::read_dir(&dir) {
                Ok(rd) => rd,
                Err(e) => {
                    warnings.push(format!("{}: {e}", dir.display()));
                    continue;
                }
            };
            let mut entries = Vec::new();
            for e in rd {
                match e {
                    Ok(e) => entries.push(e),
                    Err(e) => warnings.push(format!("{}: {e}", dir.display())),
                }
            }
            let name_of = |e: &fs::DirEntry| e.file_name().to_string_lossy().into_owned();
            if dir != *root
                && entries
                    .iter()
                    .any(|e| matches!(name_of(e).as_str(), ".nomedia" | ".ignore"))
            {
                continue;
            }
            let di = w.dirs.len();
            let mut listing = Dir {
                names: HashMap::new(),
                path: dir.clone(),
                videos: 0,
            };
            let mut files = Vec::new();
            for e in &entries {
                let name = name_of(e);
                if name.starts_with('.') {
                    continue;
                }
                let path = e.path();
                // metadata() follows symlinks: broken links surface as warnings.
                let m = match fs::metadata(&path) {
                    Ok(m) => m,
                    Err(err) => {
                        warnings.push(format!("{}: {err}", path.display()));
                        continue;
                    }
                };
                if m.is_dir() {
                    if !naming::is_extras_dir(&name) && visited.insert((m.dev(), m.ino())) {
                        stack.push(path);
                    }
                    continue;
                }
                listing.names.insert(name.to_lowercase(), name);
                if let Some(media) = classify(&path) {
                    if media == Media::Video {
                        listing.videos += 1;
                    }
                    files.push(Found {
                        path,
                        root: ri,
                        dir: di,
                        media,
                        size: m.len(),
                        mtime_ns: m.mtime() * 1_000_000_000 + m.mtime_nsec(),
                        dev: m.dev(),
                        ino: m.ino(),
                    });
                }
            }
            w.dirs.push(listing);
            w.found.extend(files);
            if w.found.len() % 64 < 8 {
                progress(Progress {
                    done: w.found.len(),
                    total: 0,
                });
            }
        }
    }
    w
}

fn apply_sidecars(item: &mut Item, dir: &Dir, warnings: &mut Vec<String>) {
    let (nfo_path, art) = sidecars(&item.path, dir, &item.parsed);
    item.side_sig = side_sig(&nfo_path, &art);
    item.art = art;
    item.nfo = nfo_path.and_then(|p| match read_text(&p) {
        Ok(t) => Some(nfo::parse(&t)),
        Err(e) => {
            warnings.push(format!("{}: {e}", p.display()));
            None
        }
    });
}

fn parse_item(item: &mut Item, root: &Path) {
    let rel = item.path.strip_prefix(root).unwrap_or(&item.path);
    item.parsed = naming::parse(rel, item.media);
}

fn show_infos(items: &[Item], scanned: &[PathBuf], warnings: &mut Vec<String>) -> Vec<ShowInfo> {
    static SEASON_ART: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^season(?:(\d{1,2})|-specials)-poster\.(?:jpe?g|png|webp)$").unwrap()
    });
    let dirs: HashSet<&Path> = items
        .iter()
        .filter(|i| {
            matches!(i.parsed, Parsed::Episode { .. })
                && i.missing_since.is_none()
                && scanned.contains(&i.root)
        })
        .filter_map(|i| naming::show_dir(&i.path))
        .collect();
    let mut out = Vec::new();
    for dir in dirs {
        let rd = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                warnings.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        let mut d = Dir {
            names: HashMap::new(),
            path: dir.to_path_buf(),
            videos: 0,
        };
        let mut season_posters = BTreeMap::new();
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(c) = SEASON_ART.captures(&n) {
                let s = c.get(1).map_or(0, |m| m.as_str().parse().unwrap_or(0));
                season_posters.insert(s, e.path());
            }
            d.names.insert(n.to_lowercase(), n);
        }
        let meta = pick(&d, &["tvshow".into()], &["nfo"])
            .and_then(|p| read_text(&p).ok())
            .map(|t| nfo::parse(&t));
        out.push(ShowInfo {
            dir: dir.to_path_buf(),
            meta: meta.filter(|m: &Meta| m.title.is_some() || m.plot.is_some() || m.year.is_some()),
            art: Art {
                poster: pick(
                    &d,
                    &["poster".into(), "folder".into(), "cover".into()],
                    &IMAGE,
                ),
                backdrop: pick(&d, &["fanart".into(), "backdrop".into()], &IMAGE),
            },
            season_posters,
        });
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// Is a readable root really the library it used to be? `recorded` is the root's st_dev at the last good scan.
/// Unmounted if (a) the device changed and none of the files found were indexed under this root before (a replugged
/// drive often gets a new device number but still holds the same files, so overlap clears the doubt), or
/// (b) nothing was found although the index holds files for it (an empty mountpoint).
pub(crate) fn root_looks_unmounted(
    dev: Option<u64>,
    recorded: Option<u64>,
    found: usize,
    overlap: usize,
    indexed: usize,
) -> bool {
    if indexed == 0 {
        return false;
    }
    let changed = matches!((dev, recorded), (Some(d), Some(r)) if d != r);
    found == 0 || (changed && overlap == 0)
}

/// Incremental scan. Mutates `index` in place; on cancel the changes made so far stay valid but nothing is marked
/// missing (we have not seen everything).
pub(crate) fn scan(
    index: &mut Index,
    roots: &[PathBuf],
    now: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> ScanReport {
    let mut rep = ScanReport::default();
    let mut w = walk(roots, cancel, progress, &mut rep.warnings);
    if cancel.load(Ordering::Relaxed) {
        rep.cancelled = true;
        return rep;
    }
    let by_path: HashMap<PathBuf, usize> = index
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| (it.path.clone(), i))
        .collect();
    // A readable root can still be an unplugged drive's empty mountpoint: judge it before trusting the walk.
    let old_roots = std::mem::take(&mut index.roots);
    for (ri, p) in roots.iter().enumerate() {
        let indexed = index.items.iter().filter(|i| i.root == *p).count();
        let (mut found, mut overlap) = (0, 0);
        for f in w.found.iter().filter(|f| f.root == ri) {
            found += 1;
            overlap += by_path
                .get(&f.path)
                .is_some_and(|&i| index.items[i].root == *p) as usize;
        }
        let recorded = old_roots.iter().find(|r| r.path == *p).and_then(|r| r.dev);
        let ok = w.root_ok[ri]
            && !root_looks_unmounted(w.root_dev[ri], recorded, found, overlap, indexed);
        if w.root_ok[ri] && !ok {
            rep.warnings
                .push(format!("root looks unmounted or replaced: {}", p.display()));
        }
        w.root_ok[ri] = ok;
        index.roots.push(Root {
            path: p.clone(),
            available: ok,
            // keep the last known device while the root is away
            dev: if ok { w.root_dev[ri] } else { recorded },
        });
    }
    w.found.retain(|f| w.root_ok[f.root]);
    let scanned: Vec<PathBuf> = index
        .roots
        .iter()
        .filter(|r| r.available)
        .map(|r| r.path.clone())
        .collect();
    rep.unavailable_roots = index
        .roots
        .iter()
        .filter(|r| !r.available)
        .map(|r| r.path.clone())
        .collect();

    let total = w.found.len();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut new: Vec<&Found> = Vec::new();
    for (n, f) in w.found.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            rep.cancelled = true;
            return rep;
        }
        progress(Progress { done: n, total });
        let Some(&i) = by_path.get(&f.path) else {
            new.push(f);
            continue;
        };
        seen.insert(i);
        let it = &mut index.items[i];
        it.missing_since = None;
        it.unavailable = false;
        let same =
            it.size == f.size && it.mtime_ns == f.mtime_ns && it.ino == f.ino && it.dev == f.dev;
        if same {
            rep.unchanged += 1;
        } else {
            (it.size, it.mtime_ns, it.dev, it.ino) = (f.size, f.mtime_ns, f.dev, f.ino);
            it.hash = content_hash(&f.path, f.size).unwrap_or_else(|e| {
                rep.warnings.push(format!("{}: {e}", f.path.display()));
                0
            });
            it.probe = None;
            parse_item(it, &roots[f.root]);
            rep.updated += 1;
        }
        let (nfo_path, art) = sidecars(&it.path, &w.dirs[f.dir], &it.parsed);
        if !same || side_sig(&nfo_path, &art) != it.side_sig {
            apply_sidecars(it, &w.dirs[f.dir], &mut rep.warnings);
        }
    }

    // Vanished: indexed under a root we could read, but not found this time.
    let mut vanished: Vec<usize> = (0..index.items.len())
        .filter(|i| !seen.contains(i) && scanned.contains(&index.items[*i].root))
        .collect();
    for f in new {
        let hash = content_hash(&f.path, f.size).unwrap_or_else(|e| {
            rep.warnings.push(format!("{}: {e}", f.path.display()));
            0
        });
        // Same (dev, inode) and size, or same size and content hash (a copy that replaced the original).
        let hit = vanished.iter().position(|&i| {
            let v = &index.items[i];
            v.size == f.size
                && ((v.dev == f.dev && v.ino == f.ino) || (hash != 0 && v.hash == hash))
        });
        let idx = match hit {
            Some(p) => {
                rep.moved += 1;
                vanished.swap_remove(p)
            }
            None => {
                rep.added += 1;
                let id = LocalId(index.next_id);
                index.next_id += 1;
                index.items.push(Item::new(id, now));
                index.items.len() - 1
            }
        };
        let item = &mut index.items[idx];
        item.path = f.path.clone();
        item.root = roots[f.root].clone();
        item.media = f.media;
        (item.size, item.mtime_ns, item.dev, item.ino, item.hash) =
            (f.size, f.mtime_ns, f.dev, f.ino, hash);
        item.missing_since = None;
        item.unavailable = false;
        item.probe = None;
        parse_item(item, &roots[f.root]);
        apply_sidecars(item, &w.dirs[f.dir], &mut rep.warnings);
    }
    for i in vanished {
        let it = &mut index.items[i];
        if it.missing_since.is_none() {
            it.missing_since = Some(now);
            rep.missing += 1;
        }
    }
    // Files whose root is not readable now (or no longer configured) are unavailable, not missing.
    for it in &mut index.items {
        if !scanned.contains(&it.root) {
            it.unavailable = true;
        }
    }
    let kept = std::mem::take(&mut index.shows);
    index.shows = show_infos(&index.items, &scanned, &mut rep.warnings);
    index.shows.extend(
        kept.into_iter()
            .filter(|s| !scanned.iter().any(|r| s.dir.starts_with(r))),
    );
    progress(Progress { done: total, total });
    rep
}

#[cfg(test)]
mod tests {
    use super::root_looks_unmounted as f;

    #[test]
    fn unmounted_decision() {
        // empty root with indexed files: unavailable, whatever the device
        assert!(f(Some(1), Some(1), 0, 0, 5));
        assert!(f(Some(2), None, 0, 0, 5));
        // device changed and nothing we knew: a different disk is mounted there
        assert!(f(Some(2), Some(1), 3, 0, 5));
        // device changed but the same files are there: replugged drive
        assert!(!f(Some(2), Some(1), 5, 5, 5));
        // same device with files; never-indexed roots
        assert!(!f(Some(1), Some(1), 3, 3, 5));
        assert!(!f(Some(1), None, 3, 0, 0));
        assert!(!f(Some(1), Some(1), 0, 0, 0));
    }
}
