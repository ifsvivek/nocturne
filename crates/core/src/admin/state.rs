//! Pure admin state: section/selection, loaded data, stale-response guard and poll scheduling.
//! No GPUI here, so it is unit-tested without a window.
use crate::nav::{self, Dir};
use nocturne_jellyfin_admin::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Dashboard,
    General,
    Branding,
    Users,
    Access,
    Libraries,
    Providers,
    Metadata,
    Resume,
    Streaming,
    Transcoding,
    Trickplay,
    Devices,
    Activity,
    Plugins,
    Networking,
    ApiKeys,
    Backups,
    Logs,
    Tasks,
}

/// Sidebar order; keyboard Up/Down walks this list.
pub const ALL: [Section; 20] = [
    Section::Dashboard,
    Section::General,
    Section::Branding,
    Section::Users,
    Section::Access,
    Section::Libraries,
    Section::Providers,
    Section::Metadata,
    Section::Resume,
    Section::Streaming,
    Section::Transcoding,
    Section::Trickplay,
    Section::Devices,
    Section::Activity,
    Section::Plugins,
    Section::Networking,
    Section::ApiKeys,
    Section::Backups,
    Section::Logs,
    Section::Tasks,
];

pub enum Row {
    Group(&'static str),
    /// (section, indented under the previous entry)
    Item(Section, bool),
}

use Row::*;
use Section as S;
pub const SIDEBAR: [Row; 24] = [
    Group("Server"),
    Item(S::Dashboard, false),
    Item(S::General, false),
    Item(S::Branding, false),
    Item(S::Users, false),
    Item(S::Access, true),
    Item(S::Libraries, false),
    Item(S::Providers, true),
    Item(S::Metadata, true),
    Item(S::Resume, true),
    Item(S::Streaming, true),
    Item(S::Transcoding, true),
    Item(S::Trickplay, true),
    Group("Devices"),
    Item(S::Devices, false),
    Item(S::Activity, false),
    Group("Plugins"),
    Item(S::Plugins, false),
    Group("Advanced"),
    Item(S::Networking, false),
    Item(S::ApiKeys, false),
    Item(S::Backups, false),
    Item(S::Logs, false),
    Item(S::Tasks, false),
];

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Dashboard => "Dashboard",
            Section::General => "General",
            Section::Branding => "Branding",
            Section::Users => "Users",
            Section::Access => "Access and limits",
            Section::Libraries => "Libraries",
            Section::Providers => "Providers",
            Section::Metadata => "Metadata",
            Section::Resume => "Resume",
            Section::Streaming => "Streaming",
            Section::Transcoding => "Transcoding",
            Section::Trickplay => "Trickplay",
            Section::Backups => "Backups",
            Section::Devices => "Devices",
            Section::Activity => "Activity",
            Section::Plugins => "Plugins",
            Section::Networking => "Networking",
            Section::ApiKeys => "API Keys",
            Section::Logs => "Logs",
            Section::Tasks => "Scheduled Tasks",
        }
    }
}

/// Admin section is offered only when the admin probe (`system_info`) succeeded.
/// 401/403 (token without admin rights) and any other failure hide it.
pub fn admin_visible<T>(probe: &Result<T>) -> bool {
    probe.is_ok()
}

#[derive(Default)]
pub enum L<T> {
    #[default]
    Loading,
    Ready(T),
    Failed,
}

impl<T> L<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            L::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// A failed refresh keeps the previous good value; only a first load can become `Failed`.
    fn set(&mut self, v: Option<T>) {
        match v {
            Some(v) => *self = L::Ready(v),
            None if matches!(self, L::Loading) => *self = L::Failed,
            None => {}
        }
    }
}

/// Display preferences as readable lines: scalar fields, then the client's custom ones.
pub fn pretty_prefs(v: &serde_json::Value) -> String {
    let mut out = vec![];
    let show = |x: &serde_json::Value| x.as_str().map_or_else(|| x.to_string(), str::to_string);
    if let Some(o) = v.as_object() {
        for (k, x) in o.iter().filter(|(k, x)| {
            !x.is_null() && !x.is_object() && !matches!(k.as_str(), "Id" | "Client")
        }) {
            out.push(format!("{k}: {}", show(x)));
        }
        for (k, x) in v["CustomPrefs"].as_object().into_iter().flatten() {
            if !x.is_null() {
                out.push(format!("custom {k}: {}", show(x)));
            }
        }
    }
    if out.is_empty() {
        "No display preferences saved.".into()
    } else {
        out.truncate(40);
        out.join("\n")
    }
}

/// A library together with the providers the server offers for its content type.
pub struct LibProviders {
    pub folder: VirtualFolder,
    pub avail: AvailableOptions,
}

/// Every library with what the server offers for it (one `AvailableOptions` read per content type).
pub async fn library_providers(c: &AdminClient) -> Result<Vec<LibProviders>> {
    let mut seen: Vec<(Option<String>, AvailableOptions)> = vec![];
    let mut out = vec![];
    for folder in c.virtual_folders().await? {
        let hit = seen.iter().find(|(t, _)| *t == folder.collection_type);
        let avail = match hit {
            Some((_, a)) => a.clone(),
            None => {
                let a = c
                    .available_library_options(folder.collection_type.as_deref())
                    .await?;
                seen.push((folder.collection_type.clone(), a.clone()));
                a
            }
        };
        out.push(LibProviders { folder, avail });
    }
    Ok(out)
}

#[derive(Default)]
pub struct Data {
    pub system: L<SystemInfo>,
    pub activity: L<ActivityPage>,
    pub sessions: L<Vec<Session>>,
    pub devices: L<Vec<Device>>,
    pub tasks: L<Vec<ScheduledTask>>,
    pub folders: L<Vec<VirtualFolder>>,
    pub users: L<Vec<User>>,
    pub plugins: L<Vec<Plugin>>,
    pub packages: L<Vec<Package>>,
    pub repos: L<Vec<Repository>>,
    pub ratings: L<Vec<ParentalRating>>,
    /// Last viewed display preferences: (user id, formatted text).
    pub prefs: Option<(String, String)>,
    pub providers: L<Vec<LibProviders>>,
    pub logs: L<Vec<LogFile>>,
    pub branding: L<Branding>,
    pub config: L<ServerConfig>,
    pub keys: L<Vec<ApiKey>>,
    pub counts: L<ItemCounts>,
    pub network: L<NetworkConfig>,
    pub encoding: L<serde_json::Value>,
    pub backups: L<Vec<serde_json::Value>>,
    /// Last viewed log: (file name, already trimmed text).
    pub log: Option<(String, String)>,
}

#[allow(clippy::large_enum_variant)] // short-lived message, boxing buys nothing
/// One finished read; `None` = the request failed.
pub enum Msg {
    System(Option<SystemInfo>),
    Activity(Option<ActivityPage>),
    Sessions(Option<Vec<Session>>),
    Devices(Option<Vec<Device>>),
    Tasks(Option<Vec<ScheduledTask>>),
    Folders(Option<Vec<VirtualFolder>>),
    Users(Option<Vec<User>>),
    Plugins(Option<Vec<Plugin>>),
    Packages(Option<Vec<Package>>),
    Repos(Option<Vec<Repository>>),
    Ratings(Option<Vec<ParentalRating>>),
    Prefs(String, Option<String>),
    Providers(Option<Vec<LibProviders>>),
    Logs(Option<Vec<LogFile>>),
    Branding(Option<Branding>),
    Config(Option<ServerConfig>),
    Keys(Option<Vec<ApiKey>>),
    Counts(Option<ItemCounts>),
    Network(Option<NetworkConfig>),
    Encoding(Option<serde_json::Value>),
    Backups(Option<Vec<serde_json::Value>>),
    Log(String, Option<String>),
}

impl Data {
    fn apply(&mut self, m: Msg) {
        match m {
            Msg::System(v) => self.system.set(v),
            Msg::Activity(v) => self.activity.set(v),
            Msg::Sessions(v) => self.sessions.set(v),
            Msg::Devices(v) => self.devices.set(v),
            Msg::Tasks(v) => self.tasks.set(v),
            Msg::Folders(v) => self.folders.set(v),
            Msg::Users(v) => self.users.set(v),
            Msg::Plugins(v) => self.plugins.set(v),
            Msg::Packages(v) => self.packages.set(v),
            Msg::Repos(v) => self.repos.set(v),
            Msg::Ratings(v) => self.ratings.set(v),
            Msg::Prefs(id, text) => {
                if let Some(t) = text {
                    self.prefs = Some((id, t));
                }
            }
            Msg::Providers(v) => self.providers.set(v),
            Msg::Logs(v) => self.logs.set(v),
            Msg::Branding(v) => self.branding.set(v),
            Msg::Config(v) => self.config.set(v),
            Msg::Keys(v) => self.keys.set(v),
            Msg::Counts(v) => self.counts.set(v),
            Msg::Network(v) => self.network.set(v),
            Msg::Encoding(v) => self.encoding.set(v),
            Msg::Backups(v) => self.backups.set(v),
            Msg::Log(name, text) => {
                if let Some(t) = text {
                    self.log = Some((name, t));
                }
            }
        }
    }
}

/// What the current screen offers the keyboard.
#[derive(Clone, Copy, Default)]
pub struct Shape {
    pub items: usize,
    pub toolbar: usize,
    /// Buttons on the focused row (toolbar buttons when `State::on_toolbar`).
    pub acts: usize,
}

pub struct State {
    pub section: Section,
    pub open: bool,
    /// Bumped whenever the visible page changes; late responses for an older page are dropped.
    pub pid: u64,
    /// Bumped on every enter/leave; a poll loop dies when its epoch is stale.
    pub epoch: u64,
    pub in_side: bool,
    pub on_toolbar: bool,
    pub sel: usize,
    pub act: usize,
    pub data: Data,
}

pub const POLL_SECS: u64 = 5;

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            section: Section::Dashboard,
            open: false,
            pid: 0,
            epoch: 0,
            in_side: true,
            on_toolbar: false,
            sel: 0,
            act: 0,
            data: Data::default(),
        }
    }

    pub fn enter(&mut self, s: Section) {
        self.section = s;
        self.open = true;
        self.pid += 1;
        self.epoch += 1;
        self.sel = 0;
        self.act = 0;
        self.on_toolbar = false;
    }

    pub fn leave(&mut self) {
        self.open = false;
        self.pid += 1;
        self.epoch += 1;
    }

    /// Apply a response unless it belongs to a page the user already left.
    pub fn accept(&mut self, pid: u64, m: Msg) -> bool {
        if !self.open || pid != self.pid {
            return false;
        }
        self.data.apply(m);
        true
    }

    /// Whether the poll loop started at `epoch` should keep going.
    pub fn polling(&self, epoch: u64) -> bool {
        self.open && epoch == self.epoch && self.section == Section::Dashboard
    }

    pub fn clamp(&mut self, items: usize) {
        self.sel = self.sel.min(items.saturating_sub(1));
    }

    /// Arrow-key reducer. Sidebar on the left, then an optional toolbar row, then the items.
    pub fn nav(&mut self, d: Dir, s: Shape) {
        if self.in_side {
            match d {
                Dir::Up | Dir::Down => {
                    let i = ALL.iter().position(|x| *x == self.section).unwrap_or(0);
                    let n = ALL[nav::step(i, 1, ALL.len(), d)];
                    if n != self.section {
                        self.enter(n);
                    }
                }
                Dir::Right if s.items + s.toolbar > 0 => {
                    self.in_side = false;
                    self.on_toolbar = s.toolbar > 0;
                    self.sel = 0;
                    self.act = 0;
                }
                _ => {}
            }
            return;
        }
        match d {
            Dir::Up if self.on_toolbar => {}
            Dir::Up if self.sel > 0 => self.sel -= 1,
            Dir::Up if s.toolbar > 0 => self.on_toolbar = true,
            Dir::Up => {}
            Dir::Down if self.on_toolbar => {
                self.on_toolbar = false;
                self.sel = 0;
            }
            Dir::Down => self.sel = (self.sel + 1).min(s.items.saturating_sub(1)),
            Dir::Left if self.act == 0 => {
                (self.in_side, self.on_toolbar) = (true, false);
                return;
            }
            Dir::Left | Dir::Right => {
                self.act = nav::step(self.act, s.acts.max(1), s.acts, d);
                return;
            }
        }
        self.act = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Dir::*;

    fn shape(items: usize, toolbar: usize) -> Shape {
        Shape {
            items,
            toolbar,
            acts: 3,
        }
    }

    #[test]
    fn prefs_are_flattened_to_lines() {
        let v = serde_json::json!({"Id":"x","Client":"emby","SortBy":"SortName","ShowBackdrop":true,
            "Gone":null,"CustomPrefs":{"skipForwardLength":"30000","tvhome":null}});
        assert_eq!(
            pretty_prefs(&v),
            "SortBy: SortName\nShowBackdrop: true\ncustom skipForwardLength: 30000"
        );
        assert_eq!(
            pretty_prefs(&serde_json::json!({})),
            "No display preferences saved."
        );
    }

    #[test]
    fn visibility_needs_a_successful_probe() {
        let st = |status| {
            Err::<(), _>(Error::Status {
                status,
                path: "/".into(),
            })
        };
        assert!(admin_visible(&Ok(())));
        assert!(!admin_visible(&st(401)));
        assert!(!admin_visible(&st(403)));
        assert!(!admin_visible(&st(500)));
    }

    #[test]
    fn stale_pages_are_dropped() {
        let mut s = State::new();
        s.enter(Section::Users);
        let old = s.pid;
        s.enter(Section::Tasks);
        assert!(!s.accept(old, Msg::Users(Some(vec![]))));
        assert!(matches!(s.data.users, L::Loading));
        assert!(s.accept(s.pid, Msg::Tasks(Some(vec![]))));
        s.leave();
        assert!(
            !s.accept(s.pid, Msg::Tasks(None)),
            "closed admin ignores replies"
        );
    }

    #[test]
    fn failed_refresh_keeps_old_data() {
        let mut d = Data::default();
        d.apply(Msg::Users(None));
        assert!(matches!(d.users, L::Failed));
        d.apply(Msg::Users(Some(vec![])));
        d.apply(Msg::Users(None));
        assert!(d.users.get().is_some());
    }

    #[test]
    fn poll_runs_only_on_visible_dashboard() {
        let mut s = State::new();
        s.enter(Section::Dashboard);
        let e = s.epoch;
        assert!(s.polling(e));
        s.enter(Section::Users);
        assert!(!s.polling(e), "left the dashboard");
        s.enter(Section::Dashboard);
        assert!(!s.polling(e), "old loop is superseded by the new one");
        let e2 = s.epoch;
        s.leave();
        assert!(!s.polling(e2), "closed");
    }

    #[test]
    fn keyboard_flow() {
        let mut s = State::new();
        s.enter(Section::Libraries);
        s.nav(Down, shape(2, 1));
        assert_eq!(s.section, Section::Providers);
        s.nav(Right, shape(2, 1));
        assert!(!s.in_side && s.on_toolbar);
        s.nav(Down, shape(2, 1));
        assert!(!s.on_toolbar && s.sel == 0);
        s.nav(Down, shape(2, 1));
        s.nav(Down, shape(2, 1));
        assert_eq!(s.sel, 1);
        s.nav(Right, shape(2, 1));
        assert_eq!(s.act, 1);
        s.nav(Left, shape(2, 1));
        s.nav(Left, shape(2, 1));
        assert!(s.in_side, "Left at the first button returns to the sidebar");
        // empty section: Right stays in the sidebar
        s.nav(Right, shape(0, 0));
        assert!(s.in_side);
        // Up from the first row goes to the toolbar, and stops there
        s.nav(Right, shape(2, 1));
        s.nav(Down, shape(2, 1));
        s.nav(Up, shape(2, 1));
        assert!(s.on_toolbar);
        s.nav(Up, shape(2, 1));
        assert!(s.on_toolbar && !s.in_side);
    }

    #[test]
    fn sidebar_lists_every_section_in_order() {
        let items: Vec<Section> = SIDEBAR
            .iter()
            .filter_map(|r| {
                if let Row::Item(s, _) = r {
                    Some(*s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(items, ALL);
    }
}
