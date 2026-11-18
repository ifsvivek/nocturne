//! One transfer: GET the original file (resuming a `.part` with a Range request), then publish it
//! under its final name without ever replacing a file, and add the `.nfo` and subtitle sidecars.

use super::{Job, Why, naming, nfo_for, show_nfo_for, write_new};
use nocturne_jellyfin::{Client, Error};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

pub const RUN: u8 = 0;
pub const PAUSE: u8 = 1;
pub const CANCEL: u8 = 2;

/// A silent connection is a dropped one after this long.
const STALL: Duration = Duration::from_secs(30);

/// What the UI reads while a transfer runs, and the switch it flips to stop it.
#[derive(Default)]
pub struct Shared {
    pub ctl: AtomicU8,
    pub done: AtomicU64,
    pub total: AtomicU64,
    /// Where the file goes (decided on the first answer from the server).
    pub dest: Mutex<Option<PathBuf>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Done,
    Paused,
    /// The `.part` is deleted.
    Cancelled,
}

pub fn why(e: &Error) -> Why {
    match e {
        // Jellyfin answers 403 when the user's policy has EnableContentDownloading off.
        Error::Status { status: 403, .. } => Why::Denied,
        Error::Status { status: 401, .. } => Why::Auth,
        Error::Status { status: 404, .. } => Why::Missing,
        _ => Why::Network,
    }
}

fn disk(_: std::io::Error) -> Why {
    Why::Disk
}

/// Open the body from `from`: `(response, start)` where `start` is where the body really begins
/// (0 when the server ignored the Range header and sends the whole file).
async fn open(c: &Client, item: &str, from: u64) -> Result<(reqwest::Response, u64), Why> {
    match c.download(item, from).await {
        Ok(r) if from > 0 && r.status().as_u16() == 206 => Ok((r, from)),
        Ok(r) => Ok((r, 0)),
        // The part is at or past the end: start over rather than guess.
        Err(Error::Status { status: 416, .. }) if from > 0 => {
            let r = c.download(item, 0).await.map_err(|e| why(&e))?;
            Ok((r, 0))
        }
        Err(e) => Err(why(&e)),
    }
}

/// Run `job` until it is done, paused or cancelled (`sh.ctl`), or fails.
pub async fn run(c: &Client, user: &str, job: &Job, sh: &Shared) -> Result<End, Why> {
    let mut dest = job.dest.clone();
    let have = dest
        .as_deref()
        .and_then(|d| std::fs::metadata(naming::part_of(d)).ok())
        .map_or(0, |m| m.len());
    let (mut resp, start) = open(c, &job.item, have).await?;
    let total = start + resp.content_length().unwrap_or(0);
    let known = resp.content_length().is_some();
    let ext = naming::ext_from(
        resp.headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok()),
        None,
    );
    let dest = match dest.take() {
        Some(d) => d,
        None => {
            let (rel, stem) = naming::place(&job.meta);
            let dir = job.root.join(rel);
            std::fs::create_dir_all(&dir).map_err(disk)?;
            naming::free_path(&dir, &stem, &ext)
        }
    };
    *sh.dest.lock().unwrap_or_else(|e| e.into_inner()) = Some(dest.clone());
    sh.total
        .store(if known { total } else { 0 }, Ordering::Relaxed);
    sh.done.store(start, Ordering::Relaxed);
    let part = naming::part_of(&dest);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(start > 0)
        .truncate(start == 0)
        .open(&part)
        .map_err(disk)?;
    let mut done = start;
    loop {
        match sh.ctl.load(Ordering::Relaxed) {
            PAUSE => {
                file.flush().map_err(disk)?;
                return Ok(End::Paused);
            }
            CANCEL => {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Ok(End::Cancelled);
            }
            _ => {}
        }
        // ponytail: blocking writes of small chunks on the runtime thread; a writer thread if a slow disk ever stalls the UI.
        match tokio::time::timeout(STALL, resp.chunk()).await {
            Ok(Ok(Some(b))) => {
                file.write_all(&b).map_err(disk)?;
                done += b.len() as u64;
                sh.done.store(done, Ordering::Relaxed);
            }
            Ok(Ok(None)) => break,
            _ => {
                let _ = file.flush();
                return Err(Why::Network);
            }
        }
    }
    if known && done != total {
        return Err(Why::Network); // cut short: the part stays for a resume
    }
    file.sync_all().map_err(disk)?;
    drop(file);
    let dest = publish(&part, &dest).map_err(disk)?;
    *sh.dest.lock().unwrap_or_else(|e| e.into_inner()) = Some(dest.clone());
    sidecars(c, user, job, &dest).await;
    Ok(End::Done)
}

/// Move the finished part to its name without replacing anything: a hard link fails if the name
/// exists (and the part is dropped after); where links are unsupported, a checked rename.
/// If the name was taken meanwhile, the next free one is used.
pub fn publish(part: &Path, dest: &Path) -> std::io::Result<PathBuf> {
    let mut to = dest.to_path_buf();
    for _ in 0..50 {
        match std::fs::hard_link(part, &to) {
            Ok(()) => {
                std::fs::remove_file(part)?;
                return Ok(to);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let (dir, name) = (to.parent().unwrap_or(Path::new(".")), to.clone());
                let stem = name.file_stem().and_then(|s| s.to_str()).unwrap_or("copy");
                let ext = name.extension().and_then(|s| s.to_str()).unwrap_or("mkv");
                // A name with " - N" already: count on from the bare stem.
                let base = stem.rsplit_once(" - ").map_or(stem, |(b, n)| {
                    if n.chars().all(|c| c.is_ascii_digit()) {
                        b
                    } else {
                        stem
                    }
                });
                to = naming::free_path(dir, base, ext);
            }
            Err(_) if !to.exists() => {
                std::fs::rename(part, &to)?;
                return Ok(to);
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free name"))
}

/// `.nfo`, `tvshow.nfo` and external text subtitles beside the new file. Best effort: the video is
/// already safe, and an existing file is never replaced.
async fn sidecars(c: &Client, user: &str, job: &Job, dest: &Path) {
    let Some(dir) = dest.parent() else { return };
    write_new(&dest.with_extension("nfo"), &nfo_for(&job.meta));
    if let (Some(text), Some(show_dir)) = (show_nfo_for(&job.meta), show_dir(dir, &job.meta)) {
        write_new(&show_dir.join("tvshow.nfo"), &text);
    }
    let Ok(item) = c.play_item(user, &job.item).await else {
        return;
    };
    let Some(src) = item.media_sources.first() else {
        return;
    };
    let stem = dest
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle");
    for s in src
        .media_streams
        .iter()
        .filter(|s| s.kind == "Subtitle" && s.is_external && s.is_text_subtitle_stream)
    {
        let Ok(bytes) = c.subtitle_bytes(&job.item, &src.id, s.index).await else {
            continue;
        };
        let lang = s
            .language
            .as_deref()
            .filter(|l| !l.is_empty())
            .unwrap_or("und");
        let lang: String = lang.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        // `Name.en.srt`, then `Name.en.2.srt`: players pair these with `Name.<video ext>`.
        let target = (1..)
            .map(|n| match n {
                1 => dir.join(format!("{stem}.{lang}.srt")),
                n => dir.join(format!("{stem}.{lang}.{n}.srt")),
            })
            .find(|p| !p.exists())
            .expect("unbounded");
        if let Ok(text) = String::from_utf8(bytes) {
            write_new(&target, &text);
        }
    }
}

/// The show's folder: the episode's folder, or its parent when that is a `Season NN` folder.
fn show_dir<'a>(dir: &'a Path, m: &super::Meta) -> Option<&'a Path> {
    m.show.as_ref()?;
    match dir.file_name().and_then(|n| n.to_str()) {
        Some(n) if n.starts_with("Season ") => dir.parent(),
        _ => Some(dir),
    }
}

/// Delete what an abandoned job left behind: only its `.part`.
pub fn discard_part(job: &Job) {
    if let Some(p) = job.part() {
        let _ = std::fs::remove_file(p);
    }
}

/// Sizes of items (bytes; `None` where the server doesn't say), read one after another.
pub async fn sizes(c: &Client, user: &str, ids: &[String]) -> Vec<Option<u64>> {
    let mut out = vec![];
    for id in ids {
        out.push(
            c.play_item(user, id)
                .await
                .ok()
                .and_then(|p| p.media_sources.first().and_then(|s| s.size)),
        );
    }
    out
}

#[cfg(test)]
mod tests;
