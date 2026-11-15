//! Every WRITE lives here. Names start with a verb; destructive ones take [`Confirmed`].

use crate::client::AdminClient;
use crate::{ApiKey, Confirmed, Error, Result, User, UserPolicy};
use reqwest::Method;
use serde_json::json;

/// Selected server configuration fields (`None` = leave unchanged).
#[derive(Debug, Clone, Default)]
pub struct ConfigPatch {
    pub server_name: Option<String>,
    pub ui_culture: Option<String>,
    pub log_file_retention_days: Option<i32>,
    pub activity_log_retention_days: Option<i32>,
    pub quick_connect_available: Option<bool>,
    pub min_resume_pct: Option<i32>,
    pub max_resume_pct: Option<i32>,
    pub min_resume_duration_seconds: Option<i32>,
    pub remote_client_bitrate_limit: Option<i32>,
    pub preferred_metadata_language: Option<String>,
    pub metadata_country_code: Option<String>,
}

impl AdminClient {
    /// WRITE `POST /ScheduledTasks/Running/{id}`.
    pub async fn start_task(&self, id: &str) -> Result<()> {
        self.send(Method::POST, &["ScheduledTasks", "Running", id], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `DELETE /ScheduledTasks/Running/{id}`.
    pub async fn stop_task(&self, id: &str) -> Result<()> {
        self.send(
            Method::DELETE,
            &["ScheduledTasks", "Running", id],
            &[],
            None,
        )
        .await
        .map(drop)
    }

    /// WRITE `POST /Library/Refresh`: scan every library.
    pub async fn refresh_library(&self) -> Result<()> {
        self.send(Method::POST, &["Library", "Refresh"], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `POST /Items/{id}/Refresh`: scan one library (`VirtualFolder::item_id`).
    pub async fn refresh_library_folder(&self, item_id: &str) -> Result<()> {
        let p = [
            ("metadataRefreshMode", "Default".into()),
            ("imageRefreshMode", "Default".into()),
            ("recursive", "true".into()),
        ];
        self.send(Method::POST, &["Items", item_id, "Refresh"], &p, None)
            .await
            .map(drop)
    }

    /// WRITE `POST /Users/New`.
    pub async fn create_user(&self, name: &str, password: &str) -> Result<User> {
        self.send(
            Method::POST,
            &["Users", "New"],
            &[],
            Some(json!({"Name": name, "Password": password})),
        )
        .await?
        .json()
        .await
        .map_err(crate::client::http_err)
    }

    /// WRITE `POST /Users/{id}/Policy` (replaces the whole policy; get it from `user()` first).
    pub async fn set_user_policy(&self, id: &str, policy: &UserPolicy) -> Result<()> {
        let body = serde_json::to_value(policy).expect("policy serializes");
        self.send(Method::POST, &["Users", id, "Policy"], &[], Some(body))
            .await
            .map(drop)
    }

    /// WRITE read-modify-write of the policy: enable/disable the account.
    pub async fn set_user_disabled(&self, id: &str, disabled: bool) -> Result<()> {
        let mut p = self.user(id).await?.policy;
        p.is_disabled = disabled;
        self.set_user_policy(id, &p).await
    }

    /// WRITE read-modify-write of the policy: grant/revoke the administrator flag.
    pub async fn set_user_admin(&self, id: &str, admin: bool) -> Result<()> {
        let mut p = self.user(id).await?.policy;
        p.is_administrator = admin;
        self.set_user_policy(id, &p).await
    }

    /// WRITE `POST /Users/{id}/Password`: DESTRUCTIVE, overwrites the user's password.
    pub async fn set_user_password(
        &self,
        id: &str,
        new_password: &str,
        _: Confirmed,
    ) -> Result<()> {
        let body = json!({"CurrentPw": "", "NewPw": new_password});
        self.send(Method::POST, &["Users", id, "Password"], &[], Some(body))
            .await
            .map(drop)
    }

    /// WRITE `POST /Users/{id}/Password` with `ResetPassword`: DESTRUCTIVE, clears the password.
    pub async fn reset_user_password(&self, id: &str, _: Confirmed) -> Result<()> {
        let body = json!({"ResetPassword": true});
        self.send(Method::POST, &["Users", id, "Password"], &[], Some(body))
            .await
            .map(drop)
    }

    /// WRITE `DELETE /Users/{id}`: DESTRUCTIVE.
    pub async fn delete_user(&self, id: &str, _: Confirmed) -> Result<()> {
        self.send(Method::DELETE, &["Users", id], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `POST /Sessions/{id}/Message`: show a message on the client (needs remote-control support).
    pub async fn send_session_message(
        &self,
        id: &str,
        header: &str,
        text: &str,
        timeout_ms: u32,
    ) -> Result<()> {
        let body = json!({"Header": header, "Text": text, "TimeoutMs": timeout_ms});
        self.send(Method::POST, &["Sessions", id, "Message"], &[], Some(body))
            .await
            .map(drop)
    }

    /// WRITE `POST /Sessions/{id}/Playing/Stop`: stop what the session is playing. Jellyfin has no
    /// "kill session"; to cut a client off for good, `delete_device` revokes its tokens.
    pub async fn stop_session_playback(&self, id: &str) -> Result<()> {
        self.send(
            Method::POST,
            &["Sessions", id, "Playing", "Stop"],
            &[],
            None,
        )
        .await
        .map(drop)
    }

    /// WRITE `DELETE /Devices?id=`: DESTRUCTIVE, revokes the device's tokens and sessions.
    pub async fn delete_device(&self, device_id: &str, _: Confirmed) -> Result<()> {
        self.send(
            Method::DELETE,
            &["Devices"],
            &[("id", device_id.into())],
            None,
        )
        .await
        .map(drop)
    }

    /// WRITE `POST /Auth/Keys?app=`: create an API key, then return it (found by re-listing).
    pub async fn create_api_key(&self, app: &str) -> Result<ApiKey> {
        self.send(
            Method::POST,
            &["Auth", "Keys"],
            &[("app", app.into())],
            None,
        )
        .await?;
        self.api_keys()
            .await?
            .into_iter()
            .filter(|k| k.app_name == app)
            .max_by(|a, b| a.date_created.cmp(&b.date_created))
            .ok_or(Error::Status {
                status: 404,
                path: "/Auth/Keys".into(),
            })
    }

    /// WRITE `DELETE /Auth/Keys/{key}`: DESTRUCTIVE (anything using the key loses access).
    pub async fn delete_api_key(&self, access_token: &str, _: Confirmed) -> Result<()> {
        self.send(Method::DELETE, &["Auth", "Keys", access_token], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `POST /System/Configuration`: read-modify-write of the selected fields only.
    pub async fn set_server_config(&self, patch: &ConfigPatch) -> Result<()> {
        let mut c = self.server_config().await?;
        macro_rules! apply { ($($f:ident),*) => { $(if patch.$f.is_some() { c.$f = patch.$f.clone(); })* } }
        apply!(
            server_name,
            ui_culture,
            log_file_retention_days,
            activity_log_retention_days,
            quick_connect_available,
            min_resume_pct,
            max_resume_pct,
            min_resume_duration_seconds,
            remote_client_bitrate_limit,
            preferred_metadata_language,
            metadata_country_code
        );
        let body = serde_json::to_value(&c).expect("config serializes");
        self.send(Method::POST, &["System", "Configuration"], &[], Some(body))
            .await
            .map(drop)
    }

    /// WRITE `POST /System/Restart`: DESTRUCTIVE, restarts the server process (drops all streams).
    /// The only process-level call here; there is deliberately no shutdown.
    pub async fn restart_server(&self, _: Confirmed) -> Result<()> {
        self.send(Method::POST, &["System", "Restart"], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `POST /System/Shutdown`: DESTRUCTIVE, stops the server process; it stays down until
    /// started again from outside (there is no remote start).
    pub async fn shutdown_server(&self, _: Confirmed) -> Result<()> {
        self.send(Method::POST, &["System", "Shutdown"], &[], None)
            .await
            .map(drop)
    }
}
