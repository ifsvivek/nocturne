//! File and folder naming rules. Pure: path in, `Parsed` out; nothing touches the disk.

use crate::{Media, ProviderIds};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::LazyLock};

macro_rules! re {
    ($s:expr) => {{
        static R: LazyLock<Regex> = LazyLock::new(|| Regex::new($s).unwrap());
        &*R
    }};
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Parsed {
    Movie {
        title: String,
        year: Option<u16>,
        ids: ProviderIds,
    },
    Episode {
        show: String,
        show_year: Option<u16>,
        season: u32,
        episode: u32,
        /// Last episode of a multi-episode file (`S01E02-E03`).
        episode_end: Option<u32>,
        title: Option<String>,
        ids: ProviderIds,
    },
    Track {
        artist: String,
        album: String,
        track: Option<u32>,
        title: String,
    },
    /// Could not be classified; `title` is a cleaned-up guess for display.
    Unmatched { title: String },
}

const EXTRAS: [&str; 8] = [
    "extras",
    "featurettes",
    "behind the scenes",
    "trailers",
    "samples",
    "sample",
    "deleted scenes",
    "interviews",
];

pub fn is_extras_dir(name: &str) -> bool {
    EXTRAS.contains(&name.to_lowercase().as_str())
}

/// `Season 01`, `S2`, `Specials` -> season number (specials are 0).
pub fn season_dir(name: &str) -> Option<u32> {
    if name.eq_ignore_ascii_case("specials") || name.eq_ignore_ascii_case("special") {
        return Some(0);
    }
    re!(r"(?i)^(?:season|series|staffel|saison|s)[ ._-]*(\d{1,2})$")
        .captures(name)
        .and_then(|c| c[1].parse().ok())
}

/// The series folder of an episode file: its parent, or the parent's parent inside a season folder.
pub fn show_dir(file: &Path) -> Option<&Path> {
    let parent = file.parent()?;
    match parent.file_name().and_then(|n| n.to_str()) {
        Some(n) if season_dir(n).is_some() => parent.parent(),
        _ => Some(parent),
    }
}

fn extract_ids(s: &str) -> (String, ProviderIds) {
    let re = re!(r"(?i)[\[{](imdb|tmdb|tvdb)(?:id)?[-=: ]([a-z0-9]+)[\]}]");
    let mut ids = ProviderIds::default();
    for c in re.captures_iter(s) {
        let v = Some(c[2].to_string());
        match c[1].to_lowercase().as_str() {
            "imdb" => ids.imdb = v,
            "tmdb" => ids.tmdb = v,
            _ => ids.tvdb = v,
        }
    }
    (re.replace_all(s, "").into_owned(), ids)
}

/// Dots/underscores are separators only when the name has no spaces ("Mr. Robot" keeps its dot).
fn clean(s: &str) -> String {
    let s = if s.contains(' ') {
        s.replace('_', " ")
    } else {
        s.replace(['.', '_'], " ")
    };
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '-' || c == '.' || c.is_whitespace())
        .to_string()
}

/// Cut at the first release-tag token (`1080p`, `BluRay`, `x264`, ...).
fn cut_junk(s: &str) -> &str {
    let re = re!(
        r"(?i)\b(2160p|1080[pi]|720p|480p|4k|uhd|blu-?ray|bdrip|brrip|web-?dl|web-?rip|hdtv|dvdrip|hdrip|x26[45]|h\.?26[45]|hevc|remux|proper|repack|multi|hdr10?)\b"
    );
    re.find(s).map_or(s, |m| &s[..m.start()])
}

/// Last plausible year in `s` with a non-empty title before it: (title end, year).
fn find_year(s: &str) -> Option<(usize, u16)> {
    re!(r"(?:^|[(\[._\s-])((?:19|20)\d{2})(?:$|[)\].\s_-])")
        .captures_iter(s)
        .filter_map(|c| {
            let y = c.get(1)?;
            (y.start() > 1 && !clean(&s[..c.get(0)?.start()]).is_empty())
                .then(|| (c.get(0).unwrap().start(), y.as_str().parse().unwrap()))
        })
        .last()
}

fn merge(a: &mut ProviderIds, b: ProviderIds) {
    a.imdb = a.imdb.take().or(b.imdb);
    a.tmdb = a.tmdb.take().or(b.tmdb);
    a.tvdb = a.tvdb.take().or(b.tvdb);
}

/// `rel` is the path relative to the library root, including the file name.
pub fn parse(rel: &Path, media: Media) -> Parsed {
    let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let Some((file, dirs)) = comps.split_last() else {
        return Parsed::Unmatched {
            title: String::new(),
        };
    };
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file);
    match media {
        Media::Audio => track(stem, dirs),
        Media::Video => episode(stem, dirs).unwrap_or_else(|| movie(stem, dirs)),
    }
}

fn track(stem: &str, dirs: &[&str]) -> Parsed {
    // Skip a trailing "Disc 1" / "CD2" folder.
    let dirs = match dirs.split_last() {
        Some((d, rest)) if re!(r"(?i)^(?:disc|disk|cd)[ ._-]*\d+$").is_match(d) => rest,
        _ => dirs,
    };
    let (track, title) = match re!(r"^(?:\d{1,2}-)?(\d{1,3})[ ._-]+(.+)$").captures(stem) {
        Some(c) => (c[1].parse().ok(), c[2].trim().to_string()),
        None => (None, stem.trim().to_string()),
    };
    match dirs {
        [.., artist, album] => Parsed::Track {
            artist: artist.to_string(),
            album: album.to_string(),
            track,
            title,
        },
        _ => Parsed::Unmatched { title },
    }
}

fn show_from_dir(name: &str) -> (String, Option<u16>, ProviderIds) {
    let (n, ids) = extract_ids(name);
    let (n, year) = strip_year(&n);
    (n, year, ids)
}

fn strip_year(s: &str) -> (String, Option<u16>) {
    match re!(r"\s*[(\[]((?:19|20)\d{2})[)\]]\s*$").captures(s) {
        Some(c) => (clean(&s[..c.get(0).unwrap().start()]), c[1].parse().ok()),
        None => (clean(s), None),
    }
}

fn episode(stem: &str, dirs: &[&str]) -> Option<Parsed> {
    let (s, mut ids) = extract_ids(stem);
    let season_from_dir = dirs.last().and_then(|d| season_dir(d));
    // Folder the show name can come from: above the season folder, else the parent.
    let show_folder = match (season_from_dir, dirs) {
        (Some(_), [.., show, _]) => Some(*show),
        (Some(_), _) => None,
        (None, [.., show]) => Some(*show),
        _ => None,
    };

    let (season, first, end, prefix, rest);
    if let Some(c) =
        re!(r"(?i)\bs(\d{1,2})[ ._-]?e(\d{1,3})((?:[ ._-]?e\d{1,3}|-\d{1,3}\b)*)").captures(&s)
    {
        let m = c.get(0).unwrap();
        season = c[1].parse().ok()?;
        first = c[2].parse().ok()?;
        end = re!(r"\d+")
            .find_iter(&c[3])
            .last()
            .and_then(|n| n.as_str().parse::<u32>().ok());
        (prefix, rest) = (&s[..m.start()], &s[m.end()..]);
    } else if let Some(c) = re!(r"(?i)\b(\d{1,2})x(\d{2,3})\b").captures(&s) {
        let m = c.get(0).unwrap();
        season = c[1].parse().ok()?;
        first = c[2].parse().ok()?;
        end = None;
        (prefix, rest) = (&s[..m.start()], &s[m.end()..]);
    } else if let (Some(sn), Some(c)) = (
        season_from_dir,
        re!(r"(?i)^e?(\d{1,3})(?:[ ._-]+(.*))?$").captures(&s),
    ) {
        // "Season 01/02 - Name.mkv"
        season = sn;
        first = c[1].parse().ok()?;
        end = None;
        prefix = "";
        rest = c.get(2).map_or("", |m| m.as_str());
    } else {
        return None;
    }

    let (mut show, mut show_year) = strip_year(prefix);
    if let Some(f) = show_folder {
        let (fname, fyear, fids) = show_from_dir(f);
        merge(&mut ids, fids);
        if show.is_empty() {
            show = fname;
        }
        show_year = show_year.or(fyear);
    }
    if show.is_empty() {
        return None;
    }
    let title = Some(clean(cut_junk(rest))).filter(|t| !t.is_empty());
    Some(Parsed::Episode {
        show,
        show_year,
        season,
        episode: first,
        episode_end: end.filter(|e| *e > first),
        title,
        ids,
    })
}

fn movie(stem: &str, dirs: &[&str]) -> Parsed {
    let (s, mut ids) = extract_ids(stem);
    let s = cut_junk(&s);
    let (mut title, mut year) = match find_year(s) {
        Some((end, y)) => (clean(&s[..end]), Some(y)),
        None => (clean(s), None),
    };
    // `Title (Year)/movie.mkv`: fall back to the folder for what the file name lacks.
    if let Some(folder) = dirs.last() {
        let (f, fids) = extract_ids(folder);
        merge(&mut ids, fids);
        let f = cut_junk(&f);
        let found = find_year(f);
        if year.is_none() {
            year = found.map(|(_, y)| y);
        }
        if title.is_empty() || re!(r"(?i)^(movie|film|video|main)$").is_match(&title) {
            title = match found {
                Some((end, _)) => clean(&f[..end]),
                None => clean(f),
            };
        }
    }
    if year.is_none() && ids == ProviderIds::default() {
        return Parsed::Unmatched { title };
    }
    Parsed::Movie { title, year, ids }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(p: &str) -> Parsed {
        parse(Path::new(p), Media::Video)
    }
    fn ep(show: &str, season: u32, episode: u32, end: Option<u32>, title: Option<&str>) -> Parsed {
        Parsed::Episode {
            show: show.into(),
            show_year: None,
            season,
            episode,
            episode_end: end,
            title: title.map(Into::into),
            ids: ProviderIds::default(),
        }
    }
    fn mv(title: &str, year: Option<u16>) -> Parsed {
        Parsed::Movie {
            title: title.into(),
            year,
            ids: ProviderIds::default(),
        }
    }

    #[test]
    fn movies() {
        assert_eq!(v("Heat (1995).mkv"), mv("Heat", Some(1995)));
        assert_eq!(v("Heat (1995)/Heat (1995).mkv"), mv("Heat", Some(1995)));
        assert_eq!(
            v("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
            mv("The Matrix", Some(1999))
        );
        assert_eq!(
            v("Blade Runner 2049 (2017) 2160p.mkv"),
            mv("Blade Runner 2049", Some(2017))
        );
        assert_eq!(v("2012 (2009).mp4"), mv("2012", Some(2009)));
        // year only in the folder; generic file name
        assert_eq!(v("Alien (1979)/movie.mkv"), mv("Alien", Some(1979)));
    }

    #[test]
    fn movie_provider_tags() {
        let Parsed::Movie { title, year, ids } = v("Arrival [imdbid-tt2543164] (2016).mkv") else {
            panic!()
        };
        assert_eq!((title.as_str(), year), ("Arrival", Some(2016)));
        assert_eq!(ids.imdb.as_deref(), Some("tt2543164"));
        let Parsed::Movie { title, year, ids } = v("Dune {tmdb-438631}.mkv") else {
            panic!()
        };
        assert_eq!((title.as_str(), year), ("Dune", None));
        assert_eq!(ids.tmdb.as_deref(), Some("438631"));
        // tag in the folder name
        let Parsed::Movie { ids, .. } = v("Dune [tmdbid-438631]/Dune (2021).mkv") else {
            panic!()
        };
        assert_eq!(ids.tmdb.as_deref(), Some("438631"));
    }

    #[test]
    fn tv_forms() {
        assert_eq!(
            v("Show/Season 01/Show - S01E02 - Pilot.mkv"),
            ep("Show", 1, 2, None, Some("Pilot"))
        );
        assert_eq!(
            v("Show.Name.s1e2.720p.mkv"),
            ep("Show Name", 1, 2, None, None)
        );
        assert_eq!(
            v("Show - 1x02 - Name.mkv"),
            ep("Show", 1, 2, None, Some("Name"))
        );
        assert_eq!(v("Show - S01E02-E03.mkv"), ep("Show", 1, 2, Some(3), None));
        assert_eq!(v("Show S01E02E03.mkv"), ep("Show", 1, 2, Some(3), None));
        assert_eq!(
            v("Show S01E02-03 Two Parter.mkv"),
            ep("Show", 1, 2, Some(3), Some("Two Parter"))
        );
        // a resolution after the episode is not a second episode
        assert_eq!(v("Show S01E02-1080p.mkv"), ep("Show", 1, 2, None, None));
        // resolution in a file name is not an episode marker
        assert!(matches!(
            v("Clip 1920x1080 (2020).mkv"),
            Parsed::Movie { .. }
        ));
    }

    #[test]
    fn tv_show_from_folder() {
        assert_eq!(
            v("Breaking Bad (2008) [tvdbid-81189]/Season 02/S02E03.mkv"),
            Parsed::Episode {
                show: "Breaking Bad".into(),
                show_year: Some(2008),
                season: 2,
                episode: 3,
                episode_end: None,
                title: None,
                ids: ProviderIds {
                    tvdb: Some("81189".into()),
                    ..Default::default()
                },
            }
        );
        // "NN - Name" inside a season folder
        assert_eq!(
            v("Show/Season 3/04 - Four.mkv"),
            ep("Show", 3, 4, None, Some("Four"))
        );
        // file directly in the show folder
        assert_eq!(v("Show/S01E01.mkv"), ep("Show", 1, 1, None, None));
        // dotted name keeps its dot when it has spaces
        assert_eq!(v("Mr. Robot S01E01.mkv"), ep("Mr. Robot", 1, 1, None, None));
    }

    #[test]
    fn specials() {
        assert_eq!(
            v("Show/Specials/Show - S00E01 - Behind.mkv"),
            ep("Show", 0, 1, None, Some("Behind"))
        );
        assert_eq!(
            v("Show/Specials/01 - Extra.mkv"),
            ep("Show", 0, 1, None, Some("Extra"))
        );
        assert_eq!(season_dir("Season 00"), Some(0));
        assert_eq!(season_dir("S2"), Some(2));
        assert_eq!(season_dir("Seasonal"), None);
    }

    #[test]
    fn show_dir_resolution() {
        assert_eq!(
            show_dir(Path::new("/r/Show/Season 01/a.mkv")),
            Some(Path::new("/r/Show"))
        );
        assert_eq!(
            show_dir(Path::new("/r/Show/a.mkv")),
            Some(Path::new("/r/Show"))
        );
    }

    #[test]
    fn music() {
        let t = |p: &str| parse(Path::new(p), Media::Audio);
        assert_eq!(
            t("Artist/Album/01 - Song.flac"),
            Parsed::Track {
                artist: "Artist".into(),
                album: "Album".into(),
                track: Some(1),
                title: "Song".into()
            }
        );
        assert_eq!(
            t("Artist/Album/Disc 2/1-07 Song.mp3"),
            Parsed::Track {
                artist: "Artist".into(),
                album: "Album".into(),
                track: Some(7),
                title: "Song".into()
            }
        );
        assert!(matches!(
            t("Artist/Album/Untitled.mp3"),
            Parsed::Track { track: None, .. }
        ));
        assert!(matches!(t("Song.mp3"), Parsed::Unmatched { .. }));
    }

    #[test]
    fn unmatched() {
        assert_eq!(
            v("random clip.mp4"),
            Parsed::Unmatched {
                title: "random clip".into()
            }
        );
        assert!(matches!(v("Concerts/clip.mp4"), Parsed::Unmatched { .. }));
    }

    #[test]
    fn extras_folders() {
        for n in [
            "Extras",
            "featurettes",
            "Behind The Scenes",
            "Trailers",
            "samples",
        ] {
            assert!(is_extras_dir(n), "{n}");
        }
        assert!(!is_extras_dir("Extra Ordinary"));
    }
}
