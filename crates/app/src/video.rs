//! Playback seam (embedded libmpv vs child-process mpv) and the pure logic of the player page:
//! backend choice, control auto-hide, key map. No GPUI, so it is unit-tested without a window.
use nocturne_player::{EmbeddedPlayer, Event, Player, Result, State};
use serde_json::Value;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// How long the controls stay up without mouse movement or keys.
pub const HIDE_AFTER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Embedded,
    External,
}

/// `NOCTURNE_VIDEO=external|embedded` forces a backend; otherwise embed whenever we are on Wayland.
/// (Later failures - no viewporter, EGL - still fall back to external at creation time.)
pub fn choose(env: Option<&str>, wayland: bool) -> Choice {
    match env {
        Some("external") => Choice::External,
        Some("embedded") => Choice::Embedded,
        _ if wayland => Choice::Embedded,
        _ => Choice::External,
    }
}

/// Controls visibility: shown on activity, hidden `HIDE_AFTER` later unless something holds them.
pub struct AutoHide {
    last: Instant,
}

impl AutoHide {
    pub fn new(now: Instant) -> Self {
        Self { last: now }
    }
    pub fn poke(&mut self, now: Instant) {
        self.last = now;
    }
    /// `hold`: pointer over the controls or a drag in progress.
    pub fn visible(&self, now: Instant, hold: bool) -> bool {
        hold || now.saturating_duration_since(self.last) < HIDE_AFTER
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    Toggle,
    Seek(f64),
    Volume(f64),
    Mute,
    Stop,
    Fullscreen,
    /// Esc: close a menu, else leave fullscreen, else close playback.
    Back,
    Next,
    /// -1 slower, +1 faster, 0 reset.
    Speed(i8),
    Frame(i8),
    Chapter(i8),
    SubDelay(f64),
    AudioDelay(f64),
    CycleSubs,
    Stats,
    Help,
    Screenshot,
    Loop,
}

/// Player-page key map. `k` is GPUI's key name; `shift`/`ctrl` are the held modifiers.
pub fn key(k: &str, shift: bool, ctrl: bool) -> Option<Key> {
    Some(match (k, ctrl) {
        ("s", true) => Key::Screenshot,
        (_, true) => return None,
        ("space", _) => Key::Toggle,
        ("left", _) => Key::Seek(-10.0),
        ("right", _) => Key::Seek(10.0),
        ("up", _) => Key::Volume(5.0),
        ("down", _) => Key::Volume(-5.0),
        ("m", _) => Key::Mute,
        ("s", _) => Key::Stop,
        ("f", _) => Key::Fullscreen,
        ("escape", _) => Key::Back,
        ("n", _) => Key::Next,
        ("[", _) => Key::Speed(-1),
        ("]", _) => Key::Speed(1),
        ("\\", _) => Key::Speed(0),
        (".", _) => Key::Frame(1),
        (",", _) => Key::Frame(-1),
        ("pageup", _) => Key::Chapter(-1),
        ("pagedown", _) => Key::Chapter(1),
        ("z", _) => Key::SubDelay(-0.1),
        ("x", _) => Key::SubDelay(0.1),
        ("-", _) => Key::AudioDelay(-0.1),
        ("=", _) => Key::AudioDelay(0.1),
        ("c", _) => Key::CycleSubs,
        ("i", _) => Key::Stats,
        ("l", _) => Key::Loop,
        ("?", _) => Key::Help,
        ("/", _) if shift => Key::Help,
        _ => return None,
    })
}

/// mpv's volume range is 0..=130.
pub fn bump_volume(cur: f64, d: f64) -> f64 {
    (cur + d).clamp(0.0, 130.0)
}

pub enum Playback {
    Embedded(Box<EmbeddedPlayer>),
    External(Player),
}

macro_rules! both {
    ($s:expr, $p:ident => $e:expr) => {
        match $s {
            Playback::Embedded($p) => $e,
            Playback::External($p) => $e,
        }
    };
}

impl Playback {
    pub fn load(&self, url: &str, headers: Option<&str>) -> Result<()> {
        both!(self, p => p.load(url, headers))
    }
    pub fn toggle(&self) -> Result<()> {
        both!(self, p => p.toggle())
    }
    pub fn seek_to(&self, s: f64) -> Result<()> {
        both!(self, p => p.seek_to(s))
    }
    pub fn seek_by(&self, s: f64) -> Result<()> {
        both!(self, p => p.seek_by(s))
    }
    pub fn set_volume(&self, v: f64) -> Result<()> {
        both!(self, p => p.set_volume(v))
    }
    pub fn state(&self) -> State {
        both!(self, p => p.state())
    }
    pub fn events(&self) -> &Receiver<Event> {
        both!(self, p => p.events())
    }
    pub fn command(&self, args: &[&str]) -> Result<()> {
        both!(self, p => p.command(args))
    }
    pub fn prop(&self, name: &str) -> Option<Value> {
        both!(self, p => p.prop(name))
    }
    /// mpv `set`: the value is parsed like on the command line, whatever the property type.
    pub fn set(&self, name: &str, val: &str) -> Result<()> {
        self.command(&["set", name, val])
    }
}

// ---- tracks, chapters, speed, aspect, menus: pure helpers over mpv property values ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Sub,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub lang: String,
    pub title: String,
    pub codec: String,
    pub channels: Option<i64>,
    pub default: bool,
    pub forced: bool,
    pub external: bool,
    pub selected: bool,
}

/// Audio/subtitle entries of mpv's `track-list`.
pub fn parse_tracks(v: &Value) -> Vec<Track> {
    let Some(a) = v.as_array() else {
        return vec![];
    };
    a.iter()
        .filter_map(|t| {
            let kind = match t["type"].as_str()? {
                "audio" => TrackKind::Audio,
                "sub" => TrackKind::Sub,
                _ => return None,
            };
            let s = |k: &str| t[k].as_str().unwrap_or("").to_string();
            let b = |k: &str| t[k].as_bool().unwrap_or(false);
            Some(Track {
                id: t["id"].as_i64()?,
                kind,
                lang: s("lang"),
                title: s("title"),
                codec: s("codec"),
                channels: t["demux-channel-count"].as_i64(),
                default: b("default"),
                forced: b("forced"),
                external: b("external"),
                selected: b("selected"),
            })
        })
        .collect()
}

/// ISO 639 code to a name for the common cases; anything else is shown as given.
fn language(code: &str) -> String {
    match code {
        "" | "und" => "Unknown".into(),
        "eng" | "en" => "English".into(),
        "jpn" | "ja" => "Japanese".into(),
        "spa" | "es" => "Spanish".into(),
        "fra" | "fre" | "fr" => "French".into(),
        "deu" | "ger" | "de" => "German".into(),
        "ita" | "it" => "Italian".into(),
        "por" | "pt" => "Portuguese".into(),
        "rus" | "ru" => "Russian".into(),
        "kor" | "ko" => "Korean".into(),
        "zho" | "chi" | "zh" => "Chinese".into(),
        "hin" | "hi" => "Hindi".into(),
        c => c.to_string(),
    }
}

fn channel_layout(n: i64) -> String {
    match n {
        1 => "Mono".into(),
        2 => "Stereo".into(),
        6 => "5.1".into(),
        8 => "7.1".into(),
        n => format!("{n} ch"),
    }
}

impl Track {
    /// Audio: `Japanese - EAC3 - 5.1 - Default`; subtitles: `English - Forced - SUBRIP`.
    pub fn label(&self) -> String {
        let mut p = vec![language(&self.lang)];
        if !self.title.is_empty() {
            p.push(self.title.clone());
        }
        if self.kind == TrackKind::Sub && self.forced {
            p.push("Forced".into());
        }
        if self.external {
            p.push("External".into());
        }
        if !self.codec.is_empty() {
            p.push(self.codec.to_uppercase());
        }
        if let (TrackKind::Audio, Some(c)) = (self.kind, self.channels) {
            p.push(channel_layout(c));
        }
        if self.default {
            p.push("Default".into());
        }
        p.join(" - ")
    }
}

/// `(start seconds, title)` per chapter from mpv's `chapter-list`.
pub fn parse_chapters(v: &Value) -> Vec<(f64, String)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    (
                        c["time"].as_f64().unwrap_or(0.0),
                        c["title"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Index of the chapter containing `t`.
pub fn chapter_at(ch: &[(f64, String)], t: f64) -> Option<usize> {
    ch.iter().rposition(|(s, _)| *s <= t)
}

/// Start of the previous (-1) / next (+1) chapter relative to `t`. Previous restarts the current
/// chapter when more than 3 s into it, like players do.
pub fn chapter_target(ch: &[(f64, String)], t: f64, dir: i8) -> Option<f64> {
    let cur = chapter_at(ch, t);
    if dir > 0 {
        ch.iter().map(|c| c.0).find(|s| *s > t + 0.001)
    } else {
        let cur = cur?;
        if t - ch[cur].0 > 3.0 || cur == 0 {
            Some(ch[cur].0)
        } else {
            Some(ch[cur - 1].0)
        }
    }
}

pub const SPEEDS: [f64; 9] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

/// Next stop on the speed ladder (`dir` -1/+1); 0 resets to 1x.
pub fn step_speed(cur: f64, dir: i8) -> f64 {
    if dir == 0 {
        return 1.0;
    }
    let eps = 1e-6;
    if dir > 0 {
        SPEEDS
            .iter()
            .copied()
            .find(|s| *s > cur + eps)
            .unwrap_or(4.0)
    } else {
        SPEEDS
            .iter()
            .rev()
            .copied()
            .find(|s| *s < cur - eps)
            .unwrap_or(0.25)
    }
}

pub fn fmt_speed(s: f64) -> String {
    format!("{}x", (s * 100.0).round() / 100.0)
}

/// `+0.1 s` style value for delay OSDs (milliseconds, signed).
pub fn fmt_delay(secs: f64) -> String {
    format!("{:+} ms", (secs * 1000.0).round() as i64)
}

/// Wall-clock end time for `remaining` media seconds at `speed`, given the local time of day.
pub fn ends_at(now_hm: (u32, u32), remaining: f64, speed: f64) -> String {
    let add = (remaining / speed.max(0.01) / 60.0).round() as u32;
    let m = (now_hm.0 * 60 + now_hm.1 + add) % (24 * 60);
    format!("Ends at {}:{:02}", m / 60, m % 60)
}

/// Local (hour, minute) now.
pub fn local_hm() -> (u32, u32) {
    // SAFETY: plain libc time calls with a zeroed out-struct.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_hour as u32, tm.tm_min as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    Auto,
    R16x9,
    R4x3,
    R21x9,
    /// Crop to fill the window.
    Fill,
    Stretch,
}

impl Aspect {
    pub const ALL: [Aspect; 6] = [
        Aspect::Auto,
        Aspect::R16x9,
        Aspect::R4x3,
        Aspect::R21x9,
        Aspect::Fill,
        Aspect::Stretch,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Aspect::Auto => "Auto",
            Aspect::R16x9 => "16:9",
            Aspect::R4x3 => "4:3",
            Aspect::R21x9 => "21:9",
            Aspect::Fill => "Fill (crop)",
            Aspect::Stretch => "Stretch",
        }
    }
    /// mpv `(property, value)` pairs that realise the preset from any other preset.
    pub fn props(self) -> [(&'static str, &'static str); 3] {
        let (ar, pan, keep) = match self {
            Aspect::Auto => ("-1", "0", "yes"),
            Aspect::R16x9 => ("16:9", "0", "yes"),
            Aspect::R4x3 => ("4:3", "0", "yes"),
            Aspect::R21x9 => ("21:9", "0", "yes"),
            Aspect::Fill => ("-1", "1", "yes"),
            Aspect::Stretch => ("-1", "0", "no"),
        };
        [
            ("video-aspect-override", ar),
            ("panscan", pan),
            ("keepaspect", keep),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pop {
    Subs,
    Audio,
    Settings,
    Speed,
    Aspect,
    Repeat,
}

impl Pop {
    pub fn title(self) -> &'static str {
        match self {
            Pop::Subs => "Subtitles",
            Pop::Audio => "Audio",
            Pop::Settings => "Settings",
            Pop::Speed => "Playback Speed",
            Pop::Aspect => "Aspect Ratio",
            Pop::Repeat => "Repeat Mode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuAct {
    /// Track id; 0 = off.
    Sub(i64),
    Audio(i64),
    Speed(f64),
    Aspect(Aspect),
    Repeat(bool),
    Open(Pop),
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// Right-aligned muted value (settings list).
    pub value: String,
    pub on: bool,
    pub act: MenuAct,
}

fn item(label: impl Into<String>, on: bool, act: MenuAct) -> Item {
    Item {
        label: label.into(),
        value: String::new(),
        on,
        act,
    }
}

/// Rows of a menu. `speed`/`aspect`/`repeat` are the current settings.
pub fn items(pop: Pop, tracks: &[Track], speed: f64, aspect: Aspect, repeat: bool) -> Vec<Item> {
    match pop {
        Pop::Subs => {
            let subs: Vec<_> = tracks.iter().filter(|t| t.kind == TrackKind::Sub).collect();
            let mut v = vec![item(
                "Off",
                subs.iter().all(|t| !t.selected),
                MenuAct::Sub(0),
            )];
            v.extend(
                subs.iter()
                    .map(|t| item(t.label(), t.selected, MenuAct::Sub(t.id))),
            );
            v
        }
        Pop::Audio => tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| item(t.label(), t.selected, MenuAct::Audio(t.id)))
            .collect(),
        Pop::Speed => SPEEDS
            .iter()
            .map(|s| item(fmt_speed(*s), (*s - speed).abs() < 1e-6, MenuAct::Speed(*s)))
            .collect(),
        Pop::Aspect => Aspect::ALL
            .iter()
            .map(|a| item(a.label(), *a == aspect, MenuAct::Aspect(*a)))
            .collect(),
        Pop::Repeat => vec![
            item("None", !repeat, MenuAct::Repeat(false)),
            item("Repeat One", repeat, MenuAct::Repeat(true)),
        ],
        Pop::Settings => {
            let row = |l: &str, v: String, a| Item {
                label: l.into(),
                value: v,
                on: false,
                act: a,
            };
            vec![
                row(
                    "Aspect Ratio",
                    aspect.label().into(),
                    MenuAct::Open(Pop::Aspect),
                ),
                row(
                    "Playback Speed",
                    fmt_speed(speed),
                    MenuAct::Open(Pop::Speed),
                ),
                row(
                    "Repeat Mode",
                    if repeat { "One" } else { "None" }.into(),
                    MenuAct::Open(Pop::Repeat),
                ),
                row("Playback Info", String::new(), MenuAct::Info),
            ]
        }
    }
}

/// Next subtitle track id in the cycle off -> first -> ... -> last -> off. `cur` 0 = off.
pub fn next_sub(tracks: &[Track], cur: i64) -> i64 {
    let ids: Vec<i64> = tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Sub)
        .map(|t| t.id)
        .collect();
    match ids.iter().position(|i| *i == cur) {
        None => ids.first().copied().unwrap_or(0),
        Some(p) => ids.get(p + 1).copied().unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_choice() {
        use Choice::*;
        assert_eq!(choose(None, true), Embedded);
        assert_eq!(choose(None, false), External);
        assert_eq!(choose(Some("external"), true), External);
        assert_eq!(choose(Some("embedded"), false), Embedded);
        assert_eq!(choose(Some("junk"), true), Embedded);
    }

    #[test]
    fn controls_hide_and_return() {
        let t0 = Instant::now();
        let mut h = AutoHide::new(t0);
        assert!(h.visible(t0 + Duration::from_millis(2900), false));
        assert!(!h.visible(t0 + HIDE_AFTER, false));
        assert!(
            h.visible(t0 + Duration::from_secs(60), true),
            "held by hover/drag"
        );
        h.poke(t0 + Duration::from_secs(10));
        assert!(
            h.visible(t0 + Duration::from_secs(11), false),
            "motion brings them back"
        );
        assert!(h.visible(t0, false), "clock going backwards never panics");
    }

    #[test]
    fn keys_and_volume() {
        assert_eq!(key("space", false, false), Some(Key::Toggle));
        assert_eq!(key("left", false, false), Some(Key::Seek(-10.0)));
        assert_eq!(key("right", false, false), Some(Key::Seek(10.0)));
        assert_eq!(key("f", false, false), Some(Key::Fullscreen));
        assert_eq!(key("escape", false, false), Some(Key::Back));
        assert_eq!(key("q", false, false), None);
        assert_eq!(bump_volume(128.0, 5.0), 130.0);
        assert_eq!(bump_volume(2.0, -5.0), 0.0);
    }

    fn tl() -> Vec<Track> {
        parse_tracks(&serde_json::json!([
            {"id":1,"type":"video","codec":"h264"},
            {"id":1,"type":"audio","lang":"jpn","codec":"eac3","demux-channel-count":6,"default":true,"selected":true},
            {"id":2,"type":"audio","lang":"eng","title":"Commentary","codec":"aac","demux-channel-count":2},
            {"id":1,"type":"sub","lang":"eng","codec":"subrip","default":true},
            {"id":2,"type":"sub","lang":"spa","codec":"ass","forced":true,"external":true},
        ]))
    }

    #[test]
    fn track_labels_and_menus() {
        let t = tl();
        assert_eq!(t.len(), 4, "video track ignored");
        assert_eq!(t[0].label(), "Japanese - EAC3 - 5.1 - Default");
        assert_eq!(t[1].label(), "English - Commentary - AAC - Stereo");
        assert_eq!(t[2].label(), "English - SUBRIP - Default");
        assert_eq!(t[3].label(), "Spanish - Forced - External - ASS");
        let subs = items(Pop::Subs, &t, 1.0, Aspect::Auto, false);
        assert_eq!(subs[0].label, "Off");
        assert!(subs[0].on, "no subtitle selected means Off is on");
        assert_eq!(subs.len(), 3);
        let audio = items(Pop::Audio, &t, 1.0, Aspect::Auto, false);
        assert!(audio[0].on && !audio[1].on);
        assert_eq!(audio[1].act, MenuAct::Audio(2));
        assert_eq!(next_sub(&t, 0), 1);
        assert_eq!(next_sub(&t, 1), 2);
        assert_eq!(next_sub(&t, 2), 0, "wraps to off");
        assert!(parse_tracks(&serde_json::json!(null)).is_empty());
    }

    fn chs() -> Vec<(f64, String)> {
        parse_chapters(
            &serde_json::json!([{"time":0.0,"title":"A"},{"time":60.0,"title":"B"},{"time":120.0}]),
        )
    }

    #[test]
    fn chapter_lookup_and_navigation() {
        let c = chs();
        assert_eq!(chapter_at(&c, 0.0), Some(0));
        assert_eq!(chapter_at(&c, 59.9), Some(0));
        assert_eq!(chapter_at(&c, 60.0), Some(1));
        assert_eq!(chapter_at(&[], 5.0), None);
        assert_eq!(chapter_target(&c, 10.0, 1), Some(60.0));
        assert_eq!(chapter_target(&c, 130.0, 1), None, "no next chapter");
        assert_eq!(
            chapter_target(&c, 62.0, -1),
            Some(0.0),
            "within 3 s goes to previous"
        );
        assert_eq!(
            chapter_target(&c, 70.0, -1),
            Some(60.0),
            "later restarts the chapter"
        );
        assert_eq!(chapter_target(&c, 1.0, -1), Some(0.0));
    }

    #[test]
    fn speed_ladder_and_formats() {
        assert_eq!(step_speed(1.0, 1), 1.25);
        assert_eq!(step_speed(1.0, -1), 0.75);
        assert_eq!(step_speed(4.0, 1), 4.0);
        assert_eq!(step_speed(0.25, -1), 0.25);
        assert_eq!(step_speed(1.1, 1), 1.25, "off-ladder goes to the next stop");
        assert_eq!(step_speed(1.1, -1), 1.0);
        assert_eq!(step_speed(3.0, 0), 1.0);
        assert_eq!(fmt_speed(1.0), "1x");
        assert_eq!(fmt_speed(1.25), "1.25x");
        assert_eq!(fmt_delay(0.1), "+100 ms");
        assert_eq!(fmt_delay(-0.2), "-200 ms");
        assert_eq!(fmt_delay(0.0), "+0 ms");
    }

    #[test]
    fn ends_at_wraps_midnight_and_scales_with_speed() {
        assert_eq!(ends_at((12, 0), 3600.0, 1.0), "Ends at 13:00");
        assert_eq!(ends_at((12, 0), 3600.0, 2.0), "Ends at 12:30");
        assert_eq!(ends_at((23, 50), 1200.0, 1.0), "Ends at 0:10");
    }

    #[test]
    fn settings_menu_shows_current_values() {
        let s = items(Pop::Settings, &[], 1.5, Aspect::R16x9, true);
        assert_eq!(s[0].value, "16:9");
        assert_eq!(s[1].value, "1.5x");
        assert_eq!(s[2].value, "One");
        assert_eq!(s[3].act, MenuAct::Info);
        let a = items(Pop::Aspect, &[], 1.0, Aspect::Fill, false);
        assert_eq!(a.iter().filter(|i| i.on).count(), 1);
        assert_eq!(Aspect::Fill.props()[1], ("panscan", "1"));
    }

    #[test]
    fn extra_keys() {
        assert_eq!(key("]", false, false), Some(Key::Speed(1)));
        assert_eq!(key("\\", false, false), Some(Key::Speed(0)));
        assert_eq!(
            key("s", false, true),
            Some(Key::Screenshot),
            "ctrl+s is not stop"
        );
        assert_eq!(key("/", true, false), Some(Key::Help));
        assert_eq!(key("/", false, false), None);
        assert_eq!(key("pagedown", false, false), Some(Key::Chapter(1)));
        assert_eq!(key("a", false, true), None);
    }
}
