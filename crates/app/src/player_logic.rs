//! Pure player logic (no GPUI, no I/O): skip segments, next-episode countdown, trickplay math,
//! quality and play method, track selection, resume rule, subtitle styles, MPRIS mapping.
use crate::video::{PlayerPrefs, SubMode, Track, TrackKind, language};
use nocturne_jellyfin as jf;

pub const TICKS: f64 = 10_000_000.0;

// ---- skip segments ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    Intro,
    Credits,
    Recap,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub kind: SegKind,
    pub start: f64,
    pub end: f64,
}

impl Seg {
    pub fn label(&self) -> &'static str {
        match self.kind {
            SegKind::Intro => "Skip Intro",
            SegKind::Credits => "Skip Credits",
            SegKind::Recap => "Skip Recap",
            SegKind::Preview => "Skip Preview",
        }
    }
}

/// Skippable segments in seconds; kinds nobody skips (commercial, unknown) are dropped.
pub fn segments(raw: &[jf::Segment]) -> Vec<Seg> {
    let mut v: Vec<Seg> = raw
        .iter()
        .filter_map(|s| {
            let kind = match s.kind {
                jf::SegmentKind::Intro => SegKind::Intro,
                jf::SegmentKind::Outro => SegKind::Credits,
                jf::SegmentKind::Recap => SegKind::Recap,
                jf::SegmentKind::Preview => SegKind::Preview,
                _ => return None,
            };
            Some(Seg {
                kind,
                start: s.start_ticks as f64 / TICKS,
                end: s.end_ticks as f64 / TICKS,
            })
        })
        .collect();
    v.sort_by(|a, b| a.start.total_cmp(&b.start));
    v
}

/// The segment playing at `pos` (its last second is not offered: nothing left to skip).
pub fn active_seg(segs: &[Seg], pos: f64) -> Option<Seg> {
    segs.iter()
        .copied()
        .find(|s| pos >= s.start - 0.5 && pos < s.end - 1.0)
}

/// Auto-skip applies to intros and recaps only, never credits (the next-episode card owns those).
pub fn auto_skips(s: &Seg) -> bool {
    matches!(s.kind, SegKind::Intro | SegKind::Recap)
}

// ---- next episode ----

/// Seconds of countdown on the "Next episode" card.
pub const COUNTDOWN_S: f64 = 10.0;
/// With no credits segment, the card appears this long before the end.
pub const NEXT_BEFORE_END: f64 = 30.0;

/// Whether the next-episode card is due: credits started, or the end is near.
pub fn next_card_due(pos: f64, dur: Option<f64>, credits: Option<&Seg>) -> bool {
    let Some(d) = dur.filter(|d| *d > 0.0) else {
        return false;
    };
    let from = credits.map_or(d - NEXT_BEFORE_END, |c| c.start);
    pos >= from.min(d - 5.0).max(0.0)
}

/// Whole seconds left on the card `elapsed` seconds after it appeared; None once it ran out.
pub fn countdown_left(elapsed: f64) -> Option<u32> {
    let left = COUNTDOWN_S - elapsed;
    (left > 0.0).then(|| left.ceil() as u32)
}

// ---- trickplay ----

/// Where a scrub fraction lands in the tile sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub sheet: u32,
    /// Pixel offset of the thumbnail inside its sheet.
    pub x: u32,
    pub y: u32,
}

pub fn tile_at(info: &jf::TrickplayInfo, secs: f64) -> Option<Tile> {
    if info.thumbnail_count == 0
        || info.interval == 0
        || info.tile_width == 0
        || info.tile_height == 0
    {
        return None;
    }
    let idx = ((secs.max(0.0) * 1000.0) / info.interval as f64) as u32;
    let idx = idx.min(info.thumbnail_count - 1);
    let per = info.tile_width * info.tile_height;
    let n = idx % per;
    Some(Tile {
        sheet: idx / per,
        x: (n % info.tile_width) * info.width,
        y: (n / info.tile_width) * info.height,
    })
}

/// Pixel size of a tile sheet. Jellyfin pads every sheet to the full grid, last one included
/// (verified against 12.4: 10 thumbnails still arrive as a 10x10 grid).
pub fn sheet_dims(info: &jf::TrickplayInfo) -> (u32, u32) {
    (info.tile_width * info.width, info.tile_height * info.height)
}

// ---- quality and play method ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Original,
    P1080,
    P720,
    P480,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    DirectPlay,
    DirectStream,
    Transcode,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::DirectPlay => "Direct Play",
            Method::DirectStream => "Direct Stream",
            Method::Transcode => "Transcode",
        }
    }
}

impl Quality {
    pub const ALL: [Quality; 4] = [
        Quality::Original,
        Quality::P1080,
        Quality::P720,
        Quality::P480,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Original => "Original (direct)",
            Quality::P1080 => "1080p - 8 Mbps",
            Quality::P720 => "720p - 4 Mbps",
            Quality::P480 => "480p - 1.5 Mbps",
        }
    }

    /// `(max video bitrate bps, max height)`; Original has no caps.
    pub fn caps(self) -> Option<(u64, u32)> {
        match self {
            Quality::Original => None,
            Quality::P1080 => Some((8_000_000, 1080)),
            Quality::P720 => Some((4_000_000, 720)),
            Quality::P480 => Some((1_500_000, 480)),
        }
    }

    pub fn transcode(self, source_id: &str, play_session: &str) -> Option<jf::Transcode> {
        let (b, h) = self.caps()?;
        Some(jf::Transcode {
            source_id: source_id.into(),
            video_bitrate: Some(b),
            max_height: Some(h),
            audio_bitrate: Some(192_000),
            max_audio_channels: Some(2),
            allow_copy: true,
            play_session: play_session.into(),
        })
    }

    /// What the server will do: a cap the source already fits is only remuxed (Direct Stream).
    pub fn method(self, src_height: Option<u32>, src_bitrate: Option<u64>) -> Method {
        match self.caps() {
            None => Method::DirectPlay,
            Some((b, h)) => {
                let fits =
                    src_height.is_some_and(|sh| sh <= h) && src_bitrate.is_some_and(|sb| sb <= b);
                if fits {
                    Method::DirectStream
                } else {
                    Method::Transcode
                }
            }
        }
    }
}

// ---- tracks, prefs, memory ----

pub fn same_lang(a: &str, b: &str) -> bool {
    let (la, lb) = (language(a), language(b));
    la != "Unknown" && la.eq_ignore_ascii_case(&lb)
}

/// Remembered choice for a series/season within this session. Languages, not ids: ids differ per file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackChoice {
    pub audio_lang: Option<String>,
    /// `Some(None)` = subtitles off, `Some(Some(lang))` = that language, `None` = never chosen.
    pub sub: Option<Option<String>>,
}

fn of_kind(tracks: &[Track], k: TrackKind) -> impl Iterator<Item = &Track> {
    tracks.iter().filter(move |t| t.kind == k)
}

/// Audio track to select: remembered language, else preferred language, else None (mpv default).
pub fn pick_audio(tracks: &[Track], prefs: &PlayerPrefs, mem: &TrackChoice) -> Option<i64> {
    let want = mem.audio_lang.as_deref().or(prefs.audio_lang.as_deref())?;
    of_kind(tracks, TrackKind::Audio)
        .find(|t| same_lang(&t.lang, want))
        .map(|t| t.id)
}

/// Subtitle id to select (0 = off). `audio` is the language of the audio that will play.
/// Memory wins; then `sub_mode`: Off, ForeignOnly (only when the audio is not in the subtitle
/// language), Always. Full tracks beat forced ones except in ForeignOnly where forced is the point.
pub fn pick_sub(tracks: &[Track], audio: &str, prefs: &PlayerPrefs, mem: &TrackChoice) -> i64 {
    let subs = || of_kind(tracks, TrackKind::Sub);
    let in_lang = |l: &str, forced: bool| {
        subs()
            .filter(|t| same_lang(&t.lang, l) && t.forced == forced)
            .map(|t| t.id)
            .next()
    };
    if let Some(choice) = &mem.sub {
        return match choice {
            None => 0,
            Some(l) => in_lang(l, false).or(in_lang(l, true)).unwrap_or(0),
        };
    }
    let Some(l) = prefs.sub_lang.as_deref() else {
        return match prefs.sub_mode {
            SubMode::Off => 0,
            _ => subs().find(|t| t.forced).map_or(0, |t| t.id),
        };
    };
    match prefs.sub_mode {
        SubMode::Off => 0,
        SubMode::Always => in_lang(l, false).or(in_lang(l, true)).unwrap_or(0),
        SubMode::ForeignOnly => {
            if same_lang(audio, l) {
                in_lang(l, true).unwrap_or(0) // audio already in their language: forced signs only
            } else {
                in_lang(l, false).or(in_lang(l, true)).unwrap_or(0)
            }
        }
    }
}

/// Friendly names: Jellyfin names streams ("English - Dolby Digital - 5.1 - Default"); mpv lists
/// them in the same order per kind, so zip them when the counts agree.
pub fn name_tracks(tracks: &mut [Track], streams: &[jf::MediaStream]) {
    for (kind, jk) in [(TrackKind::Audio, "Audio"), (TrackKind::Sub, "Subtitle")] {
        let names: Vec<&str> = streams
            .iter()
            .filter(|s| s.kind == jk && !s.is_external)
            .filter_map(|s| s.display_title.as_deref())
            .collect();
        let idx: Vec<usize> = (0..tracks.len())
            .filter(|i| tracks[*i].kind == kind && !tracks[*i].external)
            .collect();
        if idx.len() == names.len() {
            for (i, n) in idx.into_iter().zip(names) {
                tracks[i].name = Some(n.to_string());
            }
        }
    }
}

// ---- resume ----

/// Position to offer "Resume from" for: a real saved spot (past 20 s, not in the last minute),
/// on an item not already watched.
pub fn resume_offer(resume: Option<f64>, dur: Option<f64>, played: bool) -> Option<f64> {
    let r = resume.filter(|r| *r >= 20.0 && !played)?;
    match dur {
        Some(d) if r > d - 60.0 => None,
        _ => Some(r),
    }
}

// ---- subtitle style ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubStyle {
    pub name: &'static str,
    pub size: u32,
    pub color: &'static str,
    pub outline: f64,
    /// Background box colour (`#AARRGGBB`); fully transparent = none.
    pub back: &'static str,
}

pub const SUB_STYLES: [SubStyle; 5] = [
    SubStyle {
        name: "Default",
        size: 55,
        color: "#FFFFFFFF",
        outline: 1.5,
        back: "#00000000",
    },
    SubStyle {
        name: "Large",
        size: 70,
        color: "#FFFFFFFF",
        outline: 2.0,
        back: "#00000000",
    },
    SubStyle {
        name: "Yellow",
        size: 55,
        color: "#FFFFEB3B",
        outline: 2.0,
        back: "#00000000",
    },
    SubStyle {
        name: "Boxed",
        size: 55,
        color: "#FFFFFFFF",
        outline: 0.0,
        back: "#B3000000",
    },
    SubStyle {
        name: "Outline only",
        size: 55,
        color: "#FFFFFFFF",
        outline: 3.0,
        back: "#00000000",
    },
];

impl SubStyle {
    /// mpv properties realising the style. Forced over ASS styling so presets visibly apply.
    pub fn props(&self) -> Vec<(&'static str, String)> {
        vec![
            ("sub-font-size", self.size.to_string()),
            ("sub-color", self.color.into()),
            ("sub-border-size", self.outline.to_string()),
            ("sub-back-color", self.back.into()),
            (
                "sub-border-style",
                if self.back == "#00000000" {
                    "outline-and-shadow"
                } else {
                    "background-box"
                }
                .into(),
            ),
        ]
    }
}

/// `sub-pos` (0 top .. 100 bottom) that keeps subtitles clear of a controls bar `bar_px` tall
/// over a `video_h` tall video; never lower than the user's `base`.
pub fn sub_pos(base: i64, bar_px: f32, video_h: f32, bar_visible: bool) -> i64 {
    if !bar_visible || video_h <= 0.0 {
        return base;
    }
    let lift = (bar_px / video_h * 100.0).ceil() as i64 + 1;
    base.min(100 - lift).max(0)
}

// ---- MPRIS ----

pub fn mpris_status(idle: bool, paused: bool) -> &'static str {
    match (idle, paused) {
        (true, _) => "Stopped",
        (false, true) => "Paused",
        (false, false) => "Playing",
    }
}

pub fn micros(secs: f64) -> i64 {
    (secs * 1_000_000.0).round() as i64
}

/// MPRIS `Seek(offset)` is relative microseconds; `SetPosition` absolute microseconds.
pub fn from_micros(us: i64) -> f64 {
    us as f64 / 1_000_000.0
}

/// Where Next/Previous lead in a queue of `len`, with repeat-all wrapping. None = nothing there.
pub fn step_queue(cur: usize, len: usize, dir: i8, wrap: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let n = cur as i64 + dir as i64;
    if (0..len as i64).contains(&n) {
        Some(n as usize)
    } else if wrap && len > 1 {
        Some(n.rem_euclid(len as i64) as usize)
    } else {
        None
    }
}

/// Move queue entry `i` one place up or down. `cur` (the playing entry) keeps following the same
/// track. Returns the entry's new index, or None at the edge.
pub fn queue_move<T>(q: &mut [T], cur: &mut usize, i: usize, up: bool) -> Option<usize> {
    let j = if up { i.checked_sub(1)? } else { i + 1 };
    if i >= q.len() || j >= q.len() {
        return None;
    }
    q.swap(i, j);
    if *cur == i {
        *cur = j;
    } else if *cur == j {
        *cur = i;
    }
    Some(j)
}

/// Remove entry `i`. The playing entry cannot be removed (it would stop the sound under the
/// user); returns whether something was removed.
pub fn queue_remove<T>(q: &mut Vec<T>, cur: &mut usize, i: usize) -> bool {
    if i >= q.len() || i == *cur {
        return false;
    }
    q.remove(i);
    if i < *cur {
        *cur -= 1;
    }
    true
}

/// Put `items` right after the playing entry (`next`) or at the end of the queue.
pub fn queue_insert<T>(q: &mut Vec<T>, cur: usize, items: Vec<T>, next: bool) {
    let at = if next {
        (cur + 1).min(q.len())
    } else {
        q.len()
    };
    q.splice(at..at, items);
}

/// Shuffle everything after the playing entry (Fisher-Yates); what already played and the
/// playing entry stay put. `rnd(n)` returns a number below `n`.
pub fn shuffle_after<T>(q: &mut [T], cur: usize, rnd: &mut impl FnMut(usize) -> usize) {
    let start = (cur + 1).min(q.len());
    let tail = &mut q[start..];
    for i in (1..tail.len()).rev() {
        tail.swap(i, rnd(i + 1));
    }
}

/// Undo a shuffle: the original order restricted to what is still queued, then anything added
/// since. Returns the queue and where the playing entry (`playing` id) ended up.
pub fn unshuffle<T: Clone>(
    q: &[T],
    original: &[T],
    id: impl Fn(&T) -> &str,
    playing: &str,
) -> (Vec<T>, usize) {
    let queued = |x: &&T| q.iter().any(|y| id(y) == id(x));
    let mut out: Vec<T> = original.iter().filter(queued).cloned().collect();
    let added: Vec<T> = q
        .iter()
        .filter(|x| !out.iter().any(|y| id(y) == id(x)))
        .cloned()
        .collect();
    out.extend(added);
    let at = out.iter().position(|x| id(x) == playing).unwrap_or(0);
    (out, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_edits_keep_the_playing_track() {
        let mut q = vec!["a", "b", "c", "d"];
        let mut cur = 1; // b is playing
        assert_eq!(queue_move(&mut q, &mut cur, 2, true), Some(1));
        assert_eq!(
            (q.clone(), cur),
            (vec!["a", "c", "b", "d"], 2),
            "b is still current"
        );
        assert_eq!(queue_move(&mut q, &mut cur, 0, true), None);
        assert_eq!(queue_move(&mut q, &mut cur, 3, false), None);
        assert!(
            !queue_remove(&mut q, &mut cur, 2),
            "the playing track stays"
        );
        assert!(queue_remove(&mut q, &mut cur, 0));
        assert_eq!((q.clone(), cur), (vec!["c", "b", "d"], 1));
        queue_insert(&mut q, cur, vec!["x", "y"], true);
        assert_eq!(q, ["c", "b", "x", "y", "d"]);
        queue_insert(&mut q, cur, vec!["z"], false);
        assert_eq!(q.last(), Some(&"z"));
    }

    #[test]
    fn shuffle_touches_only_what_comes_next_and_can_be_undone() {
        let original: Vec<String> = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
        let mut q = original.clone();
        let mut n = 0;
        shuffle_after(&mut q, 1, &mut |m| {
            n += 1;
            n % m
        });
        assert_eq!(&q[..2], ["a", "b"], "played and playing entries stay");
        let mut sorted = q.clone();
        sorted.sort();
        assert_eq!(sorted, original, "nothing lost");
        assert_ne!(q, original);
        q.push("f".into()); // added while shuffled
        let (back, at) = unshuffle(&q, &original, |s| s.as_str(), "b");
        assert_eq!(back, ["a", "b", "c", "d", "e", "f"]);
        assert_eq!(at, 1);
        q.retain(|x| x != "d");
        assert!(
            !unshuffle(&q, &original, |s| s.as_str(), "b")
                .0
                .contains(&"d".to_string())
        );
    }

    fn seg(kind: SegKind, s: f64, e: f64) -> Seg {
        Seg {
            kind,
            start: s,
            end: e,
        }
    }

    #[test]
    fn segment_windows() {
        let raw = [
            jf::Segment {
                kind: jf::SegmentKind::Outro,
                start_ticks: 1200 * 10_000_000,
                end_ticks: 1300 * 10_000_000,
            },
            jf::Segment {
                kind: jf::SegmentKind::Intro,
                start_ticks: 30 * 10_000_000,
                end_ticks: 90 * 10_000_000,
            },
            jf::Segment {
                kind: jf::SegmentKind::Commercial,
                start_ticks: 1,
                end_ticks: 2,
            },
        ];
        let s = segments(&raw);
        assert_eq!(s.len(), 2, "commercials are not skippable");
        assert_eq!(s[0].kind, SegKind::Intro, "sorted by start");
        assert_eq!((s[0].start, s[0].end), (30.0, 90.0));
        assert!(active_seg(&s, 10.0).is_none());
        assert_eq!(active_seg(&s, 29.6).unwrap().label(), "Skip Intro");
        assert_eq!(active_seg(&s, 60.0).unwrap().kind, SegKind::Intro);
        assert!(active_seg(&s, 89.5).is_none(), "last second not offered");
        assert_eq!(active_seg(&s, 1250.0).unwrap().label(), "Skip Credits");
        assert!(
            auto_skips(&s[0]) && !auto_skips(&s[1]),
            "credits are never auto-skipped"
        );
    }

    #[test]
    fn next_card_and_countdown() {
        assert!(!next_card_due(100.0, Some(1500.0), None));
        assert!(
            next_card_due(1470.0, Some(1500.0), None),
            "30 s before the end"
        );
        let c = seg(SegKind::Credits, 1400.0, 1500.0);
        assert!(!next_card_due(1399.0, Some(1500.0), Some(&c)));
        assert!(
            next_card_due(1400.0, Some(1500.0), Some(&c)),
            "credits start"
        );
        assert!(!next_card_due(5.0, None, None), "unknown duration never");
        assert!(
            next_card_due(18.0, Some(20.0), None),
            "short clips: last 5 s at least"
        );
        assert_eq!(countdown_left(0.0), Some(10));
        assert_eq!(countdown_left(0.2), Some(10));
        assert_eq!(countdown_left(9.5), Some(1));
        assert_eq!(countdown_left(10.0), None);
    }

    #[test]
    fn trickplay_tiles() {
        let i = jf::TrickplayInfo {
            width: 320,
            height: 180,
            tile_width: 10,
            tile_height: 10,
            thumbnail_count: 250,
            interval: 10_000,
        };
        assert_eq!(
            tile_at(&i, 0.0),
            Some(Tile {
                sheet: 0,
                x: 0,
                y: 0
            })
        );
        assert_eq!(
            tile_at(&i, 9.9),
            Some(Tile {
                sheet: 0,
                x: 0,
                y: 0
            })
        );
        assert_eq!(
            tile_at(&i, 10.0),
            Some(Tile {
                sheet: 0,
                x: 320,
                y: 0
            })
        );
        assert_eq!(
            tile_at(&i, 100.0),
            Some(Tile {
                sheet: 0,
                x: 0,
                y: 180
            })
        );
        assert_eq!(
            tile_at(&i, 1000.0),
            Some(Tile {
                sheet: 1,
                x: 0,
                y: 0
            })
        );
        assert_eq!(
            tile_at(&i, 99999.0),
            Some(Tile {
                sheet: 2,
                x: 320 * 9,
                y: 180 * 4
            }),
            "clamped to the last thumbnail (249)"
        );
        assert_eq!(
            tile_at(&i, -5.0),
            Some(Tile {
                sheet: 0,
                x: 0,
                y: 0
            })
        );
        assert!(tile_at(&Default::default(), 5.0).is_none());
        assert_eq!(sheet_dims(&i), (3200, 1800));
    }

    #[test]
    fn quality_and_method() {
        assert!(Quality::Original.transcode("s", "p").is_none());
        let t = Quality::P720.transcode("src", "ps").unwrap();
        assert_eq!(
            (t.video_bitrate, t.max_height),
            (Some(4_000_000), Some(720))
        );
        assert_eq!(t.source_id, "src");
        assert_eq!(
            Quality::Original.method(Some(1080), Some(9_000_000)),
            Method::DirectPlay
        );
        assert_eq!(
            Quality::P1080.method(Some(720), Some(3_000_000)),
            Method::DirectStream
        );
        assert_eq!(
            Quality::P720.method(Some(1080), Some(3_000_000)),
            Method::Transcode
        );
        assert_eq!(Quality::P480.method(None, None), Method::Transcode);
        assert_eq!(Method::DirectStream.label(), "Direct Stream");
    }

    fn tr(id: i64, kind: TrackKind, lang: &str, forced: bool) -> Track {
        Track {
            id,
            kind,
            lang: lang.into(),
            title: String::new(),
            codec: String::new(),
            channels: None,
            default: false,
            forced,
            external: false,
            selected: false,
            name: None,
        }
    }

    fn tracks() -> Vec<Track> {
        vec![
            tr(1, TrackKind::Audio, "jpn", false),
            tr(2, TrackKind::Audio, "eng", false),
            tr(1, TrackKind::Sub, "eng", false),
            tr(2, TrackKind::Sub, "eng", true),
            tr(3, TrackKind::Sub, "spa", false),
        ]
    }

    #[test]
    fn track_selection_rules() {
        let t = tracks();
        let none = TrackChoice::default();
        let mut p = PlayerPrefs {
            audio_lang: Some("en".into()),
            sub_lang: Some("eng".into()),
            sub_mode: SubMode::Off,
            ..Default::default()
        };
        assert_eq!(pick_audio(&t, &p, &none), Some(2), "en matches eng");
        assert_eq!(
            pick_audio(&t, &PlayerPrefs::default(), &none),
            None,
            "no preference keeps mpv's default"
        );
        assert_eq!(pick_sub(&t, "eng", &p, &none), 0, "Off");
        p.sub_mode = SubMode::Always;
        assert_eq!(
            pick_sub(&t, "jpn", &p, &none),
            1,
            "Always prefers the full track"
        );
        assert_eq!(pick_sub(&t, "eng", &p, &none), 1);
        p.sub_mode = SubMode::ForeignOnly;
        assert_eq!(
            pick_sub(&t, "jpn", &p, &none),
            1,
            "foreign audio: full subs"
        );
        assert_eq!(
            pick_sub(&t, "eng", &p, &none),
            2,
            "own language: forced only"
        );
        p.sub_lang = Some("fra".into());
        assert_eq!(pick_sub(&t, "jpn", &p, &none), 0, "no such language");
        // memory beats prefs, by language
        let mem = TrackChoice {
            audio_lang: Some("jpn".into()),
            sub: Some(Some("spa".into())),
        };
        assert_eq!(pick_audio(&t, &p, &mem), Some(1));
        assert_eq!(pick_sub(&t, "jpn", &p, &mem), 3);
        let off = TrackChoice {
            audio_lang: None,
            sub: Some(None),
        };
        p.sub_mode = SubMode::Always;
        assert_eq!(pick_sub(&t, "jpn", &p, &off), 0, "remembered Off sticks");
    }

    #[test]
    fn friendly_names_only_when_counts_agree() {
        let mut t = tracks();
        let st = |k: &str, n: &str| jf::MediaStream {
            kind: k.into(),
            display_title: Some(n.into()),
            ..Default::default()
        };
        let s = [
            st("Video", "x"),
            st("Audio", "Japanese - Stereo"),
            st("Audio", "English - 5.1"),
            st("Subtitle", "a"),
            st("Subtitle", "b"),
        ];
        name_tracks(&mut t, &s);
        assert_eq!(t[0].label(), "Japanese - Stereo");
        assert_eq!(t[1].name.as_deref(), Some("English - 5.1"));
        assert!(
            t[2].name.is_none(),
            "3 mpv subs vs 2 jellyfin subs: no guessing"
        );
    }

    #[test]
    fn resume_rule() {
        assert_eq!(resume_offer(Some(723.0), Some(5400.0), false), Some(723.0));
        assert_eq!(
            resume_offer(Some(10.0), Some(5400.0), false),
            None,
            "too early"
        );
        assert_eq!(
            resume_offer(Some(5380.0), Some(5400.0), false),
            None,
            "credits"
        );
        assert_eq!(
            resume_offer(Some(600.0), Some(5400.0), true),
            None,
            "already played"
        );
        assert_eq!(resume_offer(None, None, false), None);
        assert_eq!(resume_offer(Some(600.0), None, false), Some(600.0));
    }

    #[test]
    fn subtitle_styles_and_position() {
        let p = SUB_STYLES[3].props();
        assert!(p.contains(&("sub-border-style", "background-box".into())));
        assert!(
            SUB_STYLES[0]
                .props()
                .contains(&("sub-border-style", "outline-and-shadow".into()))
        );
        assert_eq!(sub_pos(100, 96.0, 800.0, false), 100);
        assert_eq!(sub_pos(100, 96.0, 800.0, true), 87);
        assert_eq!(sub_pos(60, 96.0, 800.0, true), 60, "already above the bar");
        assert_eq!(sub_pos(100, 96.0, 0.0, true), 100);
    }

    #[test]
    fn mpris_mapping_and_queue_steps() {
        assert_eq!(mpris_status(true, false), "Stopped");
        assert_eq!(mpris_status(false, true), "Paused");
        assert_eq!(mpris_status(false, false), "Playing");
        assert_eq!(micros(1.5), 1_500_000);
        assert_eq!(from_micros(-2_000_000), -2.0);
        assert_eq!(step_queue(0, 3, 1, false), Some(1));
        assert_eq!(step_queue(2, 3, 1, false), None);
        assert_eq!(step_queue(2, 3, 1, true), Some(0), "repeat-all wraps");
        assert_eq!(step_queue(0, 3, -1, true), Some(2));
        assert_eq!(
            step_queue(0, 1, 1, true),
            None,
            "single item never wraps onto itself"
        );
        assert_eq!(step_queue(0, 0, 1, true), None);
    }
}
