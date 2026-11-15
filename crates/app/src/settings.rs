//! User settings: TOML at `$XDG_CONFIG_HOME/nocturne/settings.toml`, atomic write, versioned,
//! never any secret (the login lives in the keyring). Every field has a default, so a missing,
//! older or half-written file still loads.
use crate::libview::LibPrefs;
use crate::video::{PlayerPrefs, SubMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Report playback progress and watched state to the server.
    pub sync_progress: bool,
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
    pub sub_mode: SubPref,
    pub auto_skip_intro: bool,
    pub autoplay_next: bool,
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
            version: VERSION,
            sync_progress: true,
            audio_lang: None,
            sub_lang: None,
            sub_mode: SubPref::Off,
            auto_skip_intro: false,
            autoplay_next: true,
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

/// ISO 639-2 codes the language rows cycle through (`None` first = "Automatic").
const LANGS: [(&str, &str); 9] = [
    ("eng", "English"),
    ("jpn", "Japanese"),
    ("spa", "Spanish"),
    ("fra", "French"),
    ("deu", "German"),
    ("ita", "Italian"),
    ("por", "Portuguese"),
    ("kor", "Korean"),
    ("hin", "Hindi"),
];

fn lang_name(l: &Option<String>) -> String {
    match l {
        None => "Automatic".into(),
        Some(c) => LANGS
            .iter()
            .find(|(k, _)| k == c)
            .map_or(c.clone(), |(_, n)| n.to_string()),
    }
}

fn next_lang(l: &Option<String>, dir: i32) -> Option<String> {
    // Index 0 = automatic, 1.. = LANGS.
    let n = LANGS.len() as i32 + 1;
    let cur = l
        .as_ref()
        .and_then(|c| LANGS.iter().position(|(k, _)| k == c))
        .map_or(0, |i| i as i32 + 1);
    match (cur + dir).rem_euclid(n) {
        0 => None,
        i => Some(LANGS[i as usize - 1].0.into()),
    }
}

fn onoff(b: bool) -> String {
    if b { "On" } else { "Off" }.into()
}

pub struct Row {
    pub label: &'static str,
    pub hint: &'static str,
    pub value: String,
}

/// Number of rows in the Settings screen (`rows()` / `cycle()` index them).
pub const ROWS: usize = 11;

const CACHE_MB: [u32; 5] = [128, 256, 512, 1024, 2048];

impl Settings {
    pub fn rows(&self) -> Vec<Row> {
        let r = |label, hint, value| Row { label, hint, value };
        vec![
            r(
                "Sync watch progress",
                "Report what you watch and mark it watched on the server.",
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
                match self.sub_mode {
                    SubPref::Off => "Off",
                    SubPref::ForeignOnly => "Foreign audio only",
                    SubPref::Always => "Always",
                }
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
                format!("{} MB", self.image_cache_mb),
            ),
        ]
    }

    /// Step row `i` forward (`dir` > 0) or back. Out-of-range rows do nothing.
    pub fn cycle(&mut self, i: usize, dir: i32) {
        let d = if dir < 0 { -1 } else { 1 };
        match i {
            0 => self.sync_progress ^= true,
            1 => self.audio_lang = next_lang(&self.audio_lang, d),
            2 => self.sub_lang = next_lang(&self.sub_lang, d),
            3 => {
                use SubPref::*;
                self.sub_mode = match (self.sub_mode, d) {
                    (Off, 1) | (Always, -1) => ForeignOnly,
                    (ForeignOnly, 1) | (Off, _) => Always,
                    _ => Off,
                }
            }
            4 => self.auto_skip_intro ^= true,
            5 => self.autoplay_next ^= true,
            6 => self.home_resume ^= true,
            7 => self.home_next_up ^= true,
            8 => self.home_latest ^= true,
            9 => {
                use Density::*;
                self.density = match (self.density, d) {
                    (Compact, 1) | (Spacious, -1) => Comfortable,
                    (Comfortable, 1) | (Compact, _) => Spacious,
                    _ => Compact,
                }
            }
            10 => {
                let i = CACHE_MB
                    .iter()
                    .position(|m| *m == self.image_cache_mb)
                    .unwrap_or(2) as i32;
                self.image_cache_mb = CACHE_MB[(i + d).rem_euclid(CACHE_MB.len() as i32) as usize];
            }
            _ => {}
        }
    }

    /// Home sections shown: continue watching, next up, latest.
    pub fn home(&self) -> (bool, bool, bool) {
        (self.home_resume, self.home_next_up, self.home_latest)
    }

    /// What the player gets (see `hooks::PlayerPrefs`).
    #[allow(dead_code)] // consumed by the player once its setter lands
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
        s.version = VERSION;
        s
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
    fn defaults_sync_on() {
        let s = Settings::default();
        assert!(s.sync_progress && s.autoplay_next && !s.auto_skip_intro);
        assert_eq!(s.rows().len(), ROWS);
    }

    #[test]
    fn round_trip_is_atomic_and_secret_free() {
        let d = dir("rt");
        let p = d.join("nocturne").join("settings.toml");
        let mut s = Settings::default();
        s.cycle(0, 1);
        s.cycle(1, 1);
        s.cycle(3, 1);
        s.cycle(9, -1);
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        assert!(!p.with_extension("toml.tmp").exists());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("sync_progress = false") && text.contains("audio_lang = \"eng\""));
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
        std::fs::write(&p, "version = 0\nsync_progress = false\nfuture_thing = 1\n").unwrap();
        let s = Settings::load_from(&p);
        assert!(!s.sync_progress && s.autoplay_next && s.version == VERSION);
        std::fs::write(&p, "this is {{ not toml").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default(), "corrupt");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn cycling_wraps_both_ways() {
        let mut s = Settings::default();
        assert_eq!(s.audio_lang, None);
        s.cycle(1, -1);
        assert_eq!(
            s.audio_lang.as_deref(),
            Some("hin"),
            "back from automatic wraps to the end"
        );
        s.cycle(1, 1);
        assert_eq!(s.audio_lang, None);
        for _ in 0..3 {
            s.cycle(3, 1);
        }
        assert_eq!(s.sub_mode, SubPref::Off);
        s.cycle(9, 1);
        assert_eq!(s.density, Density::Spacious);
        s.cycle(9, 1);
        assert_eq!(s.density, Density::Compact);
        s.cycle(99, 1); // out of range is a no-op
        s.cycle(10, 1);
        assert_eq!(s.image_cache_mb, 1024);
        s.cycle(10, -1);
        s.cycle(10, -1);
        assert_eq!(s.image_cache_mb, 256);
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
        s.cycle(1, 1);
        s.cycle(3, 1);
        s.cycle(4, 1);
        let p = s.prefs();
        assert_eq!(p.audio_lang.as_deref(), Some("eng"));
        assert_eq!(p.sub_mode, SubMode::ForeignOnly);
        assert!(p.auto_skip_intro && p.autoplay_next);
    }
}
