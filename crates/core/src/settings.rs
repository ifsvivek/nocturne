//! User settings: TOML at `$XDG_CONFIG_HOME/nocturne/settings.toml`, atomic write, versioned,
//! never any secret (the login lives in the keyring). Every field has a default, so a missing,
//! older or half-written file still loads.
use crate::gateway::{Binding, GatewaySettings};
use crate::hdr::HdrMode;
use crate::libview::LibPrefs;
use crate::video::{Installed, LANGUAGES, PlayerPref, PlayerPrefs, SubMode, language};
use nocturne_player::Buffering;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 2: server writes became opt-in (`allow_server_writes`); the old sync default is reset.
pub const VERSION: u32 = 2;
/// Revision of the first-run walkthrough. `Settings::onboarding` below this means it is due.
pub const ONBOARDING: u32 = 1;

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

/// Couch / TV mode: a 10-foot layout (larger type and targets, always-visible focus, button hints).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CouchMode {
    #[default]
    Off,
    On,
    /// On while a gamepad is connected or the window fills a TV-sized display.
    Auto,
}

impl CouchMode {
    pub fn active(self, pad: bool, fills_tv: bool) -> bool {
        match self {
            Self::Off => false,
            Self::On => true,
            Self::Auto => pad || fills_tv,
        }
    }
}

/// Reduce motion: follow the desktop (`System`), or force it on / off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReduceMotion {
    #[default]
    System,
    On,
    Off,
}

impl ReduceMotion {
    /// Whether to cut decorative animation, given what the desktop asked for.
    pub fn reduced(self, system_prefers: bool) -> bool {
        match self {
            Self::System => system_prefers,
            Self::On => true,
            Self::Off => false,
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
    /// Let other Jellyfin apps of the same account see and control playback on this computer
    /// (posts this app's capabilities, listens for play/pause/seek/volume commands). Never in
    /// developer-token mode. docs/WRITES.md.
    pub remote_control: bool,
    /// Apply watched / unwatched (and, with `sync_progress`, resume positions) to the other copies of
    /// a title on other sources. Server copies also need `allow_server_writes`. docs/WRITES.md.
    pub sync_watched: bool,
    /// Scrobble what plays to Trakt (needs the Trakt connection made in its dialog). docs/TRAKT.md.
    pub trakt_scrobble: bool,
    /// Import Trakt's watched history into local items (and server items when changes are allowed).
    pub trakt_history: bool,
    /// Look for a newer NOCTURNE at most once a day (one GET of a small file; only the version is sent). docs/dev/UPDATER_PLAN.md.
    pub check_updates: bool,
    /// Themes may fetch what they link to: `@import`ed stylesheets and `@font-face` fonts, over
    /// https only, once, with size and time limits (docs/dev/CUSTOMIZATION_PLAN.md section 15).
    /// Off: nothing is fetched and those parts are ignored with a warning.
    pub remote_css: bool,
    /// Last first-run walkthrough the user finished or skipped (0 = never). Internal state, no Settings row.
    pub onboarding: u32,
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
    pub sub_mode: SubPref,
    pub auto_skip_intro: bool,
    pub autoplay_next: bool,
    /// What to do when an HDR video starts (docs/HDR.md).
    pub hdr: HdrMode,
    /// Where video is shown: in the window, or in VLC / mpv's own window (installed ones only).
    pub player: PlayerPref,
    /// Network read-ahead: fill to `buffer_ahead_secs`, refill once below `buffer_refill_secs`,
    /// never hold more than `buffer_max_mb` MiB in RAM.
    pub buffer_ahead_secs: u32,
    pub buffer_refill_secs: u32,
    pub buffer_max_mb: u32,
    pub home_resume: bool,
    pub home_next_up: bool,
    pub home_latest: bool,
    /// A "Coming this week" row on Home: this week's upcoming episodes from the Calendar. Off by default so Home stays as it was.
    pub home_coming: bool,
    pub density: Density,
    /// Disk image cache cap in MB.
    pub image_cache_mb: u32,
    pub reduce_motion: ReduceMotion,
    /// The home hero banner rotates by itself (a theme that shows one); off = manual only.
    pub hero_autoplay: bool,
    /// Seconds a banner slide stays (one of `HERO_S`).
    pub hero_interval_secs: u32,
    pub couch: CouchMode,
    /// Sort, view and flag filters remembered per library (key = library id).
    pub libs: BTreeMap<String, LibPrefs>,
    /// Most recent first.
    pub recent_searches: Vec<String>,
    /// Titles can be requested from the Seerr server, and own pending requests cancelled. A request goes to Seerr only,
    /// never to a media server, so this does not need `allow_server_writes`. Off by default. docs/WRITES.md.
    pub seerr_requests: bool,
    /// The one Seerr server and who is signed in there (the session cookie is in the keyring, never here).
    pub seerr: SeerrCfg,
    /// Requests without a Seerr server: kept in a list on this computer (`wishlist.rs`), nothing leaves it
    /// unless Radarr or Sonarr is also connected and `arr_requests` is on. Off by default.
    pub native_requests: bool,
    /// A native request is also added to the connected Radarr / Sonarr. A write to that service, always
    /// after the request dialog's confirmation. Off until the user turns it on. Needs no `allow_server_writes`
    /// (no media server is touched). docs/WRITES.md.
    pub arr_requests: bool,
    pub radarr: ArrCfg,
    pub sonarr: ArrCfg,
    /// The optional gateway companion (`nocturne-gateway`, docs/dev/GATEWAY_PLAN.md); the same `[gateway]` table is read by that process.
    pub gateway: GatewaySettings,
}

/// Non-secret state of a Radarr or Sonarr connection (the API key is in the keyring, never here).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrCfg {
    pub url: String,
    /// The quality profile and root folder requests use; 0 / empty = the service's first, chosen on connect.
    pub profile_id: u32,
    pub profile_name: String,
    pub root_folder: String,
}

/// Non-secret state of the Seerr connection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeerrCfg {
    pub url: String,
    pub user_id: u64,
    pub user_name: String,
    /// Seerr's permission bit mask as of the last sign-in or refresh.
    pub permissions: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profiles: Profiles::default(),
            version: VERSION,
            allow_server_writes: false,
            sync_progress: false,
            remote_control: false,
            sync_watched: false,
            trakt_scrobble: false,
            trakt_history: false,
            check_updates: true,
            remote_css: true,
            onboarding: 0,
            audio_lang: None,
            sub_lang: None,
            sub_mode: SubPref::Off,
            auto_skip_intro: false,
            autoplay_next: true,
            hdr: HdrMode::Ask,
            player: PlayerPref::InWindow,
            buffer_ahead_secs: 300,
            buffer_refill_secs: 120,
            buffer_max_mb: 1536,
            home_resume: true,
            home_next_up: true,
            home_latest: true,
            home_coming: false,
            density: Density::Comfortable,
            image_cache_mb: 512,
            reduce_motion: ReduceMotion::System,
            hero_autoplay: true,
            hero_interval_secs: 10,
            couch: CouchMode::Off,
            libs: BTreeMap::new(),
            recent_searches: vec![],
            seerr_requests: false,
            seerr: SeerrCfg::default(),
            native_requests: false,
            arr_requests: false,
            radarr: ArrCfg::default(),
            sonarr: ArrCfg::default(),
            gateway: GatewaySettings::default(),
        }
    }
}

fn arr_value(c: &ArrCfg) -> String {
    if c.url.is_empty() {
        "Not set up".into()
    } else {
        "Connected".into()
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
pub const ROWS: usize = 44;

/// The "Default profile" row; the app applies its changes (they live in `accounts.toml`).
pub const PROFILE_ROW: usize = 22;

/// The "Couch mode" row.
pub const COUCH_ROW: usize = 23;

/// The "Check for updates" row.
pub const UPDATES_ROW: usize = 24;

/// The "Themes may load from the web" row.
pub const REMOTE_CSS_ROW: usize = 25;

/// The "Rotate the home banner" row.
pub const HERO_AUTOPLAY_ROW: usize = 26;

/// The "Banner interval" row.
pub const HERO_INTERVAL_ROW: usize = 27;

/// The "Video player" row.
pub const PLAYER_ROW: usize = 28;
/// The "Coming this week" Home row switch.
pub const COMING_ROW: usize = 29;
/// The "Seerr server" row: not a toggle, it opens the sign-in dialog.
pub const SEERR_ROW: usize = 30;

/// The "Allow requests" row.
pub const SEERR_ALLOW_ROW: usize = 31;

/// "Requests without a Seerr server".
pub const NATIVE_ROW: usize = 32;
/// The "Radarr" / "Sonarr" rows: not toggles, they open the connect dialog.
pub const RADARR_ROW: usize = 33;
pub const SONARR_ROW: usize = 34;
/// "Add requests to Radarr and Sonarr".
pub const ARR_ALLOW_ROW: usize = 35;
/// The Settings > Gateway rows (docs/dev/GATEWAY_PLAN.md section 6), last in the list.
pub const GATEWAY_ROW: usize = 36;
pub const GATEWAY_BINDING_ROW: usize = 37;
pub const GATEWAY_PORT_ROW: usize = 38;
pub const GATEWAY_CACHE_ROW: usize = 39;
pub const GATEWAY_KEEP_ROW: usize = 40;
pub const GATEWAY_REMOTE_ROW: usize = 41;
pub const GATEWAY_VIEWS_ROW: usize = 42;
pub const GATEWAY_ANNOUNCE_ROW: usize = 43;

/// The "Trakt account" row: not a toggle, it opens the connect dialog.
pub const TRAKT_ROW: usize = 19;

/// Section title shown above row `i`, if a section starts there.
pub fn head(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("Server changes"),
        2 => Some("Playback"),
        11 => Some("Home"),
        14 => Some("Appearance and storage"),
        17 => Some("Other devices"),
        18 => Some("Watched state"),
        19 => Some("Trakt"),
        22 => Some("Profiles"),
        23 => Some("Living room"),
        24 => Some("Updates"),
        25 => Some("Themes"),
        26 => Some("Home banner"),
        PLAYER_ROW => Some("Video player"),
        COMING_ROW => Some("Calendar"),
        SEERR_ROW => Some("Requests"),
        GATEWAY_ROW => Some("Gateway"),
        NATIVE_ROW => Some("Requests without Seerr"),
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

const BINDINGS: [Binding; 3] = [Binding::Local, Binding::Lan, Binding::Remote];

/// The systemd user service is the only "keep running" mechanism so far (launchd and a Windows login task come with those platforms).
pub const fn keep_running_supported() -> bool {
    cfg!(target_os = "linux")
}

const GATEWAY_PORTS: [u32; 4] = [8099, 8100, 8199, 18096];
/// Gateway disk cache sizes in MiB; 0 is none.
const GATEWAY_CACHE_MB: [u32; 4] = [0, 256, 1024, 4096];

fn fmt_gw_cache(mb: u32) -> String {
    match mb {
        0 => "None".into(),
        m if m % 1024 == 0 => format!("{} GiB", m / 1024),
        m => format!("{m} MiB"),
    }
}

const CACHE_MB: [u32; 5] = [128, 256, 512, 1024, 2048];
/// Choices for the banner interval, seconds.
pub const HERO_S: [u32; 5] = [5, 8, 10, 15, 30];
const AHEAD_S: [u32; 6] = [60, 120, 180, 300, 600, 900];
const REFILL_S: [u32; 5] = [30, 60, 120, 180, 300];
const BUF_MB: [u32; 7] = [256, 512, 1024, 1536, 2048, 3072, 4096];

const SUB_MODES: [(SubPref, &str); 3] = [
    (SubPref::Off, "Off"),
    (SubPref::ForeignOnly, "Foreign audio only"),
    (SubPref::Always, "Always"),
];
const MOTION: [(ReduceMotion, &str); 3] = [
    (ReduceMotion::System, "System"),
    (ReduceMotion::On, "On"),
    (ReduceMotion::Off, "Off"),
];
const COUCH: [(CouchMode, &str); 3] = [
    (CouchMode::Off, "Off"),
    (CouchMode::On, "On"),
    (CouchMode::Auto, "Automatic"),
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
                "HDR video",
                "In-window video is SDR (tone-mapped). An HDR pop-out opens mpv's own window and needs an HDR-capable display.",
                self.hdr.label().into(),
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
                "Reduce motion",
                "Cut fades and the startup animation. System follows your desktop's animation setting.",
                MOTION
                    .iter()
                    .find(|(m, _)| *m == self.reduce_motion)
                    .map_or("System", |(_, n)| n)
                    .into(),
            ),
            r(
                "Allow remote control from other devices",
                "Other Jellyfin apps signed in as you can then see this computer and control playback on it. Off by default; needs server changes on.",
                onoff(self.remote_control),
            ),
            r(
                "Keep watched status in step across sources",
                "Marking something watched, or finishing it, also marks its copies on your other sources (matched by IMDb / TMDB / TVDB id, never by title alone). Server copies only change when server changes are on; resume positions also need Sync watch progress. Off by default.",
                onoff(self.sync_watched),
            ),
            r(
                "Trakt account",
                "Connect your own Trakt app: you create it at trakt.tv/oauth/applications and enter its client id, then sign in in your browser. NOCTURNE ships no Trakt credentials. Trakt is a trademark of Trakt, Inc.; NOCTURNE is not affiliated with Trakt.",
                "Set up".into(),
            ),
            r(
                "Trakt: scrobble what you watch",
                "Tells Trakt when you start, pause and finish a movie or episode that has IMDb / TMDB / TVDB ids. Needs the account above.",
                onoff(self.trakt_scrobble),
            ),
            r(
                "Trakt: import watched history",
                "Marks local items you have watched on Trakt as watched here (and server items, only when server changes are on). Runs when you connect and at launch. Needs the account above.",
                onoff(self.trakt_history),
            ),
            r(
                "Default profile",
                "Opens at launch without asking. Ask every time shows Who's watching. Needs two or more profiles on this server.",
                self.profiles.labels()[self.profiles.current()].clone(),
            ),
            r(
                "Couch mode",
                "A layout for the sofa and a TV: larger text and targets, a clear focus ring and button hints, driven by a gamepad, remote or keyboard. Automatic turns it on while a gamepad is connected or the window fills a large display.",
                COUCH
                    .iter()
                    .find(|(m, _)| *m == self.couch)
                    .map_or("Off", |(_, n)| n)
                    .into(),
            ),
            r(
                "Check for updates",
                "Once a day NOCTURNE asks nocturne.ifsvivek.in whether a newer version exists and says so here. It sends nothing but the version number (and, unavoidably, your IP address). Never downloads or installs anything by itself.",
                onoff(self.check_updates),
            ),
            r(
                "Themes may load from the web",
                "A theme or a server's own style can link to other stylesheets and fonts. When on, NOCTURNE downloads them once (https only, no cookies or sign-in, size and time limits, kept on disk) and again only when you press Refresh or the text changes. Off: nothing is downloaded and those parts are ignored.",
                onoff(self.remote_css),
            ),
            r(
                "Rotate the home banner",
                "The large banner at the top of Home moves to the next title by itself. It always stops while you hover or focus it, while a video plays, and when reduce motion is on.",
                onoff(self.hero_autoplay),
            ),
            r(
                "Banner interval",
                "How long each banner title stays before the next one.",
                fmt_secs(self.hero_interval_secs),
            ),
            r(
                "Player",
                "Where video plays. In-window is the default. VLC and mpv window open that program's own window and are offered only when installed; NOCTURNE keeps your place, progress and the controls it can. A source that needs extra request headers plays in-window. NOCTURNE_VIDEO overrides this.",
                self.player.label().into(),
            ),
            r(
                "Coming this week on Home",
                "Adds a row to Home with the episodes airing in the next seven days from your servers and, when Trakt is connected, your Trakt calendar. The Calendar page in the header is always there. Off by default.",
                onoff(self.home_coming),
            ),
            r(
                "Seerr server",
                "Connect a Seerr (or Jellyseerr / Overseerr) server to request movies and shows that are not in your library. You sign in with your own Jellyfin account on that server: Quick Connect, or username and password. NOCTURNE keeps only the session in your system keyring, never the password.",
                match (self.seerr.url.is_empty(), self.seerr.user_name.is_empty()) {
                    (true, _) => "Not set up".into(),
                    (false, true) => "Signed out".into(),
                    (false, false) => self.seerr.user_name.clone(),
                },
            ),
            r(
                "Allow requests",
                crate::requests::ALLOW_HINT,
                onoff(self.seerr_requests),
            ),
            r(
                "Requests without a Seerr server",
                "Keep your own list of requested movies and shows on this computer. A request turns Available by itself when the title shows up on any of your sources. Used only when no Seerr server is signed in; nothing leaves this computer unless you also connect Radarr or Sonarr below. Off by default.",
                onoff(self.native_requests),
            ),
            r(
                "Radarr",
                "Connect your own Radarr (address and API key) to have requested movies added to it. The key stays in your system keyring, never in settings. Never contacts your media servers.",
                arr_value(&self.radarr),
            ),
            r(
                "Sonarr",
                "Connect your own Sonarr (address and API key) to have requested shows added to it, with the seasons you pick. The key stays in your system keyring, never in settings. Never contacts your media servers.",
                arr_value(&self.sonarr),
            ),
            r(
                "Add requests to Radarr and Sonarr",
                "When on, a request made without Seerr is also added to your connected Radarr (movies) or Sonarr (shows), after you confirm it in the request dialog, and its status follows the service. Off until you turn it on; needs a connected service.",
                onoff(self.arr_requests),
            ),
            r(
                "Gateway",
                "One Jellyfin-compatible address over all your sources, for phone and TV apps. Off by default; when off nothing runs. Starts the nocturne-gateway helper while NOCTURNE is open.",
                onoff(self.gateway.enabled),
            ),
            r(
                "Allow connections from",
                "This computer only (default), your local network (private addresses only, plain HTTP), or remote over HTTPS with your own certificate and key.",
                self.gateway.binding.label().into(),
            ),
            r(
                "Gateway port",
                "8099 by default (8096 is Jellyfin's own). Other numbers can be set in settings.toml.",
                self.gateway.port.to_string(),
            ),
            r(
                "Gateway caching",
                "Disk cache for images and small metadata answers, never for video. None by default.",
                fmt_gw_cache(self.gateway.cache_mb),
            ),
            r(
                "Keep running when NOCTURNE is closed",
                "Installs a systemd user service so the gateway starts at login and survives closing the window. Linux only; off removes the service.",
                if keep_running_supported() {
                    onoff(self.gateway.keep_running)
                } else {
                    "Not available".into()
                },
            ),
            r(
                "Allow remote control",
                "Lets apps list and control players across your servers through the gateway. Commands to a player on a real server also need server changes on above. Off by default.",
                onoff(self.gateway.remote_control),
            ),
            r(
                "Per-server library views",
                "Show each server's libraries next to the combined Movies, Shows and Music views. Off by default.",
                onoff(self.gateway.per_server_views),
            ),
            r(
                "Announce on the local network",
                "Answer the Jellyfin apps' automatic discovery (UDP 7359). Only in local network mode.",
                onoff(self.gateway.announce),
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
            7 => (
                HdrMode::ALL.iter().map(|m| m.label().to_string()).collect(),
                HdrMode::ALL.iter().position(|m| *m == self.hdr)?,
            ),
            8 => nums(&AHEAD_S, self.buffer_ahead_secs, fmt_secs),
            9 => nums(&REFILL_S, self.buffer_refill_secs, fmt_secs),
            10 => nums(&BUF_MB, self.buffer_max_mb, fmt_gib),
            14 => (
                DENSITIES.iter().map(|d| format!("{d:?}")).collect(),
                DENSITIES.iter().position(|d| *d == self.density)?,
            ),
            15 => nums(&CACHE_MB, self.image_cache_mb, fmt_mb),
            16 => (
                MOTION.iter().map(|(_, n)| n.to_string()).collect(),
                MOTION.iter().position(|(m, _)| *m == self.reduce_motion)?,
            ),
            HERO_INTERVAL_ROW => nums(&HERO_S, self.hero_interval_secs, fmt_secs),
            GATEWAY_BINDING_ROW => (
                BINDINGS.iter().map(|b| b.label().to_string()).collect(),
                BINDINGS.iter().position(|b| *b == self.gateway.binding)?,
            ),
            GATEWAY_PORT_ROW => nums(&GATEWAY_PORTS, self.gateway.port.into(), |p| p.to_string()),
            GATEWAY_CACHE_ROW => nums(&GATEWAY_CACHE_MB, self.gateway.cache_mb, fmt_gw_cache),
            PLAYER_ROW => {
                let o = self.player_options();
                (
                    o.iter().map(|p| p.label().to_string()).collect(),
                    o.iter().position(|p| *p == self.player)?,
                )
            }
            COUCH_ROW => (
                COUCH.iter().map(|(_, n)| n.to_string()).collect(),
                COUCH.iter().position(|(m, _)| *m == self.couch)?,
            ),
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
            7 => self.hdr = HdrMode::ALL.get(idx).copied().unwrap_or(self.hdr),
            8 => {
                self.buffer_ahead_secs = AHEAD_S.get(idx).copied().unwrap_or(self.buffer_ahead_secs)
            }
            9 => {
                self.buffer_refill_secs = REFILL_S
                    .get(idx)
                    .copied()
                    .unwrap_or(self.buffer_refill_secs)
            }
            10 => self.buffer_max_mb = BUF_MB.get(idx).copied().unwrap_or(self.buffer_max_mb),
            14 => self.density = DENSITIES.get(idx).copied().unwrap_or(self.density),
            15 => self.image_cache_mb = CACHE_MB.get(idx).copied().unwrap_or(self.image_cache_mb),
            16 => self.reduce_motion = MOTION.get(idx).map_or(self.reduce_motion, |(m, _)| *m),
            HERO_INTERVAL_ROW => {
                self.hero_interval_secs =
                    HERO_S.get(idx).copied().unwrap_or(self.hero_interval_secs)
            }
            COUCH_ROW => self.couch = COUCH.get(idx).map_or(self.couch, |(m, _)| *m),
            GATEWAY_BINDING_ROW => {
                self.gateway.binding = BINDINGS.get(idx).copied().unwrap_or(self.gateway.binding)
            }
            GATEWAY_PORT_ROW => {
                if let Some(p) = GATEWAY_PORTS.get(idx) {
                    self.gateway.port = *p as u16;
                }
            }
            GATEWAY_CACHE_ROW => {
                self.gateway.cache_mb = GATEWAY_CACHE_MB
                    .get(idx)
                    .copied()
                    .unwrap_or(self.gateway.cache_mb)
            }
            PLAYER_ROW => {
                self.player = self
                    .player_options()
                    .get(idx)
                    .copied()
                    .unwrap_or(self.player)
            }
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
            7 => {
                let i = HdrMode::ALL.iter().position(|m| *m == self.hdr);
                let i = (i.unwrap_or(0) as i32 + d).rem_euclid(HdrMode::ALL.len() as i32) as usize;
                self.hdr = HdrMode::ALL[i];
            }
            8 => self.buffer_ahead_secs = step(&AHEAD_S, self.buffer_ahead_secs, 3, d),
            9 => self.buffer_refill_secs = step(&REFILL_S, self.buffer_refill_secs, 2, d),
            10 => self.buffer_max_mb = step(&BUF_MB, self.buffer_max_mb, 3, d),
            11 => self.home_resume ^= true,
            12 => self.home_next_up ^= true,
            13 => self.home_latest ^= true,
            14 => {
                use Density::*;
                self.density = match (self.density, d) {
                    (Compact, 1) | (Spacious, -1) => Comfortable,
                    (Comfortable, 1) | (Compact, _) => Spacious,
                    _ => Compact,
                }
            }
            15 => self.image_cache_mb = step(&CACHE_MB, self.image_cache_mb, 2, d),
            16 => {
                let i = MOTION.iter().position(|(m, _)| *m == self.reduce_motion);
                let i = (i.unwrap_or(0) as i32 + d).rem_euclid(MOTION.len() as i32) as usize;
                self.reduce_motion = MOTION[i].0;
            }
            17 => self.remote_control ^= true,
            18 => self.sync_watched ^= true,
            19 => {} // opens the Trakt dialog (the app handles it)
            20 => self.trakt_scrobble ^= true,
            21 => self.trakt_history ^= true,
            UPDATES_ROW => self.check_updates ^= true,
            REMOTE_CSS_ROW => self.remote_css ^= true,
            HERO_AUTOPLAY_ROW => self.hero_autoplay ^= true,
            HERO_INTERVAL_ROW => {
                self.hero_interval_secs = step(&HERO_S, self.hero_interval_secs, 2, d)
            }
            COMING_ROW => self.home_coming ^= true,
            COUCH_ROW => {
                let i = COUCH.iter().position(|(m, _)| *m == self.couch);
                let i = (i.unwrap_or(0) as i32 + d).rem_euclid(COUCH.len() as i32) as usize;
                self.couch = COUCH[i].0;
            }
            PLAYER_ROW => {
                let o = self.player_options();
                let i = o.iter().position(|p| *p == self.player).unwrap_or(0) as i32;
                self.player = o[(i + d).rem_euclid(o.len() as i32) as usize];
            }
            SEERR_ROW => {} // opens the Seerr dialog (the app handles it)
            SEERR_ALLOW_ROW => self.seerr_requests ^= true,
            NATIVE_ROW => self.native_requests ^= true,
            RADARR_ROW | SONARR_ROW => {} // open the connect dialog (the app handles them)
            ARR_ALLOW_ROW => self.arr_requests ^= true,
            GATEWAY_ROW => self.gateway.enabled ^= true,
            GATEWAY_BINDING_ROW => {
                let i = BINDINGS.iter().position(|b| *b == self.gateway.binding);
                let i = (i.unwrap_or(0) as i32 + d).rem_euclid(BINDINGS.len() as i32) as usize;
                self.gateway.binding = BINDINGS[i];
            }
            GATEWAY_PORT_ROW => {
                self.gateway.port = step(&GATEWAY_PORTS, self.gateway.port.into(), 0, d) as u16
            }
            GATEWAY_CACHE_ROW => {
                self.gateway.cache_mb = step(&GATEWAY_CACHE_MB, self.gateway.cache_mb, 0, d)
            }
            GATEWAY_KEEP_ROW if keep_running_supported() => self.gateway.keep_running ^= true,
            GATEWAY_REMOTE_ROW => self.gateway.remote_control ^= true,
            GATEWAY_VIEWS_ROW => self.gateway.per_server_views ^= true,
            GATEWAY_ANNOUNCE_ROW => self.gateway.announce ^= true,
            _ => {}
        }
    }

    /// Players the row offers right now: the installed ones, plus the stored choice if its program
    /// has gone (so the row still shows it; playing it falls back with a notice).
    fn player_options(&self) -> Vec<PlayerPref> {
        let mut o = Installed::detect().options();
        if !o.contains(&self.player) {
            o.push(self.player);
        }
        o
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
            hdr: self.hdr,
            player: self.player,
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
        Self::load_checked().0
    }

    /// Like [`Settings::load`], plus the one-line notice to show when the file could not be read (a copy of it was kept).
    pub fn load_checked() -> (Self, Option<String>) {
        match Self::default_path() {
            Some(p) => Self::load_report(&p),
            None => (Self::default(), None),
        }
    }

    /// Missing, unreadable or corrupt file gives defaults; an older version is stamped current
    /// (new fields take their defaults, so "migration" is just that).
    pub fn load_from(path: &Path) -> Self {
        Self::load_report(path).0
    }

    /// A file that exists but does not parse (a hand-edit with a duplicate key, say) is never silently dropped: a copy is kept as `settings.toml.bad-<unix time>`, a repaired parse (a repeated key keeps its last value) is used when there is one, and the notice says where the copy is.
    pub fn load_report(path: &Path) -> (Self, Option<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (Self::migrated(Self::default()), None);
        };
        let (mut s, bad) = match toml::from_str::<Self>(&text) {
            Ok(s) => (s, false),
            Err(_) => (
                toml::from_str(&drop_repeated_keys(&text)).unwrap_or_default(),
                true,
            ),
        };
        s.migrate();
        if !bad {
            return (s, None);
        }
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let name = format!(
            "{}.bad-{when}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("settings.toml")
        );
        let copy = path.with_file_name(name);
        let note = match std::fs::copy(path, &copy) {
            Ok(_) => format!(
                "Your settings file could not be read; a copy was kept at {}.",
                copy.display()
            ),
            Err(_) => "Your settings file could not be read and no copy could be made.".into(),
        };
        (s, Some(note))
    }

    fn migrated(mut s: Self) -> Self {
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

/// The text with every repeated `key =` line (within one `[table]`) reduced to its last occurrence: TOML rejects the file otherwise, and the later line is what a hand-editor meant.
fn drop_repeated_keys(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut table = String::new();
    let mut last: std::collections::HashMap<(String, String), usize> = Default::default();
    let key_of = |l: &str| {
        let t = l.trim_start();
        (!t.starts_with('#') && !t.starts_with('['))
            .then(|| t.split_once('=').map(|(k, _)| k.trim().to_string()))
            .flatten()
    };
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with('[') {
            table = l.trim().to_string();
        } else if let Some(k) = key_of(l) {
            last.insert((table.clone(), k), i);
        }
    }
    table.clear();
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with('[') {
            table = l.trim().to_string();
        } else if let Some(k) = key_of(l)
            && last.get(&(table.clone(), k)) != Some(&i)
        {
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
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
    fn couch_mode_is_off_by_default_and_automatic_follows_pad_or_tv() {
        let mut s = Settings::default();
        assert_eq!(s.couch, CouchMode::Off);
        assert!(!s.couch.active(true, true));
        s.cycle(COUCH_ROW, 1);
        assert_eq!(s.couch, CouchMode::On);
        assert!(s.couch.active(false, false));
        s.choose(COUCH_ROW, 2);
        assert_eq!(s.couch, CouchMode::Auto);
        assert!(!s.couch.active(false, false));
        assert!(s.couch.active(true, false) && s.couch.active(false, true));
        assert_eq!(s.choices(COUCH_ROW).unwrap().1, 2);
        assert_eq!(s.rows()[COUCH_ROW].value, "Automatic");
    }

    #[test]
    fn defaults_write_nothing() {
        let s = Settings::default();
        assert!(!s.allow_server_writes && !s.sync_progress && !s.remote_control);
        assert!(!s.sync_watched && !s.trakt_scrobble && !s.trakt_history);
        assert!(!s.seerr_requests && s.seerr == SeerrCfg::default());
        assert!(s.autoplay_next && !s.auto_skip_intro);
        assert_eq!(s.rows().len(), ROWS);
    }

    #[test]
    fn gateway_rows_are_last_and_off_by_default() {
        let mut s = Settings::default();
        assert!(!s.gateway.enabled && !s.gateway.remote_control && !s.gateway.keep_running);
        assert_eq!(GATEWAY_ANNOUNCE_ROW, ROWS - 1);
        let rows = s.rows();
        assert_eq!(rows.len(), ROWS);
        assert_eq!(rows[GATEWAY_ROW].value, "Off");
        assert_eq!(rows[GATEWAY_BINDING_ROW].value, "This computer only");
        assert_eq!(rows[GATEWAY_PORT_ROW].value, "8099");
        assert_eq!(rows[GATEWAY_CACHE_ROW].value, "None");
        assert_eq!(head(GATEWAY_ROW), Some("Gateway"));
        s.cycle(GATEWAY_ROW, 1);
        s.cycle(GATEWAY_REMOTE_ROW, 1);
        s.cycle(GATEWAY_VIEWS_ROW, 1);
        s.cycle(GATEWAY_ANNOUNCE_ROW, 1);
        assert!(s.gateway.enabled && s.gateway.remote_control && s.gateway.per_server_views);
        assert!(!s.gateway.announce);
        s.cycle(GATEWAY_BINDING_ROW, 1);
        assert_eq!(s.gateway.binding, Binding::Lan);
        s.choose(GATEWAY_BINDING_ROW, 2);
        assert_eq!(s.gateway.binding, Binding::Remote);
        s.cycle(GATEWAY_PORT_ROW, 1);
        assert_eq!(s.gateway.port, 8100);
        s.choose(GATEWAY_CACHE_ROW, 2);
        assert_eq!(s.gateway.cache_mb, 1024);
        assert_eq!(s.rows()[GATEWAY_CACHE_ROW].value, "1 GiB");
        // A hand-edited port stays visible and selectable as its own entry.
        s.gateway.port = 9000;
        let (labels, cur) = s.choices(GATEWAY_PORT_ROW).unwrap();
        assert_eq!((labels[cur].as_str(), labels.len()), ("9000", 5));
        assert_eq!(s.choices(GATEWAY_ROW), None);
        // Saved in the [gateway] table the gateway process reads; an older file without it keeps the defaults.
        let t = toml::to_string(&s).unwrap();
        assert!(t.contains("[gateway]") && t.contains("binding = \"remote\""));
        assert_eq!(
            toml::from_str::<Settings>("version = 2").unwrap().gateway,
            GatewaySettings::default()
        );
    }

    #[test]
    fn coming_row_is_last_and_off_by_default() {
        let mut s = Settings::default();
        assert!(!s.home_coming);
        assert_eq!(s.rows().len(), ROWS);
        assert_eq!(s.rows()[COMING_ROW].value, "Off");
        s.cycle(COMING_ROW, 1);
        assert!(s.home_coming);
        assert_eq!(head(COMING_ROW), Some("Calendar"));
        // An older settings file has no such key and keeps Home as it was.
        assert!(
            !toml::from_str::<Settings>("version = 1")
                .unwrap()
                .home_coming
        );
    }

    #[test]
    fn update_check_row_toggles_and_defaults_on() {
        let mut s = Settings::default();
        assert!(s.check_updates);
        assert_eq!(s.rows()[UPDATES_ROW].value, "On");
        s.cycle(UPDATES_ROW, 1);
        assert!(!s.check_updates);
        assert_eq!(s.rows()[UPDATES_ROW].value, "Off");
    }

    #[test]
    fn watched_and_trakt_rows_toggle_and_stay_off_by_default() {
        let mut s = Settings::default();
        let before = s.clone();
        s.cycle(TRAKT_ROW, 1); // opens a dialog, changes nothing
        assert_eq!(s, before);
        s.cycle(18, 1);
        s.cycle(20, 1);
        s.cycle(21, 1);
        assert!(s.sync_watched && s.trakt_scrobble && s.trakt_history);
        assert_eq!(s.rows()[PROFILE_ROW].label, "Default profile");
        assert_eq!(s.rows()[TRAKT_ROW].label, "Trakt account");
    }

    #[test]
    fn seerr_rows_open_a_dialog_and_toggle_and_keep_no_secret() {
        let mut s = Settings::default();
        assert_eq!(s.rows()[SEERR_ROW].value, "Not set up");
        assert_eq!(s.rows()[SEERR_ALLOW_ROW].value, "Off");
        let before = s.clone();
        s.cycle(SEERR_ROW, 1); // opens a dialog, changes nothing
        assert_eq!(s, before);
        s.cycle(SEERR_ALLOW_ROW, 1);
        assert!(
            s.seerr_requests && !s.allow_server_writes,
            "its own switch, not the server-changes one"
        );
        s.seerr = SeerrCfg {
            url: "https://seerr.example".into(),
            user_id: 2,
            user_name: "alice".into(),
            permissions: 32,
        };
        assert_eq!(s.rows()[SEERR_ROW].value, "alice");
        let d = dir("seerr");
        let p = d.join("settings.toml");
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        let text = std::fs::read_to_string(&p).unwrap().to_lowercase();
        assert!(
            !text.contains("cookie") && !text.contains("password") && !text.contains("connect.sid")
        );
        std::fs::write(&p, "version = 3\n").unwrap();
        assert_eq!(
            Settings::load_from(&p).seerr,
            SeerrCfg::default(),
            "old files: not set up"
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn native_request_and_arr_rows_toggle_and_keep_no_key() {
        let mut s = Settings::default();
        assert!(!s.native_requests && !s.arr_requests);
        assert_eq!(s.rows()[NATIVE_ROW].value, "Off");
        assert_eq!(s.rows()[RADARR_ROW].value, "Not set up");
        assert_eq!(s.rows()[SONARR_ROW].value, "Not set up");
        let before = s.clone();
        s.cycle(RADARR_ROW, 1); // open dialogs, change nothing
        s.cycle(SONARR_ROW, 1);
        assert_eq!(s, before);
        s.cycle(NATIVE_ROW, 1);
        s.cycle(ARR_ALLOW_ROW, 1);
        assert!(s.native_requests && s.arr_requests && !s.allow_server_writes);
        s.radarr = ArrCfg {
            url: "http://radarr.lan:7878".into(),
            profile_id: 4,
            profile_name: "HD".into(),
            root_folder: "/movies".into(),
        };
        assert_eq!(s.rows()[RADARR_ROW].value, "Connected");
        let d = dir("arr");
        let p = d.join("settings.toml");
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        let text = std::fs::read_to_string(&p).unwrap().to_lowercase();
        assert!(!text.contains("apikey") && !text.contains("api_key"));
        std::fs::write(&p, "version = 3\n").unwrap();
        let old = Settings::load_from(&p);
        assert!(!old.native_requests && !old.arr_requests && old.radarr == ArrCfg::default());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn onboarding_defaults_to_never_and_persists() {
        let d = dir("onb");
        let p = d.join("nocturne").join("settings.toml");
        assert_eq!(Settings::default().onboarding, 0);
        let s = Settings {
            onboarding: ONBOARDING,
            ..Settings::default()
        };
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p).onboarding, ONBOARDING);
        std::fs::write(&p, "version = 2\n").unwrap();
        assert_eq!(Settings::load_from(&p).onboarding, 0, "old files: never");
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
        s.cycle(14, -1);
        s.cycle(8, 1);
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
    fn a_duplicate_key_keeps_a_copy_repairs_and_says_so() {
        let d = dir("dup");
        let p = d.join("settings.toml");
        std::fs::create_dir_all(&d).unwrap();
        let text =
            "version = 3\nautoplay_next = false\nautoplay_next = true\nauto_skip_intro = true\n";
        std::fs::write(&p, text).unwrap();
        let (s, note) = Settings::load_report(&p);
        // The repeated key keeps its last value and the rest of the file survives.
        assert!(s.autoplay_next && s.auto_skip_intro);
        let note = note.expect("a notice");
        assert!(note.contains("settings.toml.bad-"), "{note}");
        let kept: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bad-"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), text);
        // The original is untouched until something is saved.
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        // A good file gives no notice.
        std::fs::write(&p, "version = 3\n").unwrap();
        assert!(Settings::load_report(&p).1.is_none());
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
        s.cycle(8, 1);
        s.cycle(10, -1);
        let b = s.buffering();
        assert_eq!((b.high_secs, b.max_bytes), (600.0, 1024 << 20));
        s.buffer_ahead_secs = 77; // unknown value (hand-edited file) starts from the default slot
        s.cycle(8, 1);
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
        s.cycle(14, 1);
        assert_eq!(s.density, Density::Spacious);
        s.cycle(14, 1);
        assert_eq!(s.density, Density::Compact);
        s.cycle(99, 1); // out of range is a no-op
        s.cycle(15, 1);
        assert_eq!(s.image_cache_mb, 1024);
        s.cycle(15, -1);
        s.cycle(15, -1);
        assert_eq!(s.image_cache_mb, 256);
    }

    #[test]
    fn choices_list_every_option_and_mark_the_current_one() {
        let mut s = Settings::default();
        assert!(s.choices(0).is_none() && s.choices(13).is_none() && s.choices(99).is_none());
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
        s.choose(14, 2);
        assert_eq!(s.density, Density::Spacious);
        let (l, at) = s.choices(10).unwrap();
        assert_eq!((l[at].as_str(), l.len()), ("1.5 GiB", 7));
        s.choose(8, 5);
        assert_eq!(s.buffer_ahead_secs, 900);
        s.choose(15, 99); // out of range: no change
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
        let (l, at) = s.choices(8).unwrap();
        assert_eq!((l.len(), l[at].as_str()), (7, "77 s"));
        s.choose(8, at);
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
    fn reduce_motion_follows_system_unless_forced() {
        let mut s = Settings::default();
        assert_eq!(s.reduce_motion, ReduceMotion::System);
        assert!(s.reduce_motion.reduced(true) && !s.reduce_motion.reduced(false));
        s.cycle(16, 1);
        assert!(s.reduce_motion.reduced(false), "On forces it");
        s.cycle(16, 1);
        assert!(!s.reduce_motion.reduced(true), "Off forces motion");
        s.cycle(16, 1);
        assert_eq!(s.reduce_motion, ReduceMotion::System);
        s.choose(16, 1);
        assert_eq!(s.rows()[16].value, "On");
        assert_eq!(s.choices(16).unwrap().1, 1);
        // Older files without the field load as System.
        let old: Settings = toml::from_str("version = 2\n").unwrap();
        assert_eq!(old.reduce_motion, ReduceMotion::System);
    }

    #[test]
    fn hero_rows_default_cycle_and_persist() {
        let mut s = Settings::default();
        assert!(s.hero_autoplay && s.hero_interval_secs == 10);
        assert_eq!(s.rows().len(), ROWS);
        assert_eq!(head(HERO_AUTOPLAY_ROW), Some("Home banner"));
        assert_eq!(s.rows()[HERO_INTERVAL_ROW].value, "10 s");
        s.cycle(HERO_INTERVAL_ROW, 1);
        assert_eq!(s.hero_interval_secs, 15);
        s.choose(HERO_INTERVAL_ROW, 0);
        assert_eq!(s.hero_interval_secs, 5);
        s.cycle(HERO_INTERVAL_ROW, -1);
        assert_eq!(s.hero_interval_secs, 30, "wraps");
        s.cycle(HERO_AUTOPLAY_ROW, 1);
        assert!(!s.hero_autoplay);
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert!(!back.hero_autoplay && back.hero_interval_secs == 30);
        let old: Settings = toml::from_str("version = 2\n").unwrap();
        assert!(old.hero_autoplay && old.hero_interval_secs == 10);
    }

    #[test]
    fn remote_css_row_is_on_by_default_and_persists() {
        let mut s = Settings::default();
        assert!(s.remote_css);
        assert_eq!(s.rows().len(), ROWS);
        assert_eq!(s.rows()[REMOTE_CSS_ROW].value, "On");
        assert_eq!(head(REMOTE_CSS_ROW), Some("Themes"));
        s.cycle(REMOTE_CSS_ROW, 1);
        assert!(!s.remote_css && s.rows()[REMOTE_CSS_ROW].value == "Off");
        assert!(toml::to_string(&s).unwrap().contains("remote_css = false"));
        // Older files have no such field: they load with it on.
        let old: Settings = toml::from_str("version = 2\n").unwrap();
        assert!(old.remote_css);
    }

    #[test]
    fn hdr_video_row_cycles_and_persists() {
        let mut s = Settings::default();
        assert_eq!(s.hdr, HdrMode::Ask);
        assert_eq!(s.rows()[7].label, "HDR video");
        assert_eq!(s.rows()[7].value, "Ask");
        s.cycle(7, 1);
        assert_eq!(s.hdr, HdrMode::Window);
        s.cycle(7, 1);
        assert_eq!(s.prefs().hdr, HdrMode::Popout);
        s.cycle(7, 1);
        assert_eq!(s.hdr, HdrMode::Ask, "wraps");
        s.cycle(7, -1);
        assert_eq!(s.hdr, HdrMode::Popout);
        let (l, at) = s.choices(7).unwrap();
        assert_eq!((l.len(), at), (3, 2));
        s.choose(7, 1);
        assert_eq!(s.hdr, HdrMode::Window);
        s.choose(7, 9); // out of range: no change
        assert_eq!(s.hdr, HdrMode::Window);
        // An older file has no such field: it loads as Ask.
        let old: Settings = toml::from_str("version = 2\n").unwrap();
        assert_eq!(old.hdr, HdrMode::Ask);
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("hdr = \"window\""));
    }

    #[test]
    fn player_row_cycles_installed_players_and_persists() {
        let mut s = Settings::default();
        assert_eq!(s.player, PlayerPref::InWindow);
        assert_eq!(s.rows()[PLAYER_ROW].label, "Player");
        assert_eq!(s.rows()[PLAYER_ROW].value, "In-window");
        assert_eq!(s.rows().len(), ROWS);
        let have = Installed::detect().options();
        // Cycling only ever lands on an installed player and wraps around.
        for _ in 0..have.len() + 1 {
            s.cycle(PLAYER_ROW, 1);
            assert!(have.contains(&s.player));
        }
        let (labels, at) = s.choices(PLAYER_ROW).unwrap();
        assert_eq!(labels.len(), have.len());
        assert_eq!(labels[at], s.player.label());
        s.choose(PLAYER_ROW, 99); // out of range: no change
        s.choose(PLAYER_ROW, 0);
        assert_eq!(s.player, PlayerPref::InWindow);
        // A stored choice whose program is gone is still shown (and plays with a notice).
        s.player = PlayerPref::Mpv;
        assert!(
            s.choices(PLAYER_ROW)
                .unwrap()
                .0
                .contains(&"mpv window".to_string())
        );
        assert_eq!(s.prefs().player, PlayerPref::Mpv);
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("player = \"mpv\""));
        let old: Settings = toml::from_str("version = 2\n").unwrap();
        assert_eq!(old.player, PlayerPref::InWindow);
    }

    #[test]
    fn scroll_and_focus_logic() {
        // Child 0 is the title; heads sit before rows 0, 2, 11, 14, 17, 18, 19, 22, 23, 24, 25, 26, 28, 29, 30, 32 and 36.
        assert_eq!(scroll_child(0), 2);
        assert_eq!(scroll_child(1), 3);
        assert_eq!(scroll_child(2), 5);
        assert_eq!(scroll_child(11), 15);
        assert_eq!(scroll_child(ROWS - 1), 1 + ROWS - 1 + 17);
        assert!((1..ROWS).all(|i| scroll_child(i) > scroll_child(i - 1)));
        assert_eq!(nav_row(0, "up"), Some(0));
        assert_eq!(nav_row(ROWS - 1, "down"), Some(ROWS - 1));
        assert_eq!(nav_row(3, "pagedown"), Some(8));
        assert_eq!(nav_row(ROWS - 3, "pagedown"), Some(ROWS - 1));
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
