//! Server UI mode, the pure part: which home sections a Jellyfin server's user has chosen (the
//! `homesection0..9` entries of jellyfin-web in `CustomPrefs`), in which order, and how the
//! user's library order and exclusions shape the "My media" tiles and the Latest rows. See
//! `docs/dev/SERVER_UI_PLAN.md`. No I/O; the reads live in `nocturne-jellyfin`, the write in
//! `nocturne-jellyfin-user`.
use crate::profile::{ProfileKind, Sources, UiFrom};
use crate::source::SourceId;
use nocturne_jellyfin::{Branding, DisplayPrefs, UserConfig};
use std::collections::HashMap;

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

/// What the app knows about a server whose UI might be used, right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avail {
    /// Its `ServerUi` is loaded.
    Ready,
    /// Being fetched: the NOCTURNE layout shows meanwhile, without a note.
    Loading,
    SignedOut,
    Unreachable,
}

/// Why the NOCTURNE layout is shown although another UI was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiNote {
    SignedOut(SourceId),
    Unreachable(SourceId),
    /// The source is switched off in Settings > Sources.
    Disabled(SourceId),
    /// The chosen source is gone or is not a server (the saved choice is reset on load).
    Removed,
}

impl UiNote {
    /// One visible line for the banner or the settings page; `label` names a source.
    pub fn message(self, label: &dyn Fn(SourceId) -> String) -> String {
        match self {
            Self::SignedOut(i) => format!(
                "{} is signed out, so the NOCTURNE layout is shown.",
                label(i)
            ),
            Self::Unreachable(i) => format!(
                "{} cannot be reached, so the NOCTURNE layout is shown.",
                label(i)
            ),
            Self::Disabled(i) => format!(
                "{} is switched off, so the NOCTURNE layout is shown.",
                label(i)
            ),
            Self::Removed => {
                "The server whose layout was chosen is gone, so the NOCTURNE layout is shown."
                    .into()
            }
        }
    }
}

/// Whose UI to show for one browsing context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    /// The server whose layout and branding apply; `None` is the NOCTURNE layout.
    pub from: Option<SourceId>,
    /// `from` is the very server being browsed, so its library order and exclusions apply too.
    pub own: bool,
    /// Set when `from` is `None` although another UI was chosen.
    pub note: Option<UiNote>,
}

impl Pick {
    /// The server whose branding CSS is the theme layer (the `ui` argument of
    /// `customize::Store::active_with`).
    pub fn branding_from(&self) -> Option<SourceId> {
        self.from
    }

    /// The only server the home-sections editor may be offered for: the one being browsed with its
    /// own UI. A borrowed UI is read-only, and nothing is ever written to its server.
    pub fn editable(&self) -> Option<SourceId> {
        self.from.filter(|_| self.own)
    }
}

/// Choose the UI for browsing `ctx` (a source, or `None` for "All"). `item_server` is the server
/// of the item or row at hand, used when "All" is set to each item's own server. Pure: `avail`
/// is how the app reports each server. Anything that cannot be honoured falls back to the NOCTURNE
/// layout with a note (except while loading).
pub fn pick(
    sources: &Sources,
    ctx: Option<SourceId>,
    item_server: Option<SourceId>,
    avail: &dyn Fn(SourceId) -> Avail,
) -> Pick {
    let target = match sources.ui_of(ctx) {
        UiFrom::Nocturne => return Pick::default(),
        UiFrom::SelfServer => match ctx {
            Some(c) => c,
            None => match item_server {
                Some(i) => i,
                None => return Pick::default(),
            },
        },
        UiFrom::Source(t) => t,
    };
    let fallback = |note| Pick {
        from: None,
        own: false,
        note,
    };
    let Some(p) = sources
        .get(target)
        .filter(|p| matches!(p.kind, ProfileKind::Jellyfin { .. }))
    else {
        return fallback(Some(UiNote::Removed));
    };
    if !p.enabled {
        return fallback(Some(UiNote::Disabled(target)));
    }
    match avail(target) {
        Avail::Ready => Pick {
            from: Some(target),
            own: ctx == Some(target),
            note: None,
        },
        Avail::Loading => fallback(None),
        Avail::SignedOut => fallback(Some(UiNote::SignedOut(target))),
        Avail::Unreachable => fallback(Some(UiNote::Unreachable(target))),
    }
}

/// The servers whose `ServerUi` the app has to load: every enabled server some choice points at
/// (and every enabled server while "All" uses each item's own UI).
pub fn wanted(sources: &Sources) -> Vec<SourceId> {
    let mut v: Vec<SourceId> = vec![];
    let enabled = |i: SourceId| {
        sources
            .get(i)
            .is_some_and(|p| p.enabled && matches!(p.kind, ProfileKind::Jellyfin { .. }))
    };
    let mut add = |i: SourceId| {
        if enabled(i) && !v.contains(&i) {
            v.push(i);
        }
    };
    for (k, to) in &sources.ui_from {
        let ctx = k.parse().ok().map(SourceId);
        match *to {
            UiFrom::Source(t) => add(t),
            UiFrom::SelfServer => match ctx {
                Some(c) => add(c),
                None => sources.ids().into_iter().for_each(&mut add),
            },
            UiFrom::Nocturne => {}
        }
    }
    v.sort();
    v
}

/// The layout to use for `pick`: the chosen server's sections and branding, and its library order
/// and exclusions only when it is the server being browsed. `None`: the NOCTURNE layout.
pub fn effective(pick: &Pick, loaded: &HashMap<SourceId, ServerUi>) -> Option<ServerUi> {
    let ui = loaded.get(&pick.from?)?;
    Some(if pick.own { ui.clone() } else { ui.foreign() })
}

impl ServerUi {
    /// This server's UI applied to another source's data: the section list and CSS carry over;
    /// the library order, exclusions and "hide played" belong to this server's libraries and
    /// users, so they are dropped.
    pub fn foreign(&self) -> ServerUi {
        ServerUi {
            sections: self.sections.clone(),
            css: self.css.clone(),
            disclaimer: self.disclaimer.clone(),
            ..Default::default()
        }
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

    // ---- one server's UI over another ----

    fn setup() -> (Sources, SourceId, SourceId, SourceId) {
        let jf = |u: &str| ProfileKind::Jellyfin {
            url: u.into(),
            user_id: "u".into(),
            user_name: "n".into(),
        };
        let mut s = Sources::default();
        let a = s.add("A", jf("https://a.test"));
        let b = s.add("B", jf("https://b.test"));
        let l = s.add("Local", ProfileKind::Local { roots: vec![] });
        (s, a, b, l)
    }

    fn ready(_: SourceId) -> Avail {
        Avail::Ready
    }

    #[test]
    fn default_is_nocturne_and_self_is_the_old_switch() {
        let (mut s, a, b, _) = setup();
        assert_eq!(pick(&s, Some(a), None, &ready), Pick::default());
        assert!(s.set_server_ui(a, true));
        let p = pick(&s, Some(a), None, &ready);
        assert_eq!((p.from, p.own, p.note), (Some(a), true, None));
        assert_eq!(p.editable(), Some(a));
        assert_eq!(pick(&s, Some(b), None, &ready), Pick::default());
    }

    #[test]
    fn a_server_ui_over_another_server_all_and_local() {
        let (mut s, a, b, l) = setup();
        // A's UI over B
        assert!(s.set_ui_from(Some(b), UiFrom::Source(a)));
        let p = pick(&s, Some(b), None, &ready);
        assert_eq!((p.from, p.own), (Some(a), false));
        assert_eq!(p.branding_from(), Some(a));
        assert_eq!(p.editable(), None, "a borrowed UI is never editable");
        // "All" and the local folder use B's UI
        assert!(s.set_ui_from(None, UiFrom::Source(b)));
        assert!(s.set_ui_from(Some(l), UiFrom::Source(b)));
        for ctx in [None, Some(l)] {
            let p = pick(&s, ctx, None, &ready);
            assert_eq!((p.from, p.own), (Some(b), false));
        }
        // "All" on `self` means the server of the item at hand, nothing without one
        assert!(s.set_ui_from(None, UiFrom::SelfServer));
        assert_eq!(pick(&s, None, Some(a), &ready).from, Some(a));
        assert_eq!(pick(&s, None, None, &ready), Pick::default());
        // choosing a source's own id is `self`; a local folder cannot use its own UI
        assert!(s.set_ui_from(Some(a), UiFrom::Source(a)));
        assert_eq!(s.ui_of(Some(a)), UiFrom::SelfServer);
        assert!(!s.set_ui_from(Some(l), UiFrom::SelfServer));
        assert!(
            !s.set_ui_from(Some(b), UiFrom::Source(l)),
            "only servers can lend a UI"
        );
    }

    #[test]
    fn unavailable_ui_sources_fall_back_with_a_visible_note() {
        let (mut s, a, b, _) = setup();
        s.set_ui_from(Some(b), UiFrom::Source(a));
        let p = pick(&s, Some(b), None, &|_| Avail::SignedOut);
        assert_eq!((p.from, p.note), (None, Some(UiNote::SignedOut(a))));
        let p = pick(&s, Some(b), None, &|_| Avail::Unreachable);
        assert_eq!(p.note, Some(UiNote::Unreachable(a)));
        // still loading: NOCTURNE layout, no flashing note
        assert_eq!(
            pick(&s, Some(b), None, &|_| Avail::Loading),
            Pick::default()
        );
        s.profiles[0].enabled = false;
        assert_eq!(
            pick(&s, Some(b), None, &ready).note,
            Some(UiNote::Disabled(a))
        );
        let m = UiNote::SignedOut(a).message(&|_| "Server A".into());
        assert!(m.contains("Server A") && m.contains("NOCTURNE layout"));
        // a dangling choice (hand-edited file) is handled too
        let mut d = Sources::default();
        d.ui_from.insert("all".into(), UiFrom::Source(SourceId(99)));
        assert_eq!(pick(&d, None, None, &ready).note, Some(UiNote::Removed));
    }

    #[test]
    fn removing_the_ui_source_resets_the_choice() {
        let (mut s, a, b, _) = setup();
        s.set_ui_from(Some(b), UiFrom::Source(a));
        s.set_ui_from(None, UiFrom::Source(a));
        s.set_ui_from(Some(a), UiFrom::SelfServer);
        assert!(s.remove(a));
        assert!(s.ui_from.is_empty(), "{:?}", s.ui_from);
        assert_eq!(pick(&s, Some(b), None, &ready), Pick::default());
        assert_eq!(s.ui_of(None), UiFrom::Nocturne);
    }

    #[test]
    fn the_layout_is_the_servers_and_the_data_is_the_browsed_sources() {
        let (_, a, b, _) = setup();
        let ui = ServerUi {
            sections: vec![Section::Latest, Section::Resume],
            ordered_views: vec!["z".into()],
            my_media_excludes: vec!["x".into()],
            latest_excludes: vec!["y".into()],
            hide_played_in_latest: true,
            css: Some("a{}".into()),
            disclaimer: Some("d".into()),
        };
        let loaded = HashMap::from([(a, ui.clone())]);
        let libs = || ["x", "y", "z"].map(String::from).to_vec();
        // the server's own UI: everything applies
        let own = effective(
            &Pick {
                from: Some(a),
                own: true,
                note: None,
            },
            &loaded,
        )
        .unwrap();
        assert_eq!(own, ui);
        assert_eq!(own.tiles(libs(), |s| s.as_str()), ["z", "y"]);
        // over another server: sections and CSS only, that server's libraries untouched
        let foreign = effective(
            &Pick {
                from: Some(a),
                own: false,
                note: None,
            },
            &loaded,
        )
        .unwrap();
        assert_eq!(foreign.shown(), [Section::Latest, Section::Resume]);
        assert_eq!(foreign.css.as_deref(), Some("a{}"));
        assert_eq!(foreign.tiles(libs(), |s| s.as_str()), libs());
        assert_eq!(foreign.latest_libs(libs(), |s| s.as_str()), libs());
        assert!(!foreign.hide_played_in_latest && foreign.ordered_views.is_empty());
        // NOCTURNE layout, or a UI that is not loaded yet
        assert!(effective(&Pick::default(), &loaded).is_none());
        assert!(
            effective(
                &Pick {
                    from: Some(b),
                    own: false,
                    note: None
                },
                &loaded
            )
            .is_none()
        );
    }

    #[test]
    fn wanted_lists_the_servers_to_load() {
        let (mut s, a, b, l) = setup();
        assert!(wanted(&s).is_empty());
        s.set_ui_from(Some(l), UiFrom::Source(b));
        assert_eq!(wanted(&s), [b]);
        s.set_ui_from(Some(a), UiFrom::SelfServer);
        assert_eq!(wanted(&s), [a, b]);
        s.set_ui_from(None, UiFrom::SelfServer);
        assert_eq!(wanted(&s), [a, b]);
        s.profiles[1].enabled = false;
        assert_eq!(wanted(&s), [a]);
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
