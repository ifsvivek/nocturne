//! User settings: TOML at `$XDG_CONFIG_HOME/nocturne/settings.toml`, atomic write, versioned,
//! never any secret (the login lives in the keyring). Every field has a default, so a missing,
//! older or half-written file still loads.
use crate::libview::LibPrefs;
use crate::video::{LANGUAGES, PlayerPrefs, SubMode, language};
use nocturne_player::Buffering;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 2: server writes became opt-in (`allow_server_writes`); the old sync default is reset.
pub const VERSION: u32 = 2;

fn zero() -> u32 {
    0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubPref {
    #[default]
    Off,
    ForeignOnly,
    Always,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

impl Density {
    /// Target poster width (px) for grids and rows.
    pub fn card(self) -> f32 {
        match self {
            Self::Compact => 150.0,
            Self::Comfortable => 176.0,
            Self::Spacious => 210.0,
        }
    }
}

/// The remembered profiles of the signed-in server, for the "Default profile" row. Read from
/// `accounts.toml` by the app (never stored in `settings.toml`); the row is changed there, not here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profiles {
    /// (user id, name) in the order they were remembered.
    pub list: Vec<(String, String)>,
    /// The default profile's user id; `None` asks every time.
    pub default: Option<String>,
}

impl Profiles {
    /// Entry of the drop-down that is current: 0 is "Ask every time", then one per profile.
    pub fn current(&self) -> usize {
        self.default
            .as_ref()
            .and_then(|d| self.list.iter().position(|(id, _)| id == d))
            .map_or(0, |i| i + 1)
    }

    pub fn labels(&self) -> Vec<String> {
        std::iter::once("Ask every time".to_string())
            .chain(self.list.iter().map(|(_, n)| n.clone()))
            .collect()
    }

    /// The user id behind drop-down entry `idx` (`None`: ask every time or out of range).
    pub fn id_at(&self, idx: usize) -> Option<&str> {
        self.list
            .get(idx.checked_sub(1)?)
            .map(|(id, _)| id.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(skip)]
    pub profiles: Profiles,
    /// A file without a version predates versioning (0).
    #[serde(default = "zero")]
    pub version: u32,
    /// Master switch: NOCTURNE may change data on the server (played, favourite, progress,
    /// admin actions, password). Off by default; every write needs it.
    pub allow_server_writes: bool,
    /// Report playback progress to the server (only counts while `allow_server_writes` is on).
    pub sync_progress: bool,
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
    pub sub_mode: SubPref,
    pub auto_skip_intro: bool,
    pub autoplay_next: bool,
    /// Network read-ahead: fill to `buffer_ahead_secs`, refill once below `buffer_refill_secs`,
    /// never hold more than `buffer_max_mb` MiB in RAM.
    pub buffer_ahead_secs: u32,
    pub buffer_refill_secs: u32,
    pub buffer_max_mb: u32,
    pub home_resume: bool,
    pub home_next_up: bool,
    pub home_latest: bool,
    pub density: Density,
    /// Disk image cache cap in MB.
    pub image_cache_mb: u32,
    /// Sort, view and flag filters remembered per library (key = library id).
    pub libs: BTreeMap<String, LibPrefs>,
    /// Most recent first.
    pub recent_searches: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profiles: Profiles::default(),
            version: VERSION,
            allow_server_writes: false,
            sync_progress: false,
            audio_lang: None,
            sub_lang: None,
            sub_mode: SubPref::Off,
            auto_skip_intro: false,
            autoplay_next: true,
            buffer_ahead_secs: 300,
            buffer_refill_secs: 120,
            buffer_max_mb: 1536,
            home_resume: true,
            home_next_up: true,
            home_latest: true,
            density: Density::Comfortable,
            image_cache_mb: 512,
            libs: BTreeMap::new(),
            recent_searches: vec![],
        }
    }
}

fn lang_name(l: &Option<String>) -> String {
    l.as_deref().map_or("Automatic".into(), language)
}

/// Menu entries for a language row: Automatic, then `LANGUAGES` in order.
fn lang_labels() -> Vec<String> {
    std::iter::once("Automatic".to_string())
        .chain(LANGUAGES.iter().map(|(n, _)| n.to_string()))
        .collect()
}

fn lang_index(l: &Option<String>) -> Option<usize> {
    match l {
        None => Some(0),
        Some(c) => LANGUAGES
            .iter()
            .position(|(_, cs)| cs.contains(&c.as_str()))
            .map(|i| i + 1),
    }
}

fn lang_pick(i: usize) -> Option<Option<String>> {
    match i {
        0 => Some(None),
        i => LANGUAGES.get(i - 1).map(|(_, cs)| Some(cs[0].to_string())),
    }
}

/// Language after/before `l` in the menu order (wrapping); an unknown value starts from Automatic.
fn next_lang(l: &Option<String>, dir: i32) -> Option<String> {
    let n = LANGUAGES.len() as i32 + 1;
    let cur = lang_index(l).unwrap_or(0) as i32;
    lang_pick((cur + dir).rem_euclid(n) as usize).flatten()
}

fn onoff(b: bool) -> String {
    if b { "On" } else { "Off" }.into()
}

pub struct Row {
    pub label: &'static str,
    pub hint: &'static str,
    pub value: String,
}

/// Number of rows in the Settings screen (`rows()` / `choices()` / `cycle()` index them).
pub const ROWS: usize = 16;

/// The "Default profile" row; the app applies its changes (they live in `accounts.toml`).
pub const PROFILE_ROW: usize = 15;

/// Section title shown above row `i`, if a section starts there.
pub fn head(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("Server changes"),
        2 => Some("Playback"),
        10 => Some("Home"),
        13 => Some("Appearance and storage"),
        15 => Some("Profiles"),
        _ => None,
    }
}

/// Index of row `i` among the Settings page's scroll children: a title, then each row preceded by its
/// section header, if it starts one. Lets the page scroll the focused row into view.
pub fn scroll_child(i: usize) -> usize {
    1 + i + (0..=i).filter(|r| head(*r).is_some()).count()
}

/// The row a navigation key lands on from `sel` (Up / Down / PgUp / PgDn / Home / End), clamped.
pub fn nav_row(sel: usize, key: &str) -> Option<usize> {
    const PAGE: usize = 5;
    Some(
        match key {
            "up" => sel.saturating_sub(1),
            "down" => sel + 1,
            "pageup" => sel.saturating_sub(PAGE),
            "pagedown" => sel + PAGE,
            "home" => 0,
            "end" => ROWS - 1,
            _ => return None,
        }
        .min(ROWS - 1),
    )
}

/// Two plain sentences under the "Server changes" title.
pub const WRITES_NOTE: &str = "When this is on, NOCTURNE can mark items watched, set favourites, report what you watch (if sync is on) and, for administrators, change server settings, always asking before anything destructive. When it is off, NOCTURNE only reads from your server and never changes anything on it.";

const CACHE_MB: [u32; 5] = [128, 256, 512, 1024, 2048];
const AHEAD_S: [u32; 6] = [60, 120, 180, 300, 600, 900];
const REFILL_S: [u32; 5] = [30, 60, 120, 180, 300];
const BUF_MB: [u32; 7] = [256, 512, 1024, 1536, 2048, 3072, 4096];

const SUB_MODES: [(SubPref, &str); 3] = [
    (SubPref::Off, "Off"),
    (SubPref::ForeignOnly, "Foreign audio only"),
    (SubPref::Always, "Always"),
];
const DENSITIES: [Density; 3] = [Density::Compact, Density::Comfortable, Density::Spacious];

fn fmt_secs(v: u32) -> String {
    format!("{v} s")
}
fn fmt_gib(mb: u32) -> String {
    format!("{:.1} GiB", mb as f64 / 1024.0)
}
fn fmt_mb(v: u32) -> String {
    format!("{v} MB")
}

/// The value after/before `cur` in `opts` (wrapping); unknown values start from `at`.
fn step(opts: &[u32], cur: u32, at: usize, d: i32) -> u32 {
    let i = opts.iter().position(|v| *v == cur).unwrap_or(at) as i32;
    opts[(i + d).rem_euclid(opts.len() as i32) as usize]
}

impl Settings {
    pub fn rows(&self) -> Vec<Row> {
        let r = |label, hint, value| Row { label, hint, value };
        vec![
            r(
                "Allow NOCTURNE to change data on the server",
                "Off by default. Needed for watched, favourites, progress sync, admin changes and password change.",
                onoff(self.allow_server_writes),
            ),
            r(
                "Sync watch progress",
                "Report what you watch so Continue Watching follows you. Needs server changes on.",
                onoff(self.sync_progress),
            ),
            r(
                "Audio language",
                "Preferred when a video has several audio tracks.",
                lang_name(&self.audio_lang),
            ),
            r(
                "Subtitle language",
                "Preferred when a video has several subtitle tracks.",
                lang_name(&self.sub_lang),
            ),
            r(
                "Subtitles",
                "When to turn subtitles on by default.",
                SUB_MODES
                    .iter()
                    .find(|(m, _)| *m == self.sub_mode)
                    .map_or("Off", |(_, n)| n)
                    .into(),
            ),
            r(
                "Skip intros",
                "Jump past recognised intros without asking.",
                onoff(self.auto_skip_intro),
            ),
            r(
                "Play next episode",
                "Continue with the next episode when one ends.",
                onoff(self.autoplay_next),
            ),
            r(
                "Buffer ahead",
                "How much video to download ahead while playing.",
                fmt_secs(self.buffer_ahead_secs),
            ),
            r(
                "Refill below",
                "Start downloading again once the buffer drops under this.",
                fmt_secs(self.buffer_refill_secs),
            ),
            r(
                "Max RAM buffer",
                "Memory cap for the buffer. A 4K remux hits this cap before the seconds above.",
                fmt_gib(self.buffer_max_mb),
            ),
            r(
                "Home: Continue watching",
                "Show the row of items you have started.",
                onoff(self.home_resume),
            ),
            r(
                "Home: Next up",
                "Show the next episode of what you follow.",
                onoff(self.home_next_up),
            ),
            r(
                "Home: Latest",
                "Show recently added items per library.",
                onoff(self.home_latest),
            ),
            r(
                "Grid density",
                "How large posters are in rows and grids.",
                format!("{:?}", self.density),
            ),
            r(
                "Image cache",
                "Disk space for posters and backdrops; oldest are removed first.",
                fmt_mb(self.image_cache_mb),
            ),
            r(
                "Default profile",
                "Opens at launch without asking. Ask every time shows Who's watching. Needs two or more profiles on this server.",
                self.profiles.labels()[self.profiles.current()].clone(),
            ),
        ]
    }

    /// The entries of a multi-choice row and the index of the current one. A stored value that is not
    /// one of the presets (hand-edited file) is appended as its own entry and shown as it is.
    /// `None` for On / Off rows (they toggle) and out-of-range rows.
    pub fn choices(&self, i: usize) -> Option<(Vec<String>, usize)> {
        let nums = |opts: &[u32], cur: u32, f: fn(u32) -> String| {
            let mut v: Vec<String> = opts.iter().map(|o| f(*o)).collect();
            let at = opts.iter().position(|o| *o == cur).unwrap_or_else(|| {
                v.push(f(cur));
                v.len() - 1
            });
            (v, at)
        };
        let lang = |l: &Option<String>| {
            let mut v = lang_labels();
            let at = lang_index(l).unwrap_or_else(|| {
                v.push(lang_name(l));
                v.len() - 1
            });
            (v, at)
        };
        Some(match i {
            2 => lang(&self.audio_lang),
            3 => lang(&self.sub_lang),
            4 => (
                SUB_MODES.iter().map(|(_, n)| n.to_string()).collect(),
                SUB_MODES.iter().position(|(m, _)| *m == self.sub_mode)?,
            ),
            7 => nums(&AHEAD_S, self.buffer_ahead_secs, fmt_secs),
            8 => nums(&REFILL_S, self.buffer_refill_secs, fmt_secs),
            9 => nums(&BUF_MB, self.buffer_max_mb, fmt_gib),
            13 => (
                DENSITIES.iter().map(|d| format!("{d:?}")).collect(),
                DENSITIES.iter().position(|d| *d == self.density)?,
            ),
            14 => nums(&CACHE_MB, self.image_cache_mb, fmt_mb),
            PROFILE_ROW if self.profiles.list.len() > 1 => {
                (self.profiles.labels(), self.profiles.current())
            }
            _ => return None,
        })
    }

    /// Apply entry `idx` of `choices(i)`. The appended "unknown value" entry and anything out of
    /// range change nothing.
    pub fn choose(&mut self, i: usize, idx: usize) {
        match i {
            2 => self.audio_lang = lang_pick(idx).unwrap_or_else(|| self.audio_lang.take()),
            3 => self.sub_lang = lang_pick(idx).unwrap_or_else(|| self.sub_lang.take()),
            4 => self.sub_mode = SUB_MODES.get(idx).map_or(self.sub_mode, |(m, _)| *m),
            7 => {
                self.buffer_ahead_secs = AHEAD_S.get(idx).copied().unwrap_or(self.buffer_ahead_secs)
            }
            8 => {
                self.buffer_refill_secs = REFILL_S
                    .get(idx)
                    .copied()
                    .unwrap_or(self.buffer_refill_secs)
            }
            9 => self.buffer_max_mb = BUF_MB.get(idx).copied().unwrap_or(self.buffer_max_mb),
            13 => self.density = DENSITIES.get(idx).copied().unwrap_or(self.density),
            14 => self.image_cache_mb = CACHE_MB.get(idx).copied().unwrap_or(self.image_cache_mb),
            _ => {}
        }
    }

    /// Step row `i` forward (`dir` > 0) or back; toggles flip. The keyboard's Left / Right use this; clicks open a menu
    /// for multi-choice rows (`choices`). Out-of-range rows do nothing.
    pub fn cycle(&mut self, i: usize, dir: i32) {
        let d = if dir < 0 { -1 } else { 1 };
        match i {
            0 => self.allow_server_writes ^= true,
            1 => self.sync_progress ^= true,
            2 => self.audio_lang = next_lang(&self.audio_lang, d),
            3 => self.sub_lang = next_lang(&self.sub_lang, d),
            4 => {
                use SubPref::*;
                self.sub_mode = match (self.sub_mode, d) {
                    (Off, 1) | (Always, -1) => ForeignOnly,
                    (ForeignOnly, 1) | (Off, _) => Always,
                    _ => Off,
                }
            }
            5 => self.auto_skip_intro ^= true,
            6 => self.autoplay_next ^= true,
            7 => self.buffer_ahead_secs = step(&AHEAD_S, self.buffer_ahead_secs, 3, d),
            8 => self.buffer_refill_secs = step(&REFILL_S, self.buffer_refill_secs, 2, d),
            9 => self.buffer_max_mb = step(&BUF_MB, self.buffer_max_mb, 3, d),
            10 => self.home_resume ^= true,
            11 => self.home_next_up ^= true,
            12 => self.home_latest ^= true,
            13 => {
                use Density::*;
                self.density = match (self.density, d) {
                    (Compact, 1) | (Spacious, -1) => Comfortable,
                    (Comfortable, 1) | (Compact, _) => Spacious,
                    _ => Compact,
                }
            }
            14 => self.image_cache_mb = step(&CACHE_MB, self.image_cache_mb, 2, d),
            _ => {}
        }
    }

    /// Home sections shown: continue watching, next up, latest.
    pub fn home(&self) -> (bool, bool, bool) {
        (self.home_resume, self.home_next_up, self.home_latest)
    }

    /// What the player gets (see `video::PlayerPrefs`).
    pub fn prefs(&self) -> PlayerPrefs {
        PlayerPrefs {
            audio_lang: self.audio_lang.clone(),
            sub_lang: self.sub_lang.clone(),
            sub_mode: match self.sub_mode {
                SubPref::Off => SubMode::Off,
                SubPref::ForeignOnly => SubMode::ForeignOnly,
                SubPref::Always => SubMode::Always,
            },
            auto_skip_intro: self.auto_skip_intro,
            autoplay_next: self.autoplay_next,
        }
    }

    /// The read-ahead policy; the player clamps nonsense (refill must stay below ahead).
    pub fn buffering(&self) -> Buffering {
        Buffering {
            high_secs: self.buffer_ahead_secs as f64,
            low_secs: self.buffer_refill_secs as f64,
            max_bytes: (self.buffer_max_mb as u64) << 20,
            ..Default::default()
        }
    }

    pub fn default_path() -> Option<PathBuf> {
        nocturne_session::Servers::default_path()
            .ok()
            .map(|p| p.with_file_name("settings.toml"))
    }

    pub fn load() -> Self {
        Self::default_path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    /// Missing, unreadable or corrupt file gives defaults; an older version is stamped current
    /// (new fields take their defaults, so "migration" is just that).
    pub fn load_from(path: &Path) -> Self {
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        s.migrate();
        s
    }

    /// Older files: version 1 shipped with sync on by default, which was never an informed choice
    /// to write to the server, so it is reset; the master switch starts off.
    fn migrate(&mut self) {
        if self.version < 2 {
            self.sync_progress = false;
            self.allow_server_writes = false;
        }
        self.version = VERSION;
    }

    pub fn save(&self) {
        if let Some(p) = Self::default_path() {
            let _ = self.save_to(&p); // calm: a failed save only loses the preference
        }
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("path has a parent"))?;
        let tmp = path.with_extension("toml.tmp");
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nocturne-settings-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn defaults_write_nothing() {
        let s = Settings::default();
        assert!(!s.allow_server_writes && !s.sync_progress);
        assert!(s.autoplay_next && !s.auto_skip_intro);
        assert_eq!(s.rows().len(), ROWS);
    }

    #[test]
    fn round_trip_is_atomic_and_secret_free() {
        let d = dir("rt");
        let p = d.join("nocturne").join("settings.toml");
        let mut s = Settings::default();
        s.cycle(0, 1);
        s.cycle(1, 1);
        s.choose(2, 6); // English
        s.cycle(4, 1);
        s.cycle(13, -1);
        s.cycle(7, 1);
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        assert!(!p.with_extension("toml.tmp").exists());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.contains("sync_progress = true") && text.contains("allow_server_writes = true")
        );
        assert!(text.contains("audio_lang = \"eng\""));
        assert!(
            !text.to_lowercase().contains("token") && !text.to_lowercase().contains("password")
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn missing_old_and_corrupt_files() {
        let d = dir("old");
        let p = d.join("settings.toml");
        assert_eq!(Settings::load_from(&p), Settings::default(), "missing");
        std::fs::create_dir_all(&d).unwrap();
        // Version 0 (pre-versioning) with one field and an unknown one: kept, rest defaulted, stamped.
        std::fs::write(&p, "version = 0\nautoplay_next = false\nfuture_thing = 1\n").unwrap();
        let s = Settings::load_from(&p);
        assert!(!s.autoplay_next && !s.sync_progress && s.version == VERSION);
        std::fs::write(&p, "this is {{ not toml").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default(), "corrupt");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn buffering_defaults_and_cycling() {
        let mut s = Settings::default();
        let b = s.buffering();
        assert_eq!(
            (b.high_secs, b.low_secs, b.max_bytes),
            (300.0, 120.0, 3 << 29)
        );
        s.cycle(7, 1);
        s.cycle(9, -1);
        let b = s.buffering();
        assert_eq!((b.high_secs, b.max_bytes), (600.0, 1024 << 20));
        s.buffer_ahead_secs = 77; // unknown value (hand-edited file) starts from the default slot
        s.cycle(7, 1);
        assert_eq!(s.buffer_ahead_secs, 600);
    }

    #[test]
    fn cycling_wraps_both_ways() {
        let mut s = Settings::default();
        assert_eq!(s.audio_lang, None);
        s.cycle(2, -1);
        assert_eq!(
            s.audio_lang.as_deref(),
            Some("vie"),
            "back from automatic wraps to the end"
        );
        s.cycle(2, 1);
        assert_eq!(s.audio_lang, None);
        for _ in 0..3 {
            s.cycle(4, 1);
        }
        assert_eq!(s.sub_mode, SubPref::Off);
        s.cycle(13, 1);
        assert_eq!(s.density, Density::Spacious);
        s.cycle(13, 1);
        assert_eq!(s.density, Density::Compact);
        s.cycle(99, 1); // out of range is a no-op
        s.cycle(14, 1);
        assert_eq!(s.image_cache_mb, 1024);
        s.cycle(14, -1);
        s.cycle(14, -1);
        assert_eq!(s.image_cache_mb, 256);
    }

    #[test]
    fn choices_list_every_option_and_mark_the_current_one() {
        let mut s = Settings::default();
        assert!(s.choices(0).is_none() && s.choices(12).is_none() && s.choices(99).is_none());
        let (l, at) = s.choices(2).unwrap();
        assert_eq!((l[0].as_str(), at, l.len()), ("Automatic", 0, 29));
        assert!(
            ["English", "Arabic", "Vietnamese", "Hindi"]
                .iter()
                .all(|n| l.iter().any(|x| x == n))
        );
        let eng = l.iter().position(|x| x == "English").unwrap();
        s.choose(2, eng);
        assert_eq!(
            s.audio_lang.as_deref(),
            Some("eng"),
            "stored format unchanged"
        );
        assert_eq!(s.choices(2).unwrap().1, eng);
        s.choose(2, 0);
        assert_eq!(s.audio_lang, None);
        let (l, at) = s.choices(4).unwrap();
        assert_eq!((l.len(), at), (3, 0));
        s.choose(4, 1);
        assert_eq!(s.sub_mode, SubPref::ForeignOnly);
        s.choose(13, 2);
        assert_eq!(s.density, Density::Spacious);
        let (l, at) = s.choices(9).unwrap();
        assert_eq!((l[at].as_str(), l.len()), ("1.5 GiB", 7));
        s.choose(7, 5);
        assert_eq!(s.buffer_ahead_secs, 900);
        s.choose(14, 99); // out of range: no change
        assert_eq!(s.image_cache_mb, 512);
    }

    #[test]
    fn unknown_stored_values_show_as_is_and_survive() {
        let mut s = Settings {
            audio_lang: Some("xx-custom".into()),
            buffer_ahead_secs: 77,
            ..Default::default()
        };
        let (l, at) = s.choices(2).unwrap();
        assert_eq!((l.len(), l[at].as_str()), (30, "xx-custom"));
        assert_eq!(s.rows()[2].value, "xx-custom");
        s.choose(2, at); // picking the extra entry again keeps it
        assert_eq!(s.audio_lang.as_deref(), Some("xx-custom"));
        let (l, at) = s.choices(7).unwrap();
        assert_eq!((l.len(), l[at].as_str()), (7, "77 s"));
        s.choose(7, at);
        assert_eq!(s.buffer_ahead_secs, 77);
        // An alias a hand-edited file may hold is the known language, not a duplicate entry.
        s.audio_lang = Some("en".into());
        let (l, at) = s.choices(2).unwrap();
        assert_eq!((l.len(), l[at].as_str()), (29, "English"));
    }

    #[test]
    fn default_profile_row_lists_ask_every_time_then_each_profile() {
        let mut s = Settings::default();
        assert_eq!(s.rows().len(), ROWS);
        assert_eq!(s.rows()[PROFILE_ROW].value, "Ask every time");
        assert!(
            s.choices(PROFILE_ROW).is_none(),
            "nothing to choose with fewer than two"
        );
        s.profiles = Profiles {
            list: vec![("a".into(), "Ann".into()), ("b".into(), "Bob".into())],
            default: Some("b".into()),
        };
        let (l, at) = s.choices(PROFILE_ROW).unwrap();
        assert_eq!(
            (l.as_slice(), at),
            (
                &["Ask every time".to_string(), "Ann".into(), "Bob".into()][..],
                2
            )
        );
        assert_eq!(s.rows()[PROFILE_ROW].value, "Bob");
        assert_eq!(
            (
                s.profiles.id_at(0),
                s.profiles.id_at(1),
                s.profiles.id_at(9)
            ),
            (None, Some("a"), None)
        );
        s.profiles.default = Some("gone".into()); // removed meanwhile: shows as ask every time
        assert_eq!(s.profiles.current(), 0);
        // Never written to settings.toml.
        assert!(!toml::to_string_pretty(&s).unwrap().contains("Bob"));
    }

    #[test]
    fn scroll_and_focus_logic() {
        // Child 0 is the title; heads sit before rows 0, 2, 10, 13 and 15.
        assert_eq!(scroll_child(0), 2);
        assert_eq!(scroll_child(1), 3);
        assert_eq!(scroll_child(2), 5);
        assert_eq!(scroll_child(10), 14);
        assert_eq!(scroll_child(ROWS - 1), 1 + ROWS - 1 + 5);
        assert!((1..ROWS).all(|i| scroll_child(i) > scroll_child(i - 1)));
        assert_eq!(nav_row(0, "up"), Some(0));
        assert_eq!(nav_row(ROWS - 1, "down"), Some(ROWS - 1));
        assert_eq!(nav_row(3, "pagedown"), Some(8));
        assert_eq!(nav_row(12, "pagedown"), Some(ROWS - 1));
        assert_eq!(nav_row(3, "pageup"), Some(0));
        assert_eq!(
            (nav_row(7, "home"), nav_row(7, "end")),
            (Some(0), Some(ROWS - 1))
        );
        assert_eq!(nav_row(7, "x"), None);
    }

    #[test]
    fn library_prefs_and_searches_persist_and_old_files_load() {
        let d = dir("libs");
        let p = d.join("settings.toml");
        let mut s = Settings::default();
        s.libs.insert(
            "lib1".into(),
            LibPrefs {
                desc: true,
                ..Default::default()
            },
        );
        s.recent_searches = vec!["alpha".into()];
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        std::fs::write(&p, "version = 1\nsync_progress = false\n").unwrap();
        let old = Settings::load_from(&p);
        assert!(old.libs.is_empty() && old.image_cache_mb == 512);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn prefs_follow_settings() {
        let mut s = Settings::default();
        s.choose(2, 6); // English
        s.cycle(4, 1);
        s.cycle(5, 1);
        let p = s.prefs();
        assert_eq!(p.audio_lang.as_deref(), Some("eng"));
        assert_eq!(p.sub_mode, SubMode::ForeignOnly);
        assert!(p.auto_skip_intro && p.autoplay_next);
    }

    #[test]
    fn migration_resets_the_old_sync_default() {
        let d = dir("mig");
        let p = d.join("settings.toml");
        std::fs::create_dir_all(&d).unwrap();
        // A version-1 file as the former default wrote it: sync on, nothing about the master switch.
        std::fs::write(
            &p,
            "version = 1\nsync_progress = true\naudio_lang = \"jpn\"\n",
        )
        .unwrap();
        let s = Settings::load_from(&p);
        assert!(!s.sync_progress && !s.allow_server_writes, "reset");
        assert_eq!(
            s.audio_lang.as_deref(),
            Some("jpn"),
            "other choices survive"
        );
        assert_eq!(s.version, VERSION);
        // A file with no version at all is older still.
        std::fs::write(&p, "sync_progress = true\n").unwrap();
        assert!(!Settings::load_from(&p).sync_progress);
        // Current files keep what the user chose.
        std::fs::write(
            &p,
            "version = 2\nallow_server_writes = true\nsync_progress = true\n",
        )
        .unwrap();
        let s = Settings::load_from(&p);
        assert!(s.allow_server_writes && s.sync_progress);
        std::fs::remove_dir_all(d).unwrap();
    }
}
