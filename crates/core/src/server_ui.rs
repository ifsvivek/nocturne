//! Server UI mode, the pure part: which home sections a Jellyfin server's user has chosen (the
//! `homesection0..9` entries of jellyfin-web in `CustomPrefs`), in which order, and how the
//! user's library order and exclusions shape the "My media" tiles and the Latest rows. See
//! `docs/dev/SERVER_UI_PLAN.md`. No I/O; the reads live in `nocturne-jellyfin`, the write in
//! `nocturne-jellyfin-user`.
use nocturne_jellyfin::{Branding, DisplayPrefs, UserConfig};

/// jellyfin-web's `MAX_SECTIONS`.
pub const SLOTS: usize = 10;

/// Server default when a slot has no entry (`homeSectionType.ts` / `getDefaultSection`).
const DEFAULT: [&str; SLOTS] = [
    "smalllibrarytiles",
    "resume",
    "resumeaudio",
    "resumebook",
    "livetv",
    "nextup",
    "latestmedia",
    "none",
    "none",
    "none",
];

/// A home section kind jellyfin-web knows. `none` and unknown values are not sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// `smalllibrarytiles` ("My media").
    MyMedia,
    /// `librarybuttons`: the same data as tiles, drawn as buttons.
    LibraryButtons,
    Resume,
    ResumeAudio,
    ResumeBook,
    ActiveRecordings,
    LiveTv,
    NextUp,
    Latest,
}

impl Section {
    /// Every kind, in the order the editor lists the disabled ones.
    pub const ALL: [Section; 9] = [
        Self::MyMedia,
        Self::LibraryButtons,
        Self::Resume,
        Self::ResumeAudio,
        Self::ResumeBook,
        Self::NextUp,
        Self::Latest,
        Self::LiveTv,
        Self::ActiveRecordings,
    ];

    pub fn parse(v: &str) -> Option<Self> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            // `folders` is the legacy name of the tiles.
            "smalllibrarytiles" | "folders" => Self::MyMedia,
            "librarybuttons" => Self::LibraryButtons,
            "resume" => Self::Resume,
            "resumeaudio" => Self::ResumeAudio,
            "resumebook" => Self::ResumeBook,
            "activerecordings" => Self::ActiveRecordings,
            "livetv" => Self::LiveTv,
            "nextup" => Self::NextUp,
            "latestmedia" => Self::Latest,
            _ => return None,
        })
    }

    /// The value written back to the server.
    pub fn value(self) -> &'static str {
        match self {
            Self::MyMedia => "smalllibrarytiles",
            Self::LibraryButtons => "librarybuttons",
            Self::Resume => "resume",
            Self::ResumeAudio => "resumeaudio",
            Self::ResumeBook => "resumebook",
            Self::ActiveRecordings => "activerecordings",
            Self::LiveTv => "livetv",
            Self::NextUp => "nextup",
            Self::Latest => "latestmedia",
        }
    }

    /// Drawn by NOCTURNE. Live TV rows, and audio and book continue rows (Continue watching is
    /// video only here), are kept in the list but skipped without an error.
    pub fn native(self) -> bool {
        !matches!(
            self,
            Self::LiveTv | Self::ActiveRecordings | Self::ResumeAudio | Self::ResumeBook
        )
    }

    /// The wording of the server's own web UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::MyMedia => "My media",
            Self::LibraryButtons => "My media (small)",
            Self::Resume => "Continue watching",
            Self::ResumeAudio => "Continue listening",
            Self::ResumeBook => "Continue reading",
            Self::ActiveRecordings => "Active recordings",
            Self::LiveTv => "Live TV",
            Self::NextUp => "Next up",
            Self::Latest => "Latest media",
        }
    }
}

/// The sections of a user, top to bottom: slot `i` is `homesection{i}`, a missing or null slot
/// takes the server default, `none` and unknown values are dropped, repeats keep their first
/// place.
pub fn home_sections(prefs: &DisplayPrefs) -> Vec<Section> {
    let mut out: Vec<Section> = vec![];
    for (i, d) in DEFAULT.iter().enumerate() {
        let v = prefs
            .get(&format!("homesection{i}"))
            .filter(|v| !v.is_empty())
            .unwrap_or(d);
        if let Some(s) = Section::parse(v)
            && !out.contains(&s)
        {
            out.push(s);
        }
    }
    out
}

/// `ids` re-ordered so the listed ones come first in the listed order, the rest after in their
/// own order; ids that are not present are ignored.
pub fn order_by<T>(items: Vec<T>, id: impl Fn(&T) -> &str, first: &[String]) -> Vec<T> {
    let rank = |t: &T| first.iter().position(|f| f == id(t)).unwrap_or(first.len());
    let mut v = items;
    v.sort_by_key(rank); // stable
    v
}

/// Everything Server UI mode needs from one server, read once per connect.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerUi {
    pub sections: Vec<Section>,
    pub ordered_views: Vec<String>,
    pub my_media_excludes: Vec<String>,
    pub latest_excludes: Vec<String>,
    pub hide_played_in_latest: bool,
    pub css: Option<String>,
    pub disclaimer: Option<String>,
}

impl ServerUi {
    pub fn new(prefs: &DisplayPrefs, cfg: &UserConfig, branding: &Branding) -> Self {
        let text = |o: &Option<String>| o.clone().filter(|s| !s.trim().is_empty());
        Self {
            sections: home_sections(prefs),
            ordered_views: cfg.ordered_views.clone(),
            my_media_excludes: cfg.my_media_excludes.clone(),
            latest_excludes: cfg.latest_items_excludes.clone(),
            hide_played_in_latest: cfg.hide_played_in_latest,
            css: text(&branding.custom_css),
            disclaimer: text(&branding.login_disclaimer),
        }
    }

    /// Libraries for the My media tiles: user order, excluded ones hidden.
    pub fn tiles<T>(&self, libs: Vec<T>, id: impl Fn(&T) -> &str + Copy) -> Vec<T> {
        let mut v = order_by(libs, id, &self.ordered_views);
        v.retain(|t| !self.my_media_excludes.iter().any(|e| e == id(t)));
        v
    }

    /// Libraries that get a Latest row: user order, excluded ones dropped.
    pub fn latest_libs<T>(&self, libs: Vec<T>, id: impl Fn(&T) -> &str + Copy) -> Vec<T> {
        let mut v = order_by(libs, id, &self.ordered_views);
        v.retain(|t| !self.latest_excludes.iter().any(|e| e == id(t)));
        v
    }

    /// Sections NOCTURNE draws, in the server's order.
    pub fn shown(&self) -> Vec<Section> {
        self.sections
            .iter()
            .copied()
            .filter(|s| s.native())
            .collect()
    }
}

/// The editable list: every section kind once, enabled ones first in the user's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    pub rows: Vec<(Section, bool)>,
}

impl Editor {
    pub fn new(enabled: &[Section]) -> Self {
        let mut rows: Vec<(Section, bool)> = enabled.iter().map(|s| (*s, true)).collect();
        rows.extend(
            Section::ALL
                .iter()
                .filter(|s| !enabled.contains(s))
                .map(|s| (*s, false)),
        );
        Self { rows }
    }

    pub fn toggle(&mut self, i: usize) {
        if let Some(r) = self.rows.get_mut(i) {
            r.1 = !r.1;
        }
    }

    /// Move row `i` one place up (`-1`) or down (`1`).
    pub fn shift(&mut self, i: usize, by: isize) {
        let j = i.checked_add_signed(by).filter(|j| *j < self.rows.len());
        if let (true, Some(j)) = (i < self.rows.len(), j) {
            self.rows.swap(i, j);
        }
    }

    /// The ten `homesection` values to store: enabled sections in order, then `none`.
    pub fn values(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self
            .rows
            .iter()
            .filter(|r| r.1)
            .map(|r| r.0.value())
            .collect();
        v.resize(SLOTS, "none");
        v
    }

    pub fn enabled(&self) -> Vec<Section> {
        self.rows.iter().filter(|r| r.1).map(|r| r.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Section::*;

    fn prefs(pairs: &[(&str, Option<&str>)]) -> DisplayPrefs {
        let mut p = DisplayPrefs::default();
        for (k, v) in pairs {
            p.custom_prefs.insert((*k).into(), v.map(Into::into));
        }
        p
    }

    #[test]
    fn untouched_user_gets_the_jellyfin_web_default_order() {
        assert_eq!(
            home_sections(&DisplayPrefs::default()),
            [
                MyMedia,
                Resume,
                ResumeAudio,
                ResumeBook,
                LiveTv,
                NextUp,
                Latest
            ]
        );
        // null values are the same as missing
        assert_eq!(
            home_sections(&prefs(&[("homesection0", None), ("tvhome", None)])),
            home_sections(&DisplayPrefs::default())
        );
    }

    #[test]
    fn chosen_slots_win_none_hides_and_unknown_kinds_are_ignored() {
        let p = prefs(&[
            ("homesection0", Some("latestmedia")),
            ("homesection1", Some("none")),
            ("homesection2", Some("folders")),
            ("homesection3", Some("somethingnew")),
            ("homesection4", Some("resume")),
            ("homesection5", Some("resume")),
        ]);
        // slots 6.. keep the defaults: slot 6 is latestmedia (already placed), 7..9 none
        assert_eq!(home_sections(&p), [Latest, MyMedia, Resume]);
    }

    #[test]
    fn libraries_follow_ordered_views_and_exclusions() {
        let ui = ServerUi {
            ordered_views: vec!["c".into(), "a".into(), "gone".into()],
            my_media_excludes: vec!["a".into()],
            latest_excludes: vec!["b".into()],
            ..Default::default()
        };
        let libs = || ["a", "b", "c", "d"].map(String::from).to_vec();
        assert_eq!(ui.tiles(libs(), |s| s.as_str()), ["c", "b", "d"]);
        assert_eq!(ui.latest_libs(libs(), |s| s.as_str()), ["c", "a", "d"]);
        assert_eq!(ServerUi::default().tiles(libs(), |s| s.as_str()), libs());
    }

    #[test]
    fn branding_and_skipped_rows() {
        let b = Branding {
            custom_css: Some("  ".into()),
            login_disclaimer: Some("Hello".into()),
        };
        let ui = ServerUi::new(&DisplayPrefs::default(), &UserConfig::default(), &b);
        assert!(ui.css.is_none());
        assert_eq!(ui.disclaimer.as_deref(), Some("Hello"));
        assert!(!ui.shown().contains(&LiveTv) && ui.shown().contains(&Resume));
    }

    #[test]
    fn editor_round_trips_and_keeps_ten_slots() {
        let mut e = Editor::new(&[Latest, Resume]);
        assert_eq!(e.rows.len(), Section::ALL.len());
        assert_eq!(e.values()[..3], ["latestmedia", "resume", "none"]);
        assert_eq!(e.values().len(), SLOTS);
        e.shift(1, -1);
        e.shift(0, -1); // top stays
        e.shift(99, 1); // out of range is a no-op
        assert_eq!(e.enabled(), [Resume, Latest]);
        e.toggle(0);
        assert_eq!(e.enabled(), [Latest]);
        // what the editor writes reads back as the same list
        let all_on = Editor::new(&Section::ALL);
        let mut p = DisplayPrefs::default();
        for (i, v) in all_on.values().iter().enumerate() {
            p.custom_prefs
                .insert(format!("homesection{i}"), Some((*v).into()));
        }
        assert_eq!(home_sections(&p), Section::ALL);
    }
}
