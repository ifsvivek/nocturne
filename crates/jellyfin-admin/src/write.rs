//! Every WRITE lives here. Names start with a verb; destructive ones take [`Confirmed`].

use crate::client::AdminClient;
use crate::{
    ApiKey, Confirmed, Error, NetworkEdit, Package, Result, TaskTrigger, User, UserPolicy, edit,
};
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

    /// WRITE `POST /System/Configuration/encoding`: read-modify-write of one field.
    pub async fn set_encoding_field(&self, key: &str, value: serde_json::Value) -> Result<()> {
        let mut c = self.encoding_config().await?;
        c[key] = value;
        self.send(
            Method::POST,
            &["System", "Configuration", "encoding"],
            &[],
            Some(c),
        )
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

    /// WRITE `POST /System/Configuration/branding`: login message and custom CSS (`None` = keep).
    pub async fn set_branding(
        &self,
        login_disclaimer: Option<&str>,
        custom_css: Option<&str>,
        splashscreen: Option<bool>,
    ) -> Result<()> {
        let mut b = self.branding_raw().await?;
        if let Some(v) = login_disclaimer {
            b["LoginDisclaimer"] = v.into();
        }
        if let Some(v) = custom_css {
            b["CustomCss"] = v.into();
        }
        if let Some(v) = splashscreen {
            b["SplashscreenEnabled"] = v.into();
        }
        self.send(
            Method::POST,
            &["System", "Configuration", "branding"],
            &[],
            Some(b),
        )
        .await
        .map(drop)
    }

    /// WRITE `POST /System/Configuration/network`: DESTRUCTIVE (a wrong port, bind address or
    /// network can lock every client, this app included, out until fixed on the server machine).
    /// Validated first, then a read-modify-write of the one field; most changes need a restart.
    pub async fn set_network(&self, edit: &NetworkEdit, _: Confirmed) -> Result<()> {
        let mut c = self.network_config_raw().await?;
        edit.apply(&mut c)?;
        self.send(
            Method::POST,
            &["System", "Configuration", "network"],
            &[],
            Some(c),
        )
        .await
        .map(drop)
    }

    /// WRITE `POST /ScheduledTasks/{id}/Triggers`: the server replaces the whole list.
    async fn put_triggers(&self, id: &str, list: &[TaskTrigger]) -> Result<()> {
        list.iter().try_for_each(edit::validate_trigger)?;
        let body = serde_json::to_value(list).expect("triggers serialize");
        self.send(
            Method::POST,
            &["ScheduledTasks", id, "Triggers"],
            &[],
            Some(body),
        )
        .await
        .map(drop)
    }

    /// WRITE read-modify-write of the trigger list: append one.
    pub async fn add_task_trigger(&self, id: &str, t: TaskTrigger) -> Result<()> {
        let mut l = self.scheduled_task(id).await?.triggers;
        l.push(t);
        self.put_triggers(id, &l).await
    }

    /// WRITE read-modify-write of the trigger list: replace the trigger at `index`.
    pub async fn replace_task_trigger(&self, id: &str, index: usize, t: TaskTrigger) -> Result<()> {
        let mut l = self.scheduled_task(id).await?.triggers;
        *l.get_mut(index)
            .ok_or_else(|| Error::Invalid("no such trigger".into()))? = t;
        self.put_triggers(id, &l).await
    }

    /// WRITE read-modify-write of the trigger list: DESTRUCTIVE, removes the trigger at `index`.
    pub async fn delete_task_trigger(&self, id: &str, index: usize, _: Confirmed) -> Result<()> {
        let mut l = self.scheduled_task(id).await?.triggers;
        if index >= l.len() {
            return Err(Error::Invalid("no such trigger".into()));
        }
        l.remove(index);
        self.put_triggers(id, &l).await
    }

    /// WRITE `POST /System/Configuration`: read-modify-write of one `TrickplayOptions` field.
    pub async fn set_trickplay_field(&self, key: &str, value: serde_json::Value) -> Result<()> {
        edit::validate_trickplay(key, &value)?;
        let mut c = self.server_config().await?;
        let t = c
            .extra
            .get_mut("TrickplayOptions")
            .and_then(|t| t.as_object_mut())
            .ok_or_else(|| Error::Invalid("this server has no trickplay options".into()))?;
        t.insert(key.into(), value);
        let body = serde_json::to_value(&c).expect("config serializes");
        self.send(Method::POST, &["System", "Configuration"], &[], Some(body))
            .await
            .map(drop)
    }

    /// WRITE `POST /Library/VirtualFolders`: add a library with one folder (`kind` from
    /// [`edit::LIBRARY_TYPES`]; `mixed` sends no type). Starts a scan.
    pub async fn add_library(&self, name: &str, kind: &str, path: &str) -> Result<()> {
        edit::validate_library_name(name)?;
        edit::validate_library_path(path)?;
        let mut q = vec![
            ("name", name.to_string()),
            ("refreshLibrary", "true".into()),
        ];
        match kind {
            "mixed" => {}
            k if edit::LIBRARY_TYPES.contains(&k) => q.push(("collectionType", k.into())),
            k => return Err(Error::Invalid(format!("unknown library type {k}"))),
        }
        let body = json!({"LibraryOptions": {"PathInfos": [{"Path": path}]}});
        self.send(Method::POST, &["Library", "VirtualFolders"], &q, Some(body))
            .await
            .map(drop)
    }

    /// WRITE `DELETE /Library/VirtualFolders`: DESTRUCTIVE, removes the library and its metadata
    /// from the server. Media files on disk are not touched.
    pub async fn remove_library(&self, name: &str, _: Confirmed) -> Result<()> {
        let q = [
            ("name", name.to_string()),
            ("refreshLibrary", "true".into()),
        ];
        self.send(Method::DELETE, &["Library", "VirtualFolders"], &q, None)
            .await
            .map(drop)
    }

    /// WRITE `POST /Library/VirtualFolders/Paths`: add a folder to a library.
    pub async fn add_library_path(&self, name: &str, path: &str) -> Result<()> {
        edit::validate_library_path(path)?;
        let body = json!({"Name": name, "PathInfo": {"Path": path}});
        let q = [("refreshLibrary", "true".to_string())];
        self.send(
            Method::POST,
            &["Library", "VirtualFolders", "Paths"],
            &q,
            Some(body),
        )
        .await
        .map(drop)
    }

    /// WRITE `DELETE /Library/VirtualFolders/Paths`: DESTRUCTIVE, stops indexing that folder.
    pub async fn remove_library_path(&self, name: &str, path: &str, _: Confirmed) -> Result<()> {
        let q = [
            ("name", name.to_string()),
            ("path", path.to_string()),
            ("refreshLibrary", "true".into()),
        ];
        self.send(
            Method::DELETE,
            &["Library", "VirtualFolders", "Paths"],
            &q,
            None,
        )
        .await
        .map(drop)
    }

    /// WRITE `POST /Library/VirtualFolders/LibraryOptions`: read-modify-write of one option of the
    /// library `item_id` (`VirtualFolder::item_id`); the other options are sent back unchanged.
    pub async fn set_library_option(
        &self,
        item_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<()> {
        edit::validate_library_option(key, &value)?;
        let mut o = self
            .virtual_folders()
            .await?
            .into_iter()
            .find(|f| f.item_id == item_id)
            .ok_or_else(|| Error::Invalid("no such library".into()))?
            .library_options;
        o[key] = value;
        let body = json!({"Id": item_id, "LibraryOptions": o});
        self.send(
            Method::POST,
            &["Library", "VirtualFolders", "LibraryOptions"],
            &[],
            Some(body),
        )
        .await
        .map(drop)
    }

    /// WRITE `POST /Plugins/{id}/{version}/Enable` or `/Disable`; takes effect after a restart.
    pub async fn set_plugin_enabled(&self, id: &str, version: &str, on: bool) -> Result<()> {
        let verb = if on { "Enable" } else { "Disable" };
        self.send(Method::POST, &["Plugins", id, version, verb], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `DELETE /Plugins/{id}/{version}`: DESTRUCTIVE, removes the plugin and, after a
    /// restart, everything it provides.
    pub async fn uninstall_plugin(&self, id: &str, version: &str, _: Confirmed) -> Result<()> {
        self.send(Method::DELETE, &["Plugins", id, version], &[], None)
            .await
            .map(drop)
    }

    /// WRITE `POST /Packages/Installed/{name}`: the server downloads and installs the newest
    /// version of a catalog package (needs a restart to load).
    pub async fn install_package(&self, p: &Package) -> Result<()> {
        let v = p
            .versions
            .first()
            .ok_or_else(|| Error::Invalid("package has no versions".into()))?;
        let q = [
            ("assemblyGuid", p.guid.clone()),
            ("version", v.version.clone()),
            ("repositoryUrl", v.repository_url.clone()),
        ];
        self.send(Method::POST, &["Packages", "Installed", &p.name], &q, None)
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::serve;
    use serde_json::{Value, json};

    fn yes() -> Confirmed {
        Confirmed::i_have_asked_the_user()
    }
    fn posts(log: &crate::mock::Log) -> Vec<(String, Value)> {
        log.lock()
            .unwrap()
            .iter()
            .filter(|r| r.0 != "GET")
            .map(|r| {
                (
                    format!("{} {}", r.0, r.1),
                    serde_json::from_str(&r.2).unwrap_or(Value::Null),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn branding_keeps_unknown_fields() {
        let (c, log) = serve(vec![
            (
                "GET",
                "/System/Configuration/branding",
                200,
                r#"{"LoginDisclaimer":"old","Future":1}"#.into(),
            ),
            ("POST", "/System/Configuration/branding", 204, String::new()),
        ])
        .await;
        c.set_branding(None, Some("a{b:c}"), None).await.unwrap();
        let p = posts(&log);
        assert_eq!(p[0].0, "POST /System/Configuration/branding");
        assert_eq!(
            p[0].1,
            json!({"LoginDisclaimer":"old","Future":1,"CustomCss":"a{b:c}"})
        );
    }

    #[tokio::test]
    async fn network_edit_is_validated_before_any_request() {
        let net = r#"{"InternalHttpPort":8096,"InternalHttpsPort":8920,"EnableIPv4":true,"EnableIPv6":false,"X":[1]}"#;
        let (c, log) = serve(vec![
            ("GET", "/System/Configuration/network", 200, net.into()),
            ("POST", "/System/Configuration/network", 204, String::new()),
        ])
        .await;
        assert!(
            c.set_network(&NetworkEdit::HttpPort(0), yes())
                .await
                .is_err()
        );
        assert!(
            c.set_network(&NetworkEdit::LanNetworks(vec!["nope".into()]), yes())
                .await
                .is_err()
        );
        assert!(posts(&log).is_empty(), "nothing was written");
        c.set_network(&NetworkEdit::LanNetworks(vec!["10.0.0.0/8".into()]), yes())
            .await
            .unwrap();
        let p = posts(&log);
        assert_eq!(p[0].1["LocalNetworkSubnets"], json!(["10.0.0.0/8"]));
        assert_eq!(p[0].1["X"], json!([1]), "other fields survive");
    }

    #[tokio::test]
    async fn triggers_add_edit_delete() {
        let task = r#"{"Id":"t","Triggers":[{"Type":"StartupTrigger"},{"Type":"IntervalTrigger","IntervalTicks":36000000000}]}"#;
        let (c, log) = serve(vec![
            ("GET", "/ScheduledTasks/t", 200, task.into()),
            ("POST", "/ScheduledTasks/t/Triggers", 204, String::new()),
        ])
        .await;
        c.add_task_trigger("t", TaskTrigger::daily("03:00").unwrap())
            .await
            .unwrap();
        c.replace_task_trigger("t", 1, TaskTrigger::every_minutes(30).unwrap())
            .await
            .unwrap();
        c.delete_task_trigger("t", 0, yes()).await.unwrap();
        assert!(c.delete_task_trigger("t", 5, yes()).await.is_err());
        assert!(
            c.replace_task_trigger("t", 2, TaskTrigger::on_startup())
                .await
                .is_err()
        );
        let p = posts(&log);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].1.as_array().unwrap().len(), 3);
        assert_eq!(p[0].1[2]["TimeOfDayTicks"], 108_000_000_000i64);
        assert_eq!(p[1].1[1]["IntervalTicks"], 18_000_000_000i64);
        assert_eq!(
            p[2].1,
            json!([{"Type":"IntervalTrigger","IntervalTicks":36000000000i64}])
        );
    }

    #[tokio::test]
    async fn trickplay_writes_only_its_field() {
        let cfg = r#"{"ServerName":"s","TrickplayOptions":{"Interval":10000,"JpegQuality":90}}"#;
        let (c, log) = serve(vec![
            ("GET", "/System/Configuration", 200, cfg.into()),
            ("POST", "/System/Configuration", 204, String::new()),
        ])
        .await;
        assert!(
            c.set_trickplay_field("JpegQuality", json!(500))
                .await
                .is_err()
        );
        assert!(c.set_trickplay_field("Bogus", json!(1)).await.is_err());
        c.set_trickplay_field("JpegQuality", json!(70))
            .await
            .unwrap();
        assert_eq!(
            posts(&log)[0].1["TrickplayOptions"],
            json!({"Interval":10000,"JpegQuality":70})
        );
    }

    #[tokio::test]
    async fn library_add_remove_and_options() {
        let folders = r#"[{"Name":"Films","ItemId":"L1","LibraryOptions":{"EnableRealtimeMonitor":false,"PathInfos":[{"Path":"/m"}]}}]"#;
        let (c, log) = serve(vec![
            ("GET", "/Library/VirtualFolders", 200, folders.into()),
            ("POST", "/Library/VirtualFolders", 204, String::new()),
            ("DELETE", "/Library/VirtualFolders", 204, String::new()),
            ("POST", "/Library/VirtualFolders/Paths", 204, String::new()),
            (
                "DELETE",
                "/Library/VirtualFolders/Paths",
                204,
                String::new(),
            ),
            (
                "POST",
                "/Library/VirtualFolders/LibraryOptions",
                204,
                String::new(),
            ),
        ])
        .await;
        assert!(c.add_library("Films", "weird", "/m").await.is_err());
        assert!(c.add_library("Films", "movies", "relative").await.is_err());
        assert!(c.add_library("a/b", "movies", "/m").await.is_err());
        c.add_library("My Films", "movies", "/media/films")
            .await
            .unwrap();
        c.add_library("Misc", "mixed", "/media/misc").await.unwrap();
        c.add_library_path("Films", "/media/more").await.unwrap();
        c.remove_library_path("Films", "/m", yes()).await.unwrap();
        c.remove_library("Films", yes()).await.unwrap();
        assert!(
            c.set_library_option("L1", "PathInfos", json!([]))
                .await
                .is_err()
        );
        assert!(
            c.set_library_option("nope", "EnableRealtimeMonitor", json!(true))
                .await
                .is_err()
        );
        c.set_library_option("L1", "EnableRealtimeMonitor", json!(true))
            .await
            .unwrap();
        let reqs: Vec<_> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.0 != "GET")
            .map(|r| format!("{} {}", r.0, r.1))
            .collect();
        assert_eq!(
            reqs[0],
            "POST /Library/VirtualFolders?name=My+Films&refreshLibrary=true&collectionType=movies"
        );
        assert_eq!(
            reqs[1],
            "POST /Library/VirtualFolders?name=Misc&refreshLibrary=true"
        );
        assert_eq!(
            reqs[3],
            "DELETE /Library/VirtualFolders/Paths?name=Films&path=%2Fm&refreshLibrary=true"
        );
        assert_eq!(
            reqs[4],
            "DELETE /Library/VirtualFolders?name=Films&refreshLibrary=true"
        );
        let p = posts(&log);
        assert_eq!(
            p[0].1,
            json!({"LibraryOptions":{"PathInfos":[{"Path":"/media/films"}]}})
        );
        assert_eq!(
            p[5].1,
            json!({"Id":"L1","LibraryOptions":{"EnableRealtimeMonitor":true,"PathInfos":[{"Path":"/m"}]}})
        );
    }

    #[tokio::test]
    async fn plugin_enable_uninstall_install() {
        let (c, log) = serve(vec![
            ("POST", "/Plugins/p1/1.0.0.0/Disable", 204, String::new()),
            ("POST", "/Plugins/p1/1.0.0.0/Enable", 204, String::new()),
            ("DELETE", "/Plugins/p1/1.0.0.0", 204, String::new()),
            (
                "POST",
                "/Packages/Installed/Fancy%20Plugin",
                204,
                String::new(),
            ),
        ])
        .await;
        c.set_plugin_enabled("p1", "1.0.0.0", false).await.unwrap();
        c.set_plugin_enabled("p1", "1.0.0.0", true).await.unwrap();
        c.uninstall_plugin("p1", "1.0.0.0", yes()).await.unwrap();
        let pkg: Package = serde_json::from_str(r#"{"name":"Fancy Plugin","guid":"g1","versions":[{"version":"2.0.0.0","repositoryUrl":"https://r/m.json"},{"version":"1.0.0.0","repositoryUrl":"https://r/m.json"}]}"#).unwrap();
        c.install_package(&pkg).await.unwrap();
        assert!(c.install_package(&Package::default()).await.is_err());
        let last = log.lock().unwrap().last().cloned().unwrap();
        assert!(
            last.1.contains("assemblyGuid=g1")
                && last.1.contains("version=2.0.0.0")
                && last.1.contains("repositoryUrl=https%3A%2F%2Fr%2Fm.json"),
            "{}",
            last.1
        );
    }
}
