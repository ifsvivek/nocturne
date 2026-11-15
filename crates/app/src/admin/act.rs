//! Every admin action: its confirmation dialog (plain-language, naming the exact target) and
//! the write it performs. `exec` is only reachable with a `Confirmed` token.
use super::confirm::{Dialog, Field};
use nocturne_jellyfin_admin::{AdminClient, ConfigPatch, Confirmed, Result};

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
    ToggleQuickConnect(bool),
    Restart,
    Shutdown,
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
            ToggleQuickConnect(on) => d("Quick Connect", if *on { "Allow signing in with Quick Connect codes." } else { "Stop allowing Quick Connect sign-in." }.into(), "Save", false),
            Shutdown => d("Shut down server", "The server stops now and stays off until someone starts it again on the machine it runs on. Everything playing stops.".into(), "Shut down server", true),
            Restart => d("Restart server", "The server restarts now. Everything playing stops and clients reconnect.".into(), "Restart server", true),
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
        ToggleQuickConnect(on) => {
            c.set_server_config(&ConfigPatch {
                quick_connect_available: Some(on),
                ..Default::default()
            })
            .await
        }
        Restart => c.restart_server(ok).await,
        Shutdown => c.shutdown_server(ok).await,
        ViewLog(_) => Ok(()),
    }
}
