//! Quality profiles and release scoring. Pure.
//!
//! The daemon calls [`score_with`] (or [`score`] when nothing is on disk yet and the runtime is unknown) for every search result, drops the ones with `accepted == false` (their `reasons` say why, for the UI), and grabs the accepted release with the highest `rank`.
//!
//! `rank` orders, most significant first: the quality's position in the profile's `allowed` list, the preferred-term score, the revision (PROPER/REPACK/v2), then seeders (torrents, capped at 999 999). It is only comparable between releases scored with the same profile.

use crate::parse::{QualityInfo, parse_book, parse_episode, parse_movie, parse_music};
use crate::{MediaKind, Protocol, Quality, Release};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

/// A preferred term and its score (negative to discourage).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Term {
    /// Case-insensitive substring, or `/regex/` (case-insensitive).
    pub pattern: String,
    pub score: i32,
}

/// Size limits. Per-minute limits need the runtime ([`Context::runtime_minutes`]) and are skipped without it; total limits always apply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SizeLimit {
    pub min_mb_per_minute: Option<f64>,
    pub max_mb_per_minute: Option<f64>,
    pub min_gb: Option<f64>,
    pub max_gb: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Which parser reads release names for this profile.
    pub kind: MediaKind,
    /// Allowed qualities, worst first. Anything else is rejected.
    pub allowed: Vec<Quality>,
    /// Stop upgrading once a file of this quality (or better) is on disk.
    pub cutoff: Quality,
    pub upgrade_allowed: bool,
    #[serde(default)]
    pub preferred: Vec<Term>,
    /// At least one must match (when any are set).
    #[serde(default)]
    pub required: Vec<String>,
    /// None may match.
    #[serde(default)]
    pub ignored: Vec<String>,
    #[serde(default)]
    pub size: SizeLimit,
    /// Torrents with fewer known seeders are rejected.
    #[serde(default)]
    pub min_seeders: u32,
}

impl Profile {
    /// A sensible default per kind: everything from DVD/MP3-192/PDF up, cutoff at 1080p WEB-DL / FLAC / EPUB / M4B.
    pub fn default_for(kind: MediaKind) -> Self {
        use Quality::*;
        let (allowed, cutoff): (Vec<Quality>, Quality) = match kind {
            MediaKind::Movie | MediaKind::Tv => (
                Quality::ALL
                    .iter()
                    .copied()
                    .filter(|q| q.is_video() && *q >= Sdtv && *q != BrDisk && *q != RawHd)
                    .collect(),
                WebDl1080p,
            ),
            MediaKind::Music => (
                Quality::ALL
                    .iter()
                    .copied()
                    .filter(|q| q.is_music() && *q >= Mp3_192)
                    .collect(),
                Flac,
            ),
            MediaKind::Book => (
                vec![Pdf, Mobi, Azw3, Epub, AudioMp3, AudioM4b, AudioFlac],
                Epub,
            ),
        };
        Self {
            name: "Default".into(),
            kind,
            allowed,
            cutoff,
            upgrade_allowed: true,
            preferred: Vec::new(),
            required: Vec::new(),
            ignored: Vec::new(),
            size: SizeLimit::default(),
            min_seeders: 1,
        }
    }
}

/// What the daemon knows beyond the release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    /// Runtime of the movie or episode (episodes of a pack: the sum), for per-minute size limits.
    pub runtime_minutes: Option<u32>,
    /// Quality and revision of the file already on disk, if any.
    pub existing: Option<(Quality, u8)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub accepted: bool,
    /// Rejection reasons when not accepted; otherwise notes (matched preferred terms).
    pub reasons: Vec<String>,
    /// Higher is better; see the module docs.
    pub rank: i64,
    pub quality: Quality,
    /// Sum of matched preferred-term scores.
    pub preferred_score: i32,
}

fn matches(pattern: &str, title: &str) -> bool {
    match pattern.strip_prefix('/').and_then(|p| p.strip_suffix('/')) {
        Some(re) => RegexBuilder::new(re)
            .case_insensitive(true)
            .size_limit(1 << 20)
            .build()
            .is_ok_and(|r| r.is_match(title)),
        None => title.to_lowercase().contains(&pattern.to_lowercase()),
    }
}

/// Quality and revision of a release name read the profile's way.
pub fn quality_of(title: &str, kind: MediaKind) -> (Quality, u8) {
    let video = |q: QualityInfo| (q.quality, q.version);
    match kind {
        MediaKind::Movie => video(parse_movie(title).quality),
        MediaKind::Tv => parse_episode(title)
            .map(|e| video(e.quality))
            .unwrap_or_else(|| video(crate::parse_quality(title))),
        MediaKind::Music => (parse_music(title).quality, 1),
        MediaKind::Book => (parse_book(title).quality, 1),
    }
}

/// [`score_with`] without runtime or existing file.
pub fn score(release: &Release, profile: &Profile) -> Decision {
    score_with(release, profile, &Context::default())
}

/// Decide whether `release` fits `profile` and rank it.
pub fn score_with(release: &Release, profile: &Profile, ctx: &Context) -> Decision {
    let (quality, version) = quality_of(&release.title, profile.kind);
    let mut reasons = Vec::new();
    let pos = profile.allowed.iter().position(|q| *q == quality);
    if pos.is_none() {
        reasons.push(format!("quality {quality} is not allowed"));
    }
    let title = &release.title;
    if !profile.required.is_empty() && !profile.required.iter().any(|t| matches(t, title)) {
        reasons.push("contains none of the required terms".into());
    }
    for t in profile.ignored.iter().filter(|t| matches(t, title)) {
        reasons.push(format!("contains ignored term \"{t}\""));
    }
    if let Some(size) = release.size {
        let gb = size as f64 / 1e9;
        let s = &profile.size;
        if s.min_gb.is_some_and(|m| gb < m) {
            reasons.push(format!("{gb:.2} GB is below the minimum"));
        }
        if s.max_gb.is_some_and(|m| gb > m) {
            reasons.push(format!("{gb:.2} GB is above the maximum"));
        }
        if let Some(min) = ctx.runtime_minutes.filter(|m| *m > 0) {
            let per = size as f64 / 1e6 / min as f64;
            if s.min_mb_per_minute.is_some_and(|m| per < m) {
                reasons.push(format!("{per:.1} MB per minute is below the minimum"));
            }
            if s.max_mb_per_minute.is_some_and(|m| per > m) {
                reasons.push(format!("{per:.1} MB per minute is above the maximum"));
            }
        }
    }
    if release.protocol == Protocol::Torrent
        && release.seeders.is_some_and(|s| s < profile.min_seeders)
    {
        reasons.push(format!(
            "{} seeders, fewer than {}",
            release.seeders.unwrap_or(0),
            profile.min_seeders
        ));
    }
    let rank_of = |q: Quality| profile.allowed.iter().position(|a| *a == q);
    if let Some((have, have_v)) = ctx.existing {
        let cutoff = rank_of(profile.cutoff);
        let have_pos = rank_of(have);
        if !profile.upgrade_allowed {
            reasons.push("a file exists and upgrades are off".into());
        } else if have_pos.is_some() && have_pos >= cutoff {
            reasons.push(format!("existing {have} already meets the cutoff"));
        } else if pos.is_some() && (pos < have_pos || (pos == have_pos && version <= have_v)) {
            reasons.push(format!("not better than the existing {have}"));
        }
    }
    let mut preferred_score = 0i32;
    let mut notes = Vec::new();
    for t in profile
        .preferred
        .iter()
        .filter(|t| matches(&t.pattern, title))
    {
        preferred_score = preferred_score.saturating_add(t.score);
        notes.push(format!("preferred \"{}\" {:+}", t.pattern, t.score));
    }
    let accepted = reasons.is_empty();
    let rank = pos.map_or(-1, |p| {
        p as i64 * 1_000_000_000_000
            + (preferred_score.clamp(-49_999, 49_999) as i64 + 50_000) * 10_000_000
            + version.min(9) as i64 * 1_000_000
            + release.seeders.unwrap_or(0).min(999_999) as i64
    });
    Decision {
        accepted,
        reasons: if accepted { notes } else { reasons },
        rank,
        quality,
        preferred_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(title: &str, seeders: u32, gb: f64) -> Release {
        Release {
            title: title.into(),
            seeders: Some(seeders),
            size: Some((gb * 1e9) as u64),
            ..Release::default()
        }
    }

    #[test]
    fn accepts_and_ranks_by_quality_then_terms_then_revision_then_seeders() {
        let mut p = Profile::default_for(MediaKind::Movie);
        p.preferred = vec![Term {
            pattern: "/\\bx265\\b/".into(),
            score: 10,
        }];
        let a = score(
            &rel("Some.Film.2019.1080p.WEB-DL.DDP5.1.H.264-GRP", 50, 6.0),
            &p,
        );
        let b = score(&rel("Some.Film.2019.1080p.BluRay.x264-GRP", 5, 10.0), &p);
        let c = score(&rel("Some.Film.2019.1080p.BluRay.x265-GRP", 1, 5.0), &p);
        let d = score(
            &rel("Some.Film.2019.1080p.BluRay.x265.PROPER-GRP", 1, 5.0),
            &p,
        );
        assert!(
            a.accepted && b.accepted && c.accepted && d.accepted,
            "{a:?}"
        );
        assert!(b.rank > a.rank);
        assert!(c.rank > b.rank);
        assert!(d.rank > c.rank);
        assert_eq!(c.preferred_score, 10);
        let e = score(&rel("Some.Film.2019.1080p.BluRay.x264-GRP", 6, 10.0), &p);
        assert!(e.rank > b.rank);
    }

    #[test]
    fn rejections() {
        let mut p = Profile::default_for(MediaKind::Movie);
        p.required = vec!["1080p".into()];
        p.ignored = vec!["hc".into()];
        p.size.max_gb = Some(20.0);
        p.size.max_mb_per_minute = Some(100.0);
        p.min_seeders = 3;
        let r = rel("Some.Film.2019.720p.HDTV.HC.x264-GRP", 1, 30.0);
        let d = score_with(
            &r,
            &p,
            &Context {
                runtime_minutes: Some(120),
                existing: None,
            },
        );
        assert!(!d.accepted);
        assert_eq!(d.reasons.len(), 5, "{:?}", d.reasons);
        let cam = score(&rel("Some.Film.2019.CAM.x264-GRP", 9, 1.0), &p);
        assert!(cam.reasons.iter().any(|r| r.contains("CAM is not allowed")));
        assert_eq!(cam.rank, -1);
        // Usenet results have no seeders.
        let mut u = rel("Some.Film.2019.1080p.WEB-DL.x264-GRP", 0, 5.0);
        u.protocol = Protocol::Usenet;
        u.seeders = None;
        assert!(score(&u, &p).accepted);
    }

    #[test]
    fn upgrades() {
        let p = Profile::default_for(MediaKind::Tv);
        let r = rel("Show.S01E01.1080p.WEB-DL.x264-GRP", 10, 2.0);
        let ctx = |q, v| Context {
            runtime_minutes: None,
            existing: Some((q, v)),
        };
        assert!(score_with(&r, &p, &ctx(Quality::Hdtv720p, 1)).accepted);
        assert!(!score_with(&r, &p, &ctx(Quality::WebDl1080p, 1)).accepted); // cutoff met
        let mut p2 = p.clone();
        p2.cutoff = Quality::Remux2160p;
        assert!(!score_with(&r, &p2, &ctx(Quality::Bluray1080p, 1)).accepted); // worse
        assert!(!score_with(&r, &p2, &ctx(Quality::WebDl1080p, 1)).accepted); // same
        let proper = rel("Show.S01E01.1080p.WEB-DL.x264.PROPER-GRP", 10, 2.0);
        assert!(score_with(&proper, &p2, &ctx(Quality::WebDl1080p, 1)).accepted);
        p2.upgrade_allowed = false;
        assert!(!score_with(&proper, &p2, &ctx(Quality::Hdtv720p, 1)).accepted);
    }

    #[test]
    fn music_and_books() {
        let m = Profile::default_for(MediaKind::Music);
        let flac = score(&rel("Artist - Album (2020) [FLAC]", 5, 0.4), &m);
        let mp3 = score(&rel("Artist - Album (2020) [MP3 320]", 50, 0.1), &m);
        assert!(flac.accepted && mp3.accepted && flac.rank > mp3.rank);
        let b = Profile::default_for(MediaKind::Book);
        let epub = score(&rel("Author Name - A Book (2019) [epub]", 5, 0.001), &b);
        assert_eq!(epub.quality, Quality::Epub);
        assert!(epub.accepted);
    }
}
