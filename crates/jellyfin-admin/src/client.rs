use crate::{
    ActivityPage, ApiKey, Branding, Config, Device, Error, ItemCounts, LogFile, NetworkConfig,
    Plugin, Result, ScheduledTask, ServerConfig, Session, SystemInfo, User, VirtualFolder,
    model::Page,
};
use reqwest::{Method, Url, header};
use serde::de::DeserializeOwned;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

/// Admin Jellyfin client (needs an admin API key). Reads live here; writes are in `write.rs`.
#[derive(Clone)]
pub struct AdminClient {
    http: reqwest::Client,
    base: Url,
    /// Set once any request got 401 (token revoked); shared by clones.
    rejected: Arc<AtomicBool>,
}

impl std::fmt::Debug for AdminClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminClient")
            .field("base", &self.base.as_str())
            .field("token", &"<redacted>")
            .finish()
    }
}

impl AdminClient {
    /// `Config::load()` reads `NOCTURNE_JF_URL` / `NOCTURNE_JF_TOKEN` (and `.env`), like the read-only crate.
    pub fn new(cfg: Config) -> Result<Self> {
        let base = Url::parse(&cfg.base_url)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
            .ok_or_else(|| Error::Config("base_url is not a valid http(s) URL".into()))?;
        let mut auth = header::HeaderValue::from_str(&format!(
            "MediaBrowser Client=\"NOCTURNE Admin\", Device=\"Linux\", DeviceId=\"nocturne-admin\", Version=\"{}\", Token=\"{}\"",
            env!("CARGO_PKG_VERSION"),
            cfg.token
        ))
        .map_err(|_| Error::Config("token contains invalid header characters".into()))?;
        auth.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth);
        if let Some((n, v)) = nocturne_jellyfin::header::for_url(base.as_str()) {
            headers.insert(n, v);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .default_headers(headers)
            .build()
            .map_err(http_err)?;
        Ok(Self {
            http,
            base,
            rejected: Arc::default(),
        })
    }

    /// True once the server answered 401 to any admin request.
    pub fn rejected(&self) -> bool {
        self.rejected.load(Ordering::Relaxed)
    }

    // ---- reads (every method below is READ: GET only) ----

    /// READ `GET /System/Info` (full; needs admin).
    pub async fn system_info(&self) -> Result<SystemInfo> {
        self.get(&["System", "Info"], &[]).await
    }

    /// READ `GET /System/ActivityLog/Entries` (paged, newest first).
    pub async fn activity_log(&self, start: u32, limit: u32) -> Result<ActivityPage> {
        let p = [
            ("startIndex", start.to_string()),
            ("limit", limit.to_string()),
        ];
        self.get(&["System", "ActivityLog", "Entries"], &p).await
    }

    /// READ `GET /Sessions`.
    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.get(&["Sessions"], &[]).await
    }

    /// READ `GET /Devices`.
    pub async fn devices(&self) -> Result<Vec<Device>> {
        Ok(self.get::<Page<Device>>(&["Devices"], &[]).await?.items)
    }

    /// READ `GET /ScheduledTasks` (state, progress, triggers, last result).
    pub async fn scheduled_tasks(&self) -> Result<Vec<ScheduledTask>> {
        self.get(&["ScheduledTasks"], &[]).await
    }

    /// READ `GET /ScheduledTasks/{id}`.
    pub async fn scheduled_task(&self, id: &str) -> Result<ScheduledTask> {
        self.get(&["ScheduledTasks", id], &[]).await
    }

    /// READ `GET /Library/VirtualFolders` (libraries with refresh status).
    pub async fn virtual_folders(&self) -> Result<Vec<VirtualFolder>> {
        self.get(&["Library", "VirtualFolders"], &[]).await
    }

    /// READ `GET /Users` (each includes its policy).
    pub async fn users(&self) -> Result<Vec<User>> {
        self.get(&["Users"], &[]).await
    }

    /// READ `GET /Users/{id}` (detail + policy).
    pub async fn user(&self, id: &str) -> Result<User> {
        self.get(&["Users", id], &[]).await
    }

    /// READ `GET /Plugins`.
    pub async fn plugins(&self) -> Result<Vec<Plugin>> {
        self.get(&["Plugins"], &[]).await
    }

    /// READ `GET /System/Logs` (server log file list).
    pub async fn log_files(&self) -> Result<Vec<LogFile>> {
        self.get(&["System", "Logs"], &[]).await
    }

    /// READ `GET /System/Logs/Log?name=` and keep the last `lines` lines.
    // ponytail: downloads the whole file; logs are rotated daily, add Range if they get huge.
    pub async fn log_tail(&self, name: &str, lines: usize) -> Result<String> {
        let text = self
            .send(
                Method::GET,
                &["System", "Logs", "Log"],
                &[("name", name.into())],
                None,
            )
            .await?
            .text()
            .await
            .map_err(http_err)?;
        Ok(tail_lines(&text, lines))
    }

    /// READ `GET /Branding/Configuration` (public endpoint).
    pub async fn branding(&self) -> Result<Branding> {
        self.get(&["Branding", "Configuration"], &[]).await
    }

    /// READ `GET /System/Configuration`.
    pub async fn server_config(&self) -> Result<ServerConfig> {
        self.get(&["System", "Configuration"], &[]).await
    }

    /// READ `GET /Items/Counts`.
    pub async fn item_counts(&self) -> Result<ItemCounts> {
        self.get(&["Items", "Counts"], &[]).await
    }

    /// READ `GET /System/Configuration/network`.
    pub async fn network_config(&self) -> Result<NetworkConfig> {
        self.get(&["System", "Configuration", "network"], &[]).await
    }

    /// READ `GET /System/Configuration/encoding` (kept as raw JSON: the schema differs by version).
    pub async fn encoding_config(&self) -> Result<serde_json::Value> {
        self.get(&["System", "Configuration", "encoding"], &[])
            .await
    }

    /// READ `GET /System/Configuration/network` as raw JSON (used for read-modify-write).
    pub async fn network_config_raw(&self) -> Result<serde_json::Value> {
        self.get(&["System", "Configuration", "network"], &[]).await
    }

    /// READ `GET /System/Configuration/branding` as raw JSON.
    pub async fn branding_raw(&self) -> Result<serde_json::Value> {
        self.get(&["System", "Configuration", "branding"], &[])
            .await
    }

    /// READ `GET /Packages`: the plugin catalog (needs the server to reach its repositories).
    pub async fn packages(&self) -> Result<Vec<crate::Package>> {
        self.get(&["Packages"], &[]).await
    }

    /// READ `GET /Repositories`: the plugin repositories the server reads its catalog from.
    pub async fn plugin_repositories(&self) -> Result<Vec<crate::Repository>> {
        self.get(&["Repositories"], &[]).await
    }

    /// READ `GET /Libraries/AvailableOptions`: the providers offered for a library content type
    /// (`None` = the generic list the server returns without a type).
    pub async fn available_library_options(
        &self,
        content_type: Option<&str>,
    ) -> Result<crate::AvailableOptions> {
        let mut q = vec![("isNewLibrary", "false".to_string())];
        if let Some(t) = content_type {
            q.push(("libraryContentType", t.into()));
        }
        self.get(&["Libraries", "AvailableOptions"], &q).await
    }

    /// READ `GET /Localization/ParentalRatings`: rating names with a level, lowest first, one per name.
    pub async fn parental_ratings(&self) -> Result<Vec<crate::ParentalRating>> {
        let mut l: Vec<crate::ParentalRating> =
            self.get(&["Localization", "ParentalRatings"], &[]).await?;
        l.retain(|r| r.value.is_some());
        l.sort_by_key(|r| r.value);
        l.dedup_by(|a, b| a.name == b.name);
        Ok(l)
    }

    /// READ `GET /DisplayPreferences/usersettings`: a user's display preferences (raw JSON; the
    /// shape belongs to the client that saved them, here as `client`).
    pub async fn display_preferences(
        &self,
        user_id: &str,
        client: &str,
    ) -> Result<serde_json::Value> {
        let q = [("userId", user_id.to_string()), ("client", client.into())];
        self.get(&["DisplayPreferences", "usersettings"], &q).await
    }

    /// READ `GET /Backup` (server 10.11+; an error on older servers).
    pub async fn backups(&self) -> Result<Vec<serde_json::Value>> {
        self.get(&["Backup"], &[]).await
    }

    /// READ `GET /Auth/Keys`.
    pub async fn api_keys(&self) -> Result<Vec<ApiKey>> {
        Ok(self
            .get::<Page<ApiKey>>(&["Auth", "Keys"], &[])
            .await?
            .items)
    }

    // ---- internals ----

    /// Infallible: `base` was validated in `new`; segments are percent-encoded.
    pub(crate) fn url(&self, segs: &[&str], pairs: &[(&str, String)]) -> Url {
        let mut u = self.base.clone();
        u.path_segments_mut()
            .expect("http(s) base can have path segments")
            .pop_if_empty()
            .extend(segs);
        if !pairs.is_empty() {
            u.query_pairs_mut().extend_pairs(pairs);
        }
        u
    }

    pub(crate) async fn send(
        &self,
        method: Method,
        segs: &[&str],
        pairs: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response> {
        let url = self.url(segs, pairs);
        let path = url.path().to_string();
        let mut req = self.http.request(method, url);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(http_err)?;
        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 401 {
                self.rejected.store(true, Ordering::Relaxed);
            }
            return Err(Error::Status {
                status: status.as_u16(),
                path,
            });
        }
        Ok(resp)
    }

    /// Send `body` as is with the given content type (image uploads).
    pub(crate) async fn send_raw(
        &self,
        segs: &[&str],
        content_type: &str,
        body: String,
    ) -> Result<()> {
        let url = self.url(segs, &[]);
        let path = url.path().to_string();
        let resp = self
            .http
            .post(url)
            .header(header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .map_err(http_err)?;
        let status = resp.status();
        if status.as_u16() == 401 {
            self.rejected.store(true, Ordering::Relaxed);
        }
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                path,
            });
        }
        Ok(())
    }

    async fn get<T: DeserializeOwned>(&self, segs: &[&str], pairs: &[(&str, String)]) -> Result<T> {
        self.send(Method::GET, segs, pairs, None)
            .await?
            .json()
            .await
            .map_err(http_err)
    }
}

/// Last `n` lines of `text`, joined with `\n` (no trailing newline).
pub fn tail_lines(text: &str, n: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(n)..].join("\n")
}

// Strip the URL from errors so nothing URL-borne (ids, names) lands in logs.
pub(crate) fn http_err(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn client() -> AdminClient {
        AdminClient::new(Config {
            base_url: "http://jf.test:8097/".into(),
            token: "tok123".into(),
        })
        .unwrap()
    }

    #[test]
    fn tail_keeps_last_lines() {
        assert_eq!(tail_lines("a\nb\nc\n", 2), "b\nc");
        assert_eq!(tail_lines("a\nb", 10), "a\nb");
        assert_eq!(tail_lines("a\nb", 0), "");
        assert_eq!(tail_lines("", 3), "");
    }

    #[test]
    fn debug_redacts_token() {
        assert!(!format!("{:?}", client()).contains("tok123"));
        let k: ApiKey = serde_json::from_str(r#"{"AccessToken":"sekret","AppName":"a"}"#).unwrap();
        assert!(!format!("{k:?}").contains("sekret"));
    }

    #[test]
    fn invalid_base_url() {
        for bad in ["", "not a url", "ftp://h"] {
            let r = AdminClient::new(Config {
                base_url: bad.into(),
                token: "t".into(),
            });
            assert!(matches!(r, Err(Error::Config(_))), "{bad}");
        }
    }

    #[test]
    fn url_building() {
        let c = client();
        assert_eq!(
            c.url(
                &["System", "ActivityLog", "Entries"],
                &[("startIndex", "0".into()), ("limit", "5".into())]
            )
            .as_str(),
            "http://jf.test:8097/System/ActivityLog/Entries?startIndex=0&limit=5"
        );
        assert_eq!(
            c.url(&["Users", "a/b c"], &[]).as_str(),
            "http://jf.test:8097/Users/a%2Fb%20c"
        );
    }

    #[test]
    fn deserializes_tolerantly() {
        let t: Vec<ScheduledTask> = serde_json::from_str(
            r#"[{"Id":"t1","Name":"Scan","State":"Running","CurrentProgressPercentage":12.5,
            "Triggers":[{"Type":"DailyTrigger","TimeOfDayTicks":1000,"Future":1}],
            "LastExecutionResult":{"Status":"Completed","StartTimeUtc":"a","EndTimeUtc":"b"},"Brand":"new"},{"Id":"t2"}]"#,
        ).unwrap();
        assert_eq!(t[0].triggers[0].kind, "DailyTrigger");
        assert_eq!(t[0].current_progress_percentage, Some(12.5));
        assert_eq!(
            t[0].last_execution_result.as_ref().unwrap().status,
            "Completed"
        );
        assert!(t[1].triggers.is_empty() && t[1].last_execution_result.is_none());

        let a: ActivityPage = serde_json::from_str(
            r#"{"Items":[{"Id":7,"Name":"n","Type":"UserCreated","Severity":"Information","X":1}],"TotalRecordCount":9,"StartIndex":0}"#,
        ).unwrap();
        assert_eq!(
            (
                a.items[0].id,
                a.items[0].kind.as_str(),
                a.total_record_count
            ),
            (7, "UserCreated", 9)
        );

        let s: Vec<Session> = serde_json::from_str(
            r#"[{"Id":"s","UserName":"u","NowPlayingItem":{"Id":"i","Name":"Film","Type":"Movie"},"PlayState":{"IsPaused":true}}]"#,
        ).unwrap();
        assert_eq!(s[0].now_playing_item.as_ref().unwrap().kind, "Movie");
        assert!(s[0].play_state.as_ref().unwrap().is_paused);

        let p: Page<Device> =
            serde_json::from_str(r#"{"Items":[{"Id":"d","Name":"n"}],"TotalRecordCount":1}"#)
                .unwrap();
        assert_eq!(p.items[0].id, "d");
    }

    #[test]
    fn counts_and_network_deserialize() {
        let c: ItemCounts =
            serde_json::from_str(r#"{"MovieCount":3,"EpisodeCount":9,"Weird":1}"#).unwrap();
        assert_eq!((c.movie_count, c.episode_count, c.series_count), (3, 9, 0));
        let n: NetworkConfig = serde_json::from_str(
            r#"{"InternalHttpPort":8097,"EnableUPnP":true,"EnableIPv6":false}"#,
        )
        .unwrap();
        assert!(n.enable_upnp && !n.enable_ipv6 && n.internal_http_port == 8097);
    }

    #[test]
    fn policy_and_config_round_trip_unknown_fields() {
        let p: UserPolicy = serde_json::from_str(
            r#"{"IsAdministrator":true,"EnableFoo":[1,2],"SyncPlayAccess":"None"}"#,
        )
        .unwrap();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["IsAdministrator"], true);
        assert_eq!(v["EnableFoo"], serde_json::json!([1, 2]));
        assert_eq!(v["SyncPlayAccess"], "None");
        let c: ServerConfig =
            serde_json::from_str(r#"{"ServerName":"x","CacheSize":1600}"#).unwrap();
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(
            (v["ServerName"].as_str(), v["CacheSize"].as_i64()),
            (Some("x"), Some(1600))
        );
        assert!(v.get("UICulture").is_none());
    }
}
