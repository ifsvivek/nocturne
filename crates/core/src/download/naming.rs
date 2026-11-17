//! Where an offline copy goes and what it is called: Jellyfin's own layout, so the local scanner
//! (and a Jellyfin server pointed at the same folder) recognise it. Pure; nothing touches the disk
//! except `free_path`, which only looks.

use super::Meta;
use std::path::{Path, PathBuf};

/// One path component: no separators or characters a file system or the scanner trips on.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ':' => out.push('-'),
            '/' | '\\' | '*' | '?' | '"' | '<' | '>' | '|' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let out: String = out.chars().take(120).collect();
    let out = out.trim_matches(|c| c == '.' || c == ' ');
    if out.is_empty() {
        "Untitled".into()
    } else {
        out.to_string()
    }
}

/// Folder (relative to the library root) and file stem of an item.
/// Movie: `Title (Year)/Title (Year)`. Episode: `Show/Season 01/Show - S01E02 - Name`.
pub fn place(m: &Meta) -> (PathBuf, String) {
    match (&m.show, m.episode) {
        (Some(show), Some(ep)) => {
            let season = m.season.unwrap_or(1);
            let show = clean(show);
            let mut stem = format!("{show} - S{season:02}E{ep:02}");
            if !m.title.is_empty() {
                stem.push_str(" - ");
                stem.push_str(&clean(&m.title));
            }
            (
                PathBuf::from(&show).join(format!("Season {season:02}")),
                stem,
            )
        }
        _ => {
            let mut stem = clean(&m.title);
            if let Some(y) = m.year {
                stem.push_str(&format!(" ({y})"));
            }
            (PathBuf::from(&stem), stem)
        }
    }
}

/// `dir/stem.ext`, or `dir/stem - 2.ext`, `- 3`, ... when that, or its `.part`, is taken.
/// Never returns a path that exists, so a download can not overwrite anything.
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let taken = |p: &Path| p.exists() || part_of(p).exists();
    let first = dir.join(format!("{stem}.{ext}"));
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} - {n}.{ext}")))
        .find(|p| !taken(p))
        .expect("an unbounded range has a free name")
}

pub fn part_of(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    s.into()
}

/// A container extension from the server's `Content-Disposition` file name, else from the media
/// source's container (`mov,mp4,m4a` -> `mov`), else `mkv`.
pub fn ext_from(disposition: Option<&str>, container: Option<&str>) -> String {
    let from_name = disposition.and_then(|d| {
        let i = d.to_ascii_lowercase().find("filename")?;
        let name = d[i..].split_once('=')?.1;
        let name = name.split(';').next()?.trim().trim_matches('"');
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        (!ext.is_empty() && ext.len() <= 5 && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
            .then_some(ext)
    });
    from_name
        .or_else(|| {
            container
                .and_then(|c| c.split(',').next())
                .map(|c| c.trim().to_ascii_lowercase())
                .filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric()))
        })
        .unwrap_or_else(|| "mkv".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::Meta;

    fn movie(title: &str, year: Option<u32>) -> Meta {
        Meta {
            title: title.into(),
            year,
            ..Default::default()
        }
    }

    #[test]
    fn movies_and_episodes_follow_jellyfin_layout() {
        let (d, s) = place(&movie("Heat", Some(1995)));
        assert_eq!((d, s), (PathBuf::from("Heat (1995)"), "Heat (1995)".into()));
        let ep = Meta {
            title: "Pilot".into(),
            show: Some("Some Show".into()),
            season: Some(1),
            episode: Some(2),
            ..Default::default()
        };
        let (d, s) = place(&ep);
        assert_eq!(d, PathBuf::from("Some Show/Season 01"));
        assert_eq!(s, "Some Show - S01E02 - Pilot");
    }

    #[test]
    fn unsafe_characters_are_dropped() {
        assert_eq!(clean("AC/DC: Live? <1991>"), "ACDC- Live 1991");
        assert_eq!(clean("..."), "Untitled");
        assert_eq!(clean("  a   b "), "a b");
        let (d, _) = place(&movie("../../etc", None));
        assert_eq!(d, PathBuf::from("etc"));
    }

    #[test]
    fn a_taken_name_is_never_reused() {
        let t = tempfile::tempdir().unwrap();
        let p = free_path(t.path(), "Heat (1995)", "mkv");
        assert_eq!(p, t.path().join("Heat (1995).mkv"));
        std::fs::write(&p, b"x").unwrap();
        let p2 = free_path(t.path(), "Heat (1995)", "mkv");
        assert_eq!(p2, t.path().join("Heat (1995) - 2.mkv"));
        std::fs::write(part_of(&p2), b"x").unwrap(); // an unfinished download also holds its name
        assert_eq!(
            free_path(t.path(), "Heat (1995)", "mkv"),
            t.path().join("Heat (1995) - 3.mkv")
        );
    }

    #[test]
    fn extension_from_header_then_container() {
        let d = r#"attachment; filename="Heat (1995).MP4"; filename*=UTF-8''Heat.mp4"#;
        assert_eq!(ext_from(Some(d), Some("mkv")), "mp4");
        assert_eq!(ext_from(None, Some("mov,mp4,m4a")), "mov");
        assert_eq!(ext_from(Some("attachment"), None), "mkv");
        assert_eq!(
            ext_from(Some("attachment; filename=\"noext\""), Some("avi")),
            "avi"
        );
    }
}
