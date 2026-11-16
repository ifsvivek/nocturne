//! Playback seam (embedded libmpv vs child-process mpv) and the pure logic of the player page:
//! backend choice, control auto-hide, key map. No GPUI, so it is unit-tested without a window.
use nocturne_player::{EmbeddedPlayer, Event, Player, Result, State};
use serde_json::Value;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// Subtitle default rule from the user's playback settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubMode {
    #[default]
    Off,
    /// Only when the audio language differs from the subtitle language (forced track otherwise).
    ForeignOnly,
    Always,
}

/// Playback preferences supplied by the caller (from settings). Languages are ISO 639 codes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerPrefs {
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
    pub sub_mode: SubMode,
    pub auto_skip_intro: bool,
    pub autoplay_next: bool,
}

/// Account-layer hooks, supplied through `PlayerCtx { hooks, prefs }`. The player page calls ONLY
/// `set_favorite` / `is_favorite` (heart button). `started`/`progress`/`stopped` are kept for signature
/// compatibility; playback reporting is driven by the app's own tick. Every method must return
/// immediately (called on the UI thread): implementations dispatch the work elsewhere.
pub trait PlayerHooks: Send + Sync {
    fn started(&self, item_id: &str, position_ticks: i64);
    fn progress(&self, item_id: &str, position_ticks: i64, paused: bool);
    fn stopped(&self, item_id: &str, position_ticks: i64);
    fn set_favorite(&self, item_id: &str, fav: bool);
    fn is_favorite(&self, item_id: &str) -> bool;
    /// Server changes are allowed (master switch on, real login): the heart is live only then.
    fn can_write(&self) -> bool;
}

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
    Prev,
    Queue,
    AbLoop,
    Mini,
    Favorite,
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
        ("m", _) if shift => Key::Mini,
        ("m", _) => Key::Mute,
        ("p", _) => Key::Prev,
        ("u", _) => Key::Queue,
        ("a", _) => Key::AbLoop,
        ("h", _) => Key::Favorite,
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
    /// Friendly name from the server's stream list; replaces the built label.
    pub name: Option<String>,
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
                name: None,
            })
        })
        .collect()
}

/// Languages the app knows by name: (English name, ISO 639 codes). The first code is what the
/// Settings language lists store; the others are aliases tracks may carry.
pub const LANGUAGES: [(&str, &[&str]); 28] = [
    ("Arabic", &["ara", "ar"]),
    ("Chinese", &["zho", "chi", "zh"]),
    ("Czech", &["ces", "cze", "cs"]),
    ("Danish", &["dan", "da"]),
    ("Dutch", &["nld", "dut", "nl"]),
    ("English", &["eng", "en"]),
    ("Finnish", &["fin", "fi"]),
    ("French", &["fra", "fre", "fr"]),
    ("German", &["deu", "ger", "de"]),
    ("Greek", &["ell", "gre", "el"]),
    ("Hebrew", &["heb", "he"]),
    ("Hindi", &["hin", "hi"]),
    ("Hungarian", &["hun", "hu"]),
    ("Indonesian", &["ind", "id"]),
    ("Italian", &["ita", "it"]),
    ("Japanese", &["jpn", "ja"]),
    ("Korean", &["kor", "ko"]),
    ("Norwegian", &["nor", "nob", "nno", "no"]),
    ("Polish", &["pol", "pl"]),
    ("Portuguese", &["por", "pt"]),
    ("Romanian", &["ron", "rum", "ro"]),
    ("Russian", &["rus", "ru"]),
    ("Spanish", &["spa", "es"]),
    ("Swedish", &["swe", "sv"]),
    ("Thai", &["tha", "th"]),
    ("Turkish", &["tur", "tr"]),
    ("Ukrainian", &["ukr", "uk"]),
    ("Vietnamese", &["vie", "vi"]),
];

/// ISO 639 code to a name for the languages in `LANGUAGES`; anything else is shown as given.
pub fn language(code: &str) -> String {
    match code {
        "" | "und" => "Unknown".into(),
        c => LANGUAGES
            .iter()
            .find(|(_, codes)| codes.contains(&c))
            .map_or_else(|| c.to_string(), |(n, _)| n.to_string()),
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
        if let Some(n) = &self.name {
            return n.clone();
        }
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
    Quality,
    Picture,
    SubStyle,
    AudioFx,
    Tone,
    Devices,
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
            Pop::Quality => "Quality",
            Pop::Picture => "Picture",
            Pop::SubStyle => "Subtitle Style",
            Pop::AudioFx => "Audio Options",
            Pop::Tone => "HDR Tone Mapping",
            Pop::Devices => "Audio Output",
        }
    }
    /// Sub-menus of Settings go back to it; top-level menus close.
    pub fn is_sub(self) -> bool {
        !matches!(self, Pop::Subs | Pop::Audio | Pop::Settings)
    }
    /// Settings (or its sub-menus) are showing: the gear button is lit.
    pub fn in_settings(self) -> bool {
        !matches!(self, Pop::Subs | Pop::Audio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    One,
    All,
}

impl Repeat {
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "None",
            Repeat::One => "Repeat One",
            Repeat::All => "Repeat All",
        }
    }
}

pub const TONES: [&str; 6] = ["auto", "bt.2390", "hable", "mobius", "reinhard", "clip"];
pub const PIC_NAMES: [&str; 4] = ["Brightness", "Contrast", "Saturation", "Gamma"];
pub const PIC_PROPS: [&str; 4] = ["brightness", "contrast", "saturation", "gamma"];

/// Everything the menus show besides the track list. Plain values, so rows are pure to test.
#[derive(Clone, PartialEq)]
pub struct Settings {
    pub speed: f64,
    pub aspect: Aspect,
    pub repeat: Repeat,
    pub quality: crate::player_logic::Quality,
    pub sub_style: usize,
    /// User's base subtitle position (0 top, 100 bottom).
    pub sub_pos: i64,
    /// Brightness, contrast, saturation, gamma (-100..100).
    pub pic: [i64; 4],
    /// Zoom in 10 % steps (video-zoom = log2(1 + steps/10)); pan in 5 % steps.
    pub zoom: i64,
    pub pan: [i64; 2],
    pub rotate: i64,
    pub deint: bool,
    pub tone: usize,
    pub normalize: bool,
    pub downmix: bool,
    /// (mpv device name, description) and the selected name.
    pub devices: Vec<(String, String)>,
    pub device: String,
    /// 0 off, 1 A set, 2 looping A-B.
    pub ab: u8,
    /// Jellyfin said this item can be transcoded (has a source id): the Quality menu is live.
    pub can_quality: bool,
    pub method: &'static str,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            speed: 1.0,
            aspect: Aspect::Auto,
            repeat: Repeat::Off,
            quality: crate::player_logic::Quality::Original,
            sub_style: 0,
            sub_pos: 100,
            pic: [0; 4],
            zoom: 0,
            pan: [0; 2],
            rotate: 0,
            deint: false,
            tone: 0,
            normalize: false,
            downmix: false,
            devices: vec![],
            device: String::new(),
            ab: 0,
            can_quality: false,
            method: "Direct Play",
        }
    }
}

impl Settings {
    /// mpv `video-zoom` (log2) for the zoom steps.
    pub fn zoom_log2(&self) -> f64 {
        (1.0 + self.zoom as f64 / 10.0).max(0.1).log2()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuAct {
    /// Track id; 0 = off.
    Sub(i64),
    Audio(i64),
    Speed(f64),
    Aspect(Aspect),
    Repeat(Repeat),
    Quality(crate::player_logic::Quality),
    SubStyle(usize),
    SubPos(i64),
    /// Picture property index (see `PIC_NAMES`) and direction.
    Pic(usize, i8),
    Zoom(i8),
    Pan(usize, i8),
    Rotate,
    Deint,
    Tone(usize),
    Normalize,
    Downmix,
    Device(usize),
    AbLoop,
    ResetPicture,
    Open(Pop),
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// Right-aligned muted value (settings list).
    pub value: String,
    pub on: bool,
    /// Click / Enter / Right.
    pub act: MenuAct,
    /// Left key and the minus button of stepper rows.
    pub dec: Option<MenuAct>,
}

fn item(label: impl Into<String>, on: bool, act: MenuAct) -> Item {
    Item {
        label: label.into(),
        value: String::new(),
        on,
        act,
        dec: None,
    }
}

fn row(l: &str, v: impl Into<String>, a: MenuAct) -> Item {
    Item {
        value: v.into(),
        ..item(l, false, a)
    }
}

fn step(l: &str, v: impl Into<String>, dec: MenuAct, inc: MenuAct) -> Item {
    Item {
        dec: Some(dec),
        ..row(l, v, inc)
    }
}

fn onoff(b: bool) -> &'static str {
    if b { "On" } else { "Off" }
}

/// Rows of a menu.
pub fn items(pop: Pop, tracks: &[Track], s: &Settings) -> Vec<Item> {
    use crate::player_logic::Quality;
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
            v.push(row("Subtitle Style", "", MenuAct::Open(Pop::SubStyle)));
            v
        }
        Pop::Audio => tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| item(t.label(), t.selected, MenuAct::Audio(t.id)))
            .collect(),
        Pop::Speed => SPEEDS
            .iter()
            .map(|x| {
                item(
                    fmt_speed(*x),
                    (*x - s.speed).abs() < 1e-6,
                    MenuAct::Speed(*x),
                )
            })
            .collect(),
        Pop::Aspect => Aspect::ALL
            .iter()
            .map(|a| item(a.label(), *a == s.aspect, MenuAct::Aspect(*a)))
            .collect(),
        Pop::Repeat => [Repeat::Off, Repeat::One, Repeat::All]
            .into_iter()
            .map(|r| item(r.label(), r == s.repeat, MenuAct::Repeat(r)))
            .collect(),
        Pop::Quality => Quality::ALL
            .iter()
            .map(|q| item(q.label(), *q == s.quality, MenuAct::Quality(*q)))
            .collect(),
        Pop::SubStyle => {
            let mut v: Vec<Item> = crate::player_logic::SUB_STYLES
                .iter()
                .enumerate()
                .map(|(i, st)| item(st.name, i == s.sub_style, MenuAct::SubStyle(i)))
                .collect();
            v.push(step(
                "Position",
                format!("{}%", s.sub_pos),
                MenuAct::SubPos(-5),
                MenuAct::SubPos(5),
            ));
            v
        }
        Pop::Picture => {
            let mut v: Vec<Item> = (0..4)
                .map(|i| {
                    step(
                        PIC_NAMES[i],
                        s.pic[i].to_string(),
                        MenuAct::Pic(i, -1),
                        MenuAct::Pic(i, 1),
                    )
                })
                .collect();
            v.push(step(
                "Zoom",
                format!("{}%", 100 + s.zoom * 10),
                MenuAct::Zoom(-1),
                MenuAct::Zoom(1),
            ));
            v.push(step(
                "Pan horizontal",
                format!("{:+}%", s.pan[0] * 5),
                MenuAct::Pan(0, -1),
                MenuAct::Pan(0, 1),
            ));
            v.push(step(
                "Pan vertical",
                format!("{:+}%", s.pan[1] * 5),
                MenuAct::Pan(1, -1),
                MenuAct::Pan(1, 1),
            ));
            v.push(row("Rotate", format!("{} deg", s.rotate), MenuAct::Rotate));
            v.push(row("Deinterlace", onoff(s.deint), MenuAct::Deint));
            v.push(row(
                "HDR Tone Mapping",
                TONES[s.tone % TONES.len()],
                MenuAct::Open(Pop::Tone),
            ));
            v.push(row("Reset picture", "", MenuAct::ResetPicture));
            v
        }
        Pop::Tone => TONES
            .iter()
            .enumerate()
            .map(|(i, t)| item(*t, i == s.tone, MenuAct::Tone(i)))
            .collect(),
        Pop::AudioFx => vec![
            row("Normalize volume", onoff(s.normalize), MenuAct::Normalize),
            row("Downmix to stereo", onoff(s.downmix), MenuAct::Downmix),
            row(
                "Output device",
                s.devices
                    .iter()
                    .find(|d| d.0 == s.device)
                    .map_or("Auto", |d| d.1.as_str()),
                MenuAct::Open(Pop::Devices),
            ),
        ],
        Pop::Devices => s
            .devices
            .iter()
            .enumerate()
            .map(|(i, d)| item(d.1.clone(), d.0 == s.device, MenuAct::Device(i)))
            .collect(),
        Pop::Settings => {
            let mut v = vec![];
            if s.can_quality {
                v.push(row(
                    "Quality",
                    s.quality.label(),
                    MenuAct::Open(Pop::Quality),
                ));
            }
            v.extend([
                row("Picture", "", MenuAct::Open(Pop::Picture)),
                row("Audio Options", "", MenuAct::Open(Pop::AudioFx)),
                row("Subtitle Style", "", MenuAct::Open(Pop::SubStyle)),
                row("Aspect Ratio", s.aspect.label(), MenuAct::Open(Pop::Aspect)),
                row(
                    "Playback Speed",
                    fmt_speed(s.speed),
                    MenuAct::Open(Pop::Speed),
                ),
                row("Repeat Mode", s.repeat.label(), MenuAct::Open(Pop::Repeat)),
                row(
                    "A-B Loop",
                    ["Off", "A set", "Looping"][s.ab.min(2) as usize],
                    MenuAct::AbLoop,
                ),
                row("Playback Info", s.method, MenuAct::Info),
            ]);
            v
        }
    }
}

/// Settings row index of a sub-menu (so Esc/selection returns to where you were).
pub fn settings_index(tracks: &[Track], s: &Settings, sub: Pop) -> usize {
    items(Pop::Settings, tracks, s)
        .iter()
        .position(|i| i.act == MenuAct::Open(sub))
        .unwrap_or(0)
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
        let st = Settings::default();
        let subs = items(Pop::Subs, &t, &st);
        assert_eq!(subs[0].label, "Off");
        assert!(subs[0].on, "no subtitle selected means Off is on");
        assert_eq!(subs.len(), 4, "off, two tracks, style entry");
        let audio = items(Pop::Audio, &t, &st);
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
        let mut st = Settings {
            speed: 1.5,
            aspect: Aspect::R16x9,
            repeat: Repeat::All,
            ..Default::default()
        };
        let s = items(Pop::Settings, &[], &st);
        let v = |l: &str| s.iter().find(|i| i.label == l).unwrap().value.clone();
        assert_eq!(v("Aspect Ratio"), "16:9");
        assert_eq!(v("Playback Speed"), "1.5x");
        assert_eq!(v("Repeat Mode"), "Repeat All");
        assert!(
            s.iter().all(|i| i.label != "Quality"),
            "no transcode source, no quality row"
        );
        st.can_quality = true;
        assert_eq!(items(Pop::Settings, &[], &st)[0].label, "Quality");
        let a = items(
            Pop::Aspect,
            &[],
            &Settings {
                aspect: Aspect::Fill,
                ..Default::default()
            },
        );
        assert_eq!(a.iter().filter(|i| i.on).count(), 1);
        assert_eq!(Aspect::Fill.props()[1], ("panscan", "1"));
    }

    #[test]
    fn picture_and_audio_rows() {
        let st = Settings {
            pic: [10, 0, -20, 0],
            zoom: 2,
            normalize: true,
            ..Default::default()
        };
        let p = items(Pop::Picture, &[], &st);
        assert_eq!(p[0].value, "10");
        assert_eq!(p[0].dec, Some(MenuAct::Pic(0, -1)));
        assert_eq!(p[0].act, MenuAct::Pic(0, 1));
        assert_eq!(p[4].value, "120%");
        assert!((st.zoom_log2() - 1.2f64.log2()).abs() < 1e-9);
        let a = items(Pop::AudioFx, &[], &st);
        assert_eq!(a[0].value, "On");
        assert_eq!(a[2].value, "Auto", "no device picked");
        let st = Settings {
            devices: vec![("pulse/x".into(), "Speakers".into())],
            device: "pulse/x".into(),
            ..Default::default()
        };
        assert_eq!(items(Pop::AudioFx, &[], &st)[2].value, "Speakers");
        assert!(items(Pop::Devices, &[], &st)[0].on);
        assert_eq!(
            settings_index(&[], &st, Pop::Speed),
            4,
            "Picture, Audio, SubStyle, Aspect, Speed"
        );
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
        assert_eq!(key("m", true, false), Some(Key::Mini));
        assert_eq!(key("m", false, false), Some(Key::Mute));
        assert_eq!(key("p", false, false), Some(Key::Prev));
    }
}
