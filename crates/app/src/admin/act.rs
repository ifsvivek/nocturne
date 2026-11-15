//! Every admin action: its confirmation dialog (plain-language, naming the exact target) and
//! the write it performs. `exec` is only reachable with a `Confirmed` token.
use super::confirm::{Dialog, Field};
use nocturne_jellyfin_admin::{
    AdminClient, ConfigPatch, Confirmed, Error, NetworkEdit, Package, Result, TaskTrigger, edit,
};
use serde_json::{Value, json};

/// (id, display name) of the target.
pub type Tgt = (String, String);

/// Server configuration fields editable from the UI.
#[derive(Clone, Copy)]
pub enum Cfg {
    Name,
    Culture,
    LogDays,
    ActivityDays,
    MetaLang,
    MetaCountry,
    ResumeMin,
    ResumeMax,
    ResumeSecs,
    BitrateLimit,
}

impl Cfg {
    /// (dialog title, explanation, input label, numeric)
    fn text(self) -> (&'static str, &'static str, &'static str, bool) {
        match self {
            Cfg::Name => (
                "Server name",
                "Change the name this server shows to clients.",
                "Server name",
                false,
            ),
            Cfg::Culture => (
                "Display language",
                "Change the server's default display language (for example en-US).",
                "Language code",
                false,
            ),
            Cfg::LogDays => (
                "Log retention",
                "Keep server log files for this many days.",
                "Days",
                true,
            ),
            Cfg::ActivityDays => (
                "Activity retention",
                "Keep the activity log for this many days.",
                "Days",
                true,
            ),
            Cfg::MetaLang => (
                "Metadata language",
                "Preferred language when downloading metadata (for example en).",
                "Language code",
                false,
            ),
            Cfg::MetaCountry => (
                "Metadata country",
                "Preferred country for ratings and release dates (for example US).",
                "Country code",
                false,
            ),
            Cfg::ResumeMin => (
                "Minimum resume position",
                "An item counts as started after this percentage has been watched.",
                "Percent",
                true,
            ),
            Cfg::ResumeMax => (
                "Maximum resume position",
                "An item counts as finished after this percentage has been watched.",
                "Percent",
                true,
            ),
            Cfg::ResumeSecs => (
                "Minimum resume duration",
                "Shorter videos never offer to resume. Seconds.",
                "Seconds",
                true,
            ),
            Cfg::BitrateLimit => (
                "Remote streaming limit",
                "Maximum bitrate for clients outside the local network, in bits per second. 0 means no limit.",
                "Bits per second",
                true,
            ),
        }
    }

    fn patch(self, v: &str) -> ConfigPatch {
        let n = v.parse::<i32>().ok();
        let s = Some(v.to_string());
        let mut p = ConfigPatch::default();
        match self {
            Cfg::Name => p.server_name = s,
            Cfg::Culture => p.ui_culture = s,
            Cfg::LogDays => p.log_file_retention_days = n,
            Cfg::ActivityDays => p.activity_log_retention_days = n,
            Cfg::MetaLang => p.preferred_metadata_language = s,
            Cfg::MetaCountry => p.metadata_country_code = s,
            Cfg::ResumeMin => p.min_resume_pct = n,
            Cfg::ResumeMax => p.max_resume_pct = n,
            Cfg::ResumeSecs => p.min_resume_duration_seconds = n,
            Cfg::BitrateLimit => p.remote_client_bitrate_limit = n,
        }
        p
    }
}

/// Which part of the network configuration an action edits (the safe subset only).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Net {
    HttpPort,
    PublicHttpPort,
    HttpsPort,
    PublicHttpsPort,
    Bind,
    Lan,
    Remote,
    Upnp,
    Discovery,
    Ipv4,
    Ipv6,
}

impl Net {
    pub fn label(self) -> &'static str {
        match self {
            Net::HttpPort => "HTTP port",
            Net::PublicHttpPort => "Public HTTP port",
            Net::HttpsPort => "HTTPS port",
            Net::PublicHttpsPort => "Public HTTPS port",
            Net::Bind => "Bind addresses",
            Net::Lan => "LAN networks",
            Net::Remote => "Remote access",
            Net::Upnp => "UPnP",
            Net::Discovery => "Auto discovery",
            Net::Ipv4 => "IPv4",
            Net::Ipv6 => "IPv6",
        }
    }

    fn edit(self, v: &str, on: bool) -> Result<NetworkEdit> {
        let port = || {
            v.parse::<u16>()
                .map_err(|_| Error::Invalid("port must be 1 to 65535".into()))
        };
        Ok(match self {
            Net::HttpPort => NetworkEdit::HttpPort(port()?),
            Net::PublicHttpPort => NetworkEdit::PublicHttpPort(port()?),
            Net::HttpsPort => NetworkEdit::HttpsPort(port()?),
            Net::PublicHttpsPort => NetworkEdit::PublicHttpsPort(port()?),
            Net::Bind => NetworkEdit::BindAddresses(edit::split_list(v)),
            Net::Lan => NetworkEdit::LanNetworks(edit::split_list(v)),
            Net::Remote => NetworkEdit::RemoteAccess(on),
            Net::Upnp => NetworkEdit::Upnp(on),
            Net::Discovery => NetworkEdit::AutoDiscovery(on),
            Net::Ipv4 => NetworkEdit::Ipv4(on),
            Net::Ipv6 => NetworkEdit::Ipv6(on),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrigKind {
    Daily,
    Weekly,
    Interval,
    Startup,
}

impl TrigKind {
    pub fn of(t: &TaskTrigger) -> Option<Self> {
        Some(match t.kind.as_str() {
            "DailyTrigger" => Self::Daily,
            "WeeklyTrigger" => Self::Weekly,
            "IntervalTrigger" => Self::Interval,
            "StartupTrigger" => Self::Startup,
            _ => return None,
        })
    }

    fn build(self, v: &[String]) -> Result<TaskTrigger> {
        match self {
            Self::Daily => TaskTrigger::daily(&v[0]),
            Self::Weekly => TaskTrigger::weekly(&v[0], &v[1]),
            Self::Interval => TaskTrigger::every_minutes(v[0].parse().unwrap_or(0)),
            Self::Startup => Ok(TaskTrigger::on_startup()),
        }
    }

    /// Input fields for this trigger type, prefilled from `cur` when editing.
    fn fields(self, cur: Option<&TaskTrigger>) -> Vec<Field> {
        let time = cur
            .and_then(|t| t.time_of_day_ticks)
            .map(edit::format_time_of_day)
            .unwrap_or_default();
        let time = Field::new("Time of day (24 h, HH:MM)", &time)
            .checked(|s| edit::parse_time_of_day(s).is_some());
        match self {
            Self::Daily => vec![time],
            Self::Weekly => vec![
                Field::new(
                    "Day of the week",
                    cur.and_then(|t| t.day_of_week.as_deref()).unwrap_or(""),
                )
                .checked(|s| edit::day_name(s).is_some()),
                time,
            ],
            Self::Interval => {
                let m = cur
                    .and_then(|t| t.interval_ticks)
                    .map(|t| (t / edit::TICKS_PER_SEC / 60).to_string())
                    .unwrap_or_default();
                let mut f = Field::new("Every how many minutes", &m)
                    .checked(|s| s.parse::<i64>().is_ok_and(|n| n >= 1));
                f.numeric = true;
                vec![f]
            }
            Self::Startup => vec![],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Interval => "interval",
            Self::Startup => "startup",
        }
    }
}

/// How a trickplay setting is typed.
#[derive(Clone, Copy)]
pub enum Tp {
    Num,
    List,
    Scan,
    Priority,
}

const SCAN: [&str; 2] = ["Blocking", "NonBlocking"];
const PRIORITY: [&str; 5] = ["High", "AboveNormal", "Normal", "BelowNormal", "Idle"];

fn canon(l: &[&'static str], s: &str) -> Option<&'static str> {
    l.iter().copied().find(|x| x.eq_ignore_ascii_case(s.trim()))
}

impl Tp {
    fn field(self, cur: &str) -> Field {
        let f = Field::new("Value", cur);
        match self {
            Tp::Num => {
                let mut f = f.checked(|s| s.parse::<i64>().is_ok());
                f.numeric = true;
                f
            }
            Tp::List => f.checked(|s| {
                let l = edit::split_list(s);
                !l.is_empty() && l.iter().all(|x| x.parse::<i64>().is_ok())
            }),
            Tp::Scan => f.checked(|s| canon(&SCAN, s).is_some()),
            Tp::Priority => f.checked(|s| canon(&PRIORITY, s).is_some()),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Tp::Num => "",
            Tp::List => " Separate several widths with commas.",
            Tp::Scan => " Blocking or NonBlocking.",
            Tp::Priority => " High, AboveNormal, Normal, BelowNormal or Idle.",
        }
    }

    fn value(self, v: &str) -> Value {
        match self {
            Tp::Num => json!(v.parse::<i64>().unwrap_or(-1)),
            Tp::List => json!(
                edit::split_list(v)
                    .iter()
                    .map(|x| x.parse::<i64>().unwrap_or(-1))
                    .collect::<Vec<_>>()
            ),
            Tp::Scan => json!(canon(&SCAN, v).unwrap_or("")),
            Tp::Priority => json!(canon(&PRIORITY, v).unwrap_or("")),
        }
    }
}

/// Branding text fields (empty clears them).
#[derive(Clone, Copy)]
pub enum Brand {
    Disclaimer,
    Css,
}

fn is_library_type(s: &str) -> bool {
    edit::LIBRARY_TYPES.contains(&s.to_lowercase().as_str())
}

fn is_name(s: &str) -> bool {
    edit::validate_library_name(s).is_ok()
}

fn is_path(s: &str) -> bool {
    edit::validate_library_path(s).is_ok()
}

fn is_address_list(s: &str) -> bool {
    edit::split_list(s)
        .iter()
        .all(|x| x.parse::<std::net::IpAddr>().is_ok())
}

fn is_network_list(s: &str) -> bool {
    edit::split_list(s).iter().all(|x| edit::valid_network(x))
}

fn is_port(s: &str) -> bool {
    s.parse::<u16>().is_ok_and(|p| p > 0)
}

#[derive(Clone)] // no Debug: DeleteKey carries an access token
pub enum Act {
    CreateUser,
    Disable(Tgt, bool),
    MakeAdmin(Tgt, bool),
    SetPassword(Tgt),
    ResetPassword(Tgt),
    DeleteUser(Tgt),
    RefreshAll,
    RefreshFolder(Tgt),
    StartTask(Tgt),
    StopTask(Tgt),
    Message(Tgt),
    StopPlayback(Tgt),
    DeleteDevice(Tgt),
    CreateKey,
    DeleteKey(Tgt),
    EditCfg(Cfg, String),
    /// Transcoding setting: (JSON key, label, numeric, current value).
    EditEnc(&'static str, String, bool, String),
    ToggleQuickConnect(bool),
    Restart,
    Shutdown,
    /// Branding: edit the login message or custom CSS (current value).
    EditBrand(Brand, String),
    ToggleSplash(bool),
    /// Network value edit (current value as text). Needs the typed confirmation.
    EditNet(Net, String),
    /// Network on/off setting: (which, new value).
    NetFlag(Net, bool),
    AddTrigger(Tgt, TrigKind),
    /// (task, index, current trigger)
    EditTrigger(Tgt, usize, TaskTrigger),
    /// (task, index, description)
    DeleteTrigger(Tgt, usize, String),
    /// Trickplay value: (JSON key, label, kind, current).
    EditTrickplay(&'static str, String, Tp, String),
    ToggleTrickplay(&'static str, String, bool),
    AddLibrary,
    /// (library id, name)
    AddLibPath(Tgt),
    /// (library, path)
    RemoveLibPath(Tgt, String),
    RemoveLibrary(Tgt),
    /// (library, option key, label, new value)
    LibOption(Tgt, &'static str, &'static str, bool),
    LibRefreshDays(Tgt, String),
    /// (plugin id, version, name, enable)
    PluginEnable(String, String, String, bool),
    PluginUninstall(String, String, String),
    PluginInstall(Box<Package>),
    RepoAdd,
    /// (url, name, enable)
    RepoEnable(String, String, bool),
    /// (url, name)
    RepoRemove(String, String),
    /// Read-only: fetch a log tail (no dialog).
    ViewLog(String),
}

fn text(label: &'static str, v: &str) -> Field {
    Field::new(label, v)
}

impl Act {
    pub fn dialog(&self) -> Dialog {
        use Act::*;
        let d = |t: &str, b: String, ok: &'static str, danger: bool| {
            Dialog::new(self.clone(), t.into(), b, ok, danger)
        };
        let dlg = match self {
            CreateUser => d("Create user", "Add a new account to this server. Leave the password empty to create it without one.".into(), "Create", false)
                .field(text("Username", ""))
                .field({ let mut f = text("Password (optional)", ""); f.secret = true; f.optional = true; f }),
            Disable((_, n), to) => if *to {
                d("Disable user", format!("{n} will no longer be able to sign in. Their data is kept."), "Disable", false)
            } else {
                d("Enable user", format!("{n} will be able to sign in again."), "Enable", false)
            },
            MakeAdmin((_, n), to) => if *to {
                d("Grant administrator", format!("{n} will be able to manage this whole server."), "Grant", false)
            } else {
                d("Remove administrator", format!("{n} will no longer be able to manage this server."), "Remove", false)
            },
            SetPassword((_, n)) => d("Set password", format!("Choose a new password for {n}. The old one stops working."), "Set password", false)
                .field({ let mut f = text("New password", ""); f.secret = true; f }),
            ResetPassword((_, n)) => d("Reset password", format!("The password of {n} will be cleared. They can sign in without one until a new one is set."), "Reset", true),
            DeleteUser((_, n)) => {
                let mut f = text("Type the username to confirm", "");
                f.must_equal = Some(n.clone());
                d("Delete user", format!("This permanently deletes the account {n} and its watch history. It cannot be undone."), "Delete user", true).field(f)
            }
            RefreshAll => d("Refresh all libraries", "Scan every library for new and changed media.".into(), "Refresh", false),
            RefreshFolder((_, n)) => d("Refresh library", format!("Scan the library {n} for new and changed media."), "Refresh", false),
            StartTask((_, n)) => d("Run task", format!("Start the scheduled task {n} now."), "Run", false),
            StopTask((_, n)) => d("Stop task", format!("Stop the running task {n}."), "Stop", false),
            Message((_, n)) => d("Send message", format!("Show a message on {n}."), "Send", false)
                .field(text("Message", "")),
            StopPlayback((_, n)) => d("Stop playback", format!("Stop what is playing on {n}."), "Stop playback", false),
            DeleteDevice((_, n)) => d("Remove device", format!("{n} will be signed out and must sign in again."), "Remove device", true),
            CreateKey => d("Create API key", "Create a new access key for an application.".into(), "Create", false)
                .field(text("Application name", "")),
            DeleteKey((_, n)) => d("Delete API key", format!("Anything using the key for {n} loses access immediately."), "Delete key", true),
            EditCfg(c, v) => {
                let (t, b, label, numeric) = c.text();
                let mut f = text(label, v);
                f.numeric = numeric;
                d(t, b.into(), "Save", false).field(f)
            }
            EditEnc(_, label, numeric, v) => {
                let mut f = text("Value", v);
                f.numeric = *numeric;
                d(label, "Change this transcoding setting. It applies to new streams.".into(), "Save", false).field(f)
            }
            ToggleQuickConnect(on) => d("Quick Connect", if *on { "Allow signing in with Quick Connect codes." } else { "Stop allowing Quick Connect sign-in." }.into(), "Save", false),
            Shutdown => d("Shut down server", "The server stops now and stays off until someone starts it again on the machine it runs on. Everything playing stops.".into(), "Shut down server", true),
            Restart => d("Restart server", "The server restarts now. Everything playing stops and clients reconnect.".into(), "Restart server", true),
            EditBrand(b, v) => {
                let (t, body) = match b {
                    Brand::Disclaimer => ("Login message", "Text shown on the sign-in page of every client. Leave it empty to remove it."),
                    Brand::Css => ("Custom CSS", "Style rules the web client applies. Line breaks become spaces here. Leave it empty to remove them."),
                };
                let mut f = text("Text", &v.replace(['\r', '\n'], " "));
                f.optional = true;
                d(t, body.into(), "Save", false).field(f)
            }
            ToggleSplash(on) => d("Splash screen", if *on { "Show the custom splash screen." } else { "Stop showing the custom splash screen." }.into(), "Save", false),
            EditNet(n, v) => {
                let mut f = text("New value", v);
                match n {
                    Net::Bind => { f = f.checked(is_address_list); f.optional = true; }
                    Net::Lan => { f = f.checked(is_network_list); f.optional = true; }
                    _ => { f = f.checked(is_port); f.numeric = true; }
                }
                let hint = match n {
                    Net::Bind => " Separate addresses with commas. Empty means all interfaces.",
                    Net::Lan => " Addresses or ranges such as 192.168.1.0/24, separated by commas.",
                    _ => " A number from 1 to 65535.",
                };
                d(n.label(), format!("{NET_WARN}{hint}"), "Change network setting", true)
                    .field(f)
                    .field(Field::typed("Type NETWORK to confirm", "NETWORK"))
            }
            NetFlag(n, on) => d(n.label(), format!("{NET_WARN} This turns {} {}.", n.label(), if *on { "on" } else { "off" }), "Change network setting", true)
                .field(Field::typed("Type NETWORK to confirm", "NETWORK")),
            AddTrigger((_, n), k) => {
                let mut dlg = d("Add trigger", format!("Run {n} on a {} schedule.", k.name()), "Add trigger", false);
                for f in k.fields(None) { dlg = dlg.field(f); }
                dlg
            }
            EditTrigger((_, n), _, t) => {
                let k = TrigKind::of(t).unwrap_or(TrigKind::Startup);
                let mut dlg = d("Edit trigger", format!("Change when {n} runs ({}).", t.describe()), "Save", false);
                for f in k.fields(Some(t)) { dlg = dlg.field(f); }
                dlg
            }
            DeleteTrigger((_, n), _, what) => d("Delete trigger", format!("{n} will no longer run on this schedule: {what}."), "Delete trigger", true),
            EditTrickplay(_, label, k, cur) => d(label, format!("Change this trickplay setting. It applies to thumbnails generated from now on.{}", k.hint()), "Save", false).field(k.field(cur)),
            ToggleTrickplay(_, label, on) => d(label, format!("Turn this trickplay setting {}.", if *on { "on" } else { "off" }), "Save", false),
            AddLibrary => d("Add library", "Create a library from a folder on the server machine. The type decides how files are named and scanned: movies, tvshows, music, musicvideos, homevideos, boxsets, books or mixed.".into(), "Add library", false)
                .field(text("Name", "").checked(is_name))
                .field(text("Type", "movies").checked(is_library_type))
                .field(text("Folder on the server", "").checked(is_path)),
            AddLibPath((_, n)) => d("Add folder", format!("Add another folder to the library {n}."), "Add folder", false)
                .field(text("Folder on the server", "").checked(is_path)),
            RemoveLibPath((_, n), p) => d("Remove folder", format!("{n} will stop indexing {p}. The files stay where they are."), "Remove folder", true),
            RemoveLibrary((_, n)) => d("Remove library", format!("This removes the library {n} and its metadata from the server. Your media files are not deleted. It cannot be undone."), "Remove library", true)
                .field(Field::typed("Type the library name to confirm", n)),
            LibOption((_, n), _, label, on) => d(label, format!("Turn {label} {} for the library {n}.", if *on { "on" } else { "off" }), "Save", false),
            LibRefreshDays((_, n), v) => {
                let mut f = text("Days (0 = never)", v).checked(|s| s.parse::<i64>().is_ok_and(|n| n <= 3650));
                f.numeric = true;
                d("Automatic refresh", format!("Refresh the metadata of {n} this often."), "Save", false).field(f)
            }
            PluginEnable(_, _, n, on) => d(if *on { "Enable plugin" } else { "Disable plugin" }, format!("{n} will be {}. The change applies after the server restarts.", if *on { "enabled" } else { "disabled" }), if *on { "Enable" } else { "Disable" }, false),
            PluginUninstall(_, _, n) => d("Uninstall plugin", format!("{n} will be removed after the server restarts, together with anything it provides."), "Uninstall", true)
                .field(Field::typed("Type the plugin name to confirm", n)),
            PluginInstall(p) => d("Install plugin", format!("The server downloads {} {} from its plugin repository. It loads after the server restarts.", p.name, p.versions.first().map_or("", |v| &v.version)), "Install", false),
            RepoAdd => d("Add plugin repository", "The server will download plugins from this address, and plugins run with full access to the server. Only add a repository you trust.".into(), "Add repository", false)
                .field(text("Name", "").checked(|s| !s.trim().is_empty()))
                .field(text("Manifest address (http or https)", "https://").checked(|s| edit::validate_repository("x", s).is_ok())),
            RepoEnable(_, n, on) => d(if *on { "Enable repository" } else { "Disable repository" }, format!("{n} will {} the plugin catalog.", if *on { "be used for" } else { "no longer be used for" }), if *on { "Enable" } else { "Disable" }, false),
            RepoRemove(_, n) => d("Remove repository", format!("{n} is removed from the list and its plugins leave the catalog. Plugins already installed stay installed."), "Remove repository", true),
            ViewLog(_) => unreachable!("log views have no dialog"),
        };
        dlg.start()
    }

    /// Calm toast text on success / failure.
    pub fn done(&self, ok: bool) -> &'static str {
        use Act::*;
        match (self, ok) {
            (_, false) => "That didn't work. Check the server and try again.",
            (CreateUser, _) => "User created.",
            (DeleteUser(_), _) => "User deleted.",
            (Disable(..) | MakeAdmin(..), _) => "User updated.",
            (SetPassword(_) | ResetPassword(_), _) => "Password updated.",
            (RefreshAll | RefreshFolder(_), _) => "Refresh started.",
            (StartTask(_), _) => "Task started.",
            (StopTask(_), _) => "Task stopped.",
            (Message(_), _) => "Message sent.",
            (StopPlayback(_), _) => "Playback stopped.",
            (DeleteDevice(_), _) => "Device removed.",
            (CreateKey, _) => "API key created.",
            (DeleteKey(_), _) => "API key deleted.",
            (RepoAdd | RepoEnable(..) | RepoRemove(..), _) => "Repositories updated.",
            (PluginEnable(..) | PluginUninstall(..) | PluginInstall(_), _) => {
                "Done. Restart the server to apply it."
            }
            (EditNet(..) | NetFlag(..), _) => "Saved. Restart the server to apply it.",
            (AddTrigger(..) | EditTrigger(..) | DeleteTrigger(..), _) => "Triggers updated.",
            (AddLibrary, _) => "Library added. Scanning has started.",
            (RemoveLibrary(_), _) => "Library removed.",
            (AddLibPath(..) | RemoveLibPath(..), _) => "Folders updated.",
            (Restart, _) => "Restart requested.",
            (Shutdown, _) => "Shutdown requested.",
            _ => "Saved.",
        }
    }
}

/// Runs the confirmed write. Destructive calls need `ok`; the others ignore it.
pub async fn exec(c: AdminClient, act: Act, v: Vec<String>, ok: Confirmed) -> Result<()> {
    use Act::*;
    match act {
        CreateUser => c.create_user(&v[0], &v[1]).await.map(drop),
        Disable((id, _), to) => c.set_user_disabled(&id, to).await,
        MakeAdmin((id, _), to) => c.set_user_admin(&id, to).await,
        SetPassword((id, _)) => c.set_user_password(&id, &v[0], ok).await,
        ResetPassword((id, _)) => c.reset_user_password(&id, ok).await,
        DeleteUser((id, _)) => c.delete_user(&id, ok).await,
        RefreshAll => c.refresh_library().await,
        RefreshFolder((id, _)) => c.refresh_library_folder(&id).await,
        StartTask((id, _)) => c.start_task(&id).await,
        StopTask((id, _)) => c.stop_task(&id).await,
        Message((id, _)) => c.send_session_message(&id, "NOCTURNE", &v[0], 8000).await,
        StopPlayback((id, _)) => c.stop_session_playback(&id).await,
        DeleteDevice((id, _)) => c.delete_device(&id, ok).await,
        CreateKey => c.create_api_key(&v[0]).await.map(drop),
        DeleteKey((tok, _)) => c.delete_api_key(&tok, ok).await,
        EditCfg(f, _) => c.set_server_config(&f.patch(&v[0])).await,
        EditEnc(key, _, numeric, _) => {
            let val = if numeric {
                serde_json::json!(v[0].parse::<i64>().unwrap_or(-1))
            } else {
                serde_json::json!(v[0])
            };
            c.set_encoding_field(key, val).await
        }
        ToggleQuickConnect(on) => {
            c.set_server_config(&ConfigPatch {
                quick_connect_available: Some(on),
                ..Default::default()
            })
            .await
        }
        Restart => c.restart_server(ok).await,
        Shutdown => c.shutdown_server(ok).await,
        EditBrand(b, _) => match b {
            Brand::Disclaimer => c.set_branding(Some(&v[0]), None, None).await,
            Brand::Css => c.set_branding(None, Some(&v[0]), None).await,
        },
        ToggleSplash(on) => c.set_branding(None, None, Some(on)).await,
        EditNet(n, _) => c.set_network(&n.edit(&v[0], false)?, ok).await,
        NetFlag(n, on) => c.set_network(&n.edit("", on)?, ok).await,
        AddTrigger((id, _), k) => c.add_task_trigger(&id, k.build(&v)?).await,
        EditTrigger((id, _), i, t) => {
            let k = TrigKind::of(&t).ok_or_else(|| Error::Invalid("unknown trigger".into()))?;
            c.replace_task_trigger(&id, i, k.build(&v)?).await
        }
        DeleteTrigger((id, _), i, _) => c.delete_task_trigger(&id, i, ok).await,
        EditTrickplay(key, _, k, _) => c.set_trickplay_field(key, k.value(&v[0])).await,
        ToggleTrickplay(key, _, on) => c.set_trickplay_field(key, json!(on)).await,
        AddLibrary => c.add_library(&v[0], &v[1].to_lowercase(), &v[2]).await,
        AddLibPath((_, n)) => c.add_library_path(&n, &v[0]).await,
        RemoveLibPath((_, n), p) => c.remove_library_path(&n, &p, ok).await,
        RemoveLibrary((_, n)) => c.remove_library(&n, ok).await,
        LibOption((id, _), key, _, on) => c.set_library_option(&id, key, json!(on)).await,
        LibRefreshDays((id, _), _) => {
            let days = v[0].parse::<i64>().unwrap_or(-1);
            c.set_library_option(&id, "AutomaticRefreshIntervalDays", json!(days))
                .await
        }
        PluginEnable(id, ver, _, on) => c.set_plugin_enabled(&id, &ver, on).await,
        PluginUninstall(id, ver, _) => c.uninstall_plugin(&id, &ver, ok).await,
        PluginInstall(p) => c.install_package(&p).await,
        RepoAdd => c.add_plugin_repository(&v[0], &v[1]).await,
        RepoEnable(url, _, on) => c.set_plugin_repository_enabled(&url, on).await,
        RepoRemove(url, _) => c.remove_plugin_repository(&url, ok).await,
        ViewLog(_) => Ok(()),
    }
}

const NET_WARN: &str = "Careful: a wrong value can make this server unreachable, including from NOCTURNE, until someone fixes it on the machine the server runs on. Most network changes apply after a restart.";

#[cfg(test)]
mod tests {
    use super::*;

    fn tgt() -> Tgt {
        ("id".into(), "Name".into())
    }

    /// Type `text` into field `i` the way the keyboard would.
    fn type_into(d: &mut Dialog, i: usize, text: &str) {
        d.focus = i;
        d.fields[i].value.clear();
        for c in text.chars() {
            d.key(&c.to_string(), Some(&c.to_string()));
        }
    }

    #[test]
    fn every_network_change_needs_the_typed_word_and_is_danger() {
        let acts = [
            EditNet(Net::HttpPort, "8096".into()),
            EditNet(Net::Bind, "".into()),
            EditNet(Net::Lan, "10.0.0.0/8".into()),
            NetFlag(Net::Remote, false),
            NetFlag(Net::Ipv6, true),
        ];
        use Act::*;
        for a in acts {
            let mut d = a.dialog();
            assert!(d.danger);
            let last = d.fields.len() - 1;
            assert_eq!(d.fields[last].must_equal.as_deref(), Some("NETWORK"));
            assert!(!d.valid(), "blocked until NETWORK is typed");
            type_into(&mut d, last, "network");
            assert!(!d.valid(), "case matters");
            type_into(&mut d, last, "NETWORK");
            // value fields keep their own checks
            assert_eq!(d.valid(), d.fields[..last].iter().all(Field::ok));
        }
        let mut d = Act::EditNet(Net::HttpPort, "8096".into()).dialog();
        type_into(&mut d, 1, "NETWORK");
        type_into(&mut d, 0, "99999");
        assert!(!d.valid(), "port out of range");
        type_into(&mut d, 0, "9000");
        assert!(d.valid());
    }

    #[test]
    fn network_values_become_validated_edits() {
        assert!(
            Net::HttpPort.edit("0", false).is_ok(),
            "range is checked later, by apply"
        );
        assert!(Net::HttpPort.edit("x", false).is_err());
        assert!(
            matches!(Net::Lan.edit("10.0.0.0/8, ::1", false), Ok(NetworkEdit::LanNetworks(l)) if l.len() == 2)
        );
        assert!(matches!(
            Net::Ipv6.edit("", true),
            Ok(NetworkEdit::Ipv6(true))
        ));
    }

    #[test]
    fn trigger_dialogs_validate_input() {
        let mut d = Act::AddTrigger(tgt(), TrigKind::Weekly).dialog();
        assert_eq!(d.fields.len(), 2);
        type_into(&mut d, 0, "funday");
        type_into(&mut d, 1, "99:99");
        assert!(!d.valid());
        type_into(&mut d, 0, "Monday");
        type_into(&mut d, 1, "04:30");
        assert!(d.valid());
        let t = TrigKind::Weekly.build(&d.values()).unwrap();
        assert_eq!(t.describe(), "Monday at 04:30");
        assert!(Act::AddTrigger(tgt(), TrigKind::Startup).dialog().valid());
        let cur = TaskTrigger::every_minutes(90).unwrap();
        let d = Act::EditTrigger(tgt(), 0, cur).dialog();
        assert_eq!(d.fields[0].value, "90");
        assert!(d.valid());
        assert!(Act::DeleteTrigger(tgt(), 0, "x".into()).dialog().danger);
    }

    #[test]
    fn library_and_plugin_dialogs_gate_destructive_actions() {
        let mut d = Act::AddLibrary.dialog();
        type_into(&mut d, 0, "Films");
        type_into(&mut d, 2, "relative/path");
        assert!(!d.valid(), "relative folder refused");
        type_into(&mut d, 2, "/media/films");
        assert!(d.valid());
        type_into(&mut d, 1, "cartoons");
        assert!(!d.valid(), "unknown type refused");
        let d = Act::RemoveLibrary(tgt()).dialog();
        assert!(
            d.danger && !d.valid(),
            "removing a library needs its name typed"
        );
        let d = Act::PluginUninstall("i".into(), "1".into(), "Name".into()).dialog();
        assert!(d.danger && !d.valid());
        assert!(Act::RemoveLibPath(tgt(), "/p".into()).dialog().danger);
        let mut d = Act::EditTrickplay("ScanBehavior", "Scan".into(), Tp::Scan, "".into()).dialog();
        type_into(&mut d, 0, "blocking");
        assert!(d.valid());
        assert_eq!(Tp::Scan.value("blocking"), json!("Blocking"));
        assert_eq!(Tp::List.value("320, 640"), json!([320, 640]));
    }

    #[test]
    fn repository_dialogs() {
        let mut d = Act::RepoAdd.dialog();
        type_into(&mut d, 0, "Mine");
        type_into(&mut d, 1, "ftp://x");
        assert!(!d.valid());
        type_into(&mut d, 1, "https://r.example/manifest.json");
        assert!(d.valid() && !d.danger);
        assert!(Act::RepoRemove("u".into(), "n".into()).dialog().danger);
    }

    #[test]
    fn branding_fields_may_be_cleared() {
        let d = Act::EditBrand(Brand::Css, "a{}\nb{}".into()).dialog();
        assert!(d.fields[0].optional && d.valid());
        assert_eq!(d.fields[0].value, "a{} b{}");
    }
}
