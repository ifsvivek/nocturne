//! The desktop app as a client of NOCTURNE servers (docs/dev/SERVER_PLAN.md, API: docs/api/openapi.yaml): the list of servers the user added, the typed REST calls the app needs, and the `/api/v1/events` WebSocket with reconnect. This computer's own server is reached over its unix socket, where a same-uid peer is the local admin and needs no credential; other servers by URL with a session token or API key from the keyring. Nothing here runs unless a server is configured or the local socket exists.
//!
//! Every call goes through `bridge::spawn` (the app wraps it in `task::run`); the event stream is started from an Iced subscription only while a server is active.
use crate::server::types::{MediaKind, Release};
use crate::server::{
    Candidate, Event, MediaRequest, QueueItem, Status, Title, TitleDetail, socket_path,
};
use reqwest::{Method, StatusCode, header};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The key of this computer's server in [`ServerList::active`].
pub const LOCAL: &str = "local";
const VERSION: u32 = 1;

/// Where a server is and how to sign in to it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// The unix socket: the local admin, no credential.
    Local(PathBuf),
    /// `http(s)://host:port` and the session token or API key (none before sign-in).
    Remote { url: String, token: Option<String> },
}

impl Target {
    /// This computer's server, if its socket exists.
    pub fn local() -> Option<Target> {
        socket_path().filter(|p| p.exists()).map(Target::Local)
    }

    /// [`LOCAL`] or the URL: the key in [`ServerList`] and the keyring.
    pub fn key(&self) -> &str {
        match self {
            Target::Local(_) => LOCAL,
            Target::Remote { url, .. } => url,
        }
    }

    /// The address a browser opens for the dashboard. The local server's comes from `server.toml`.
    pub fn dashboard(&self) -> String {
        match self {
            Target::Local(_) => {
                let cfg = crate::server::ServerConfig::default_path()
                    .and_then(|p| crate::server::ServerConfig::load_from(&p).ok())
                    .unwrap_or_default();
                let scheme = if cfg.http.cert_path.is_some() {
                    "https"
                } else {
                    "http"
                };
                format!("{scheme}://127.0.0.1:{}/", cfg.http.port)
            }
            Target::Remote { url, .. } => format!("{url}/"),
        }
    }
}

/// `http://host:port` without a trailing slash; a bare host gets `http://`. Anything else (a path, another scheme) is refused.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("Enter the server's address.".into());
    }
    let s = if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    let u = reqwest::Url::parse(&s).map_err(|_| "That is not a valid address.".to_string())?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return Err("Use an http:// or https:// address.".into());
    }
    if u.path() != "/" || u.query().is_some() {
        return Err("Enter only the server's address, without a path.".into());
    }
    Ok(u.as_str().trim_end_matches('/').to_string())
}

/// Keyring entry of a remote server's token (through `nocturne_session`, never a file).
pub fn secret_name(url: &str) -> String {
    format!("nocturne-server|{url}")
}

/// One remote server the user added. Names only: the token is in the keyring.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub url: String,
    /// Who is signed in (shown on its card); `None` for an API key.
    pub user: Option<String>,
}

/// `$XDG_CONFIG_HOME/nocturne/nocturne-servers.toml`: the servers added by URL and which one is active (`LOCAL` for this computer's). Versioned, atomic writes, no secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerList {
    pub version: u32,
    pub active: Option<String>,
    #[serde(rename = "server")]
    pub servers: Vec<Saved>,
}

impl ServerList {
    pub fn default_path() -> Option<PathBuf> {
        nocturne_session::paths::config_dir().map(|d| d.join("nocturne-servers.toml"))
    }

    pub fn load() -> Self {
        Self::default_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = Self::default_path().ok_or("no configuration folder")?;
        self.save_to(&p)
    }

    /// A missing or unreadable file is an empty list (nothing here is worth refusing to start for).
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let mut me = self.clone();
        me.version = VERSION;
        let text = toml::to_string_pretty(&me).map_err(|e| e.to_string())?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Add or update `url` and make it active.
    pub fn add(&mut self, url: &str, user: Option<String>) {
        match self.servers.iter_mut().find(|s| s.url == url) {
            Some(s) => s.user = user,
            None => self.servers.push(Saved {
                url: url.into(),
                user,
            }),
        }
        self.active = Some(url.into());
    }

    /// Forget `key`; it stops being active.
    pub fn remove(&mut self, key: &str) {
        self.servers.retain(|s| s.url != key);
        if self.active.as_deref() == Some(key) {
            self.active = None;
        }
    }
}

/// A failed call, in the terms the pages need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// 401: the token is gone or wrong; sign in again.
    Auth,
    /// 503: the module is switched off on the server.
    Off,
    /// Any other answer, with the server's `error` text.
    Status(u16, String),
    /// Not reachable.
    Net(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Auth => f.write_str("Sign in again: the server did not accept the credential."),
            Error::Off => f.write_str("This part of the server is switched off."),
            Error::Status(_, m) => f.write_str(m),
            Error::Net(m) => write!(f, "The server cannot be reached ({m})."),
        }
    }
}

/// The error of a non-success answer: `{"error": "..."}` when the server gave one.
pub fn error_of(code: u16, body: &str) -> Error {
    match code {
        401 => Error::Auth,
        503 => Error::Off,
        _ => Error::Status(
            code,
            serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v["error"].as_str().map(String::from))
                .unwrap_or_else(|| format!("The server answered {code}.")),
        ),
    }
}

/// Whether module `name` runs on the server.
pub fn module_on(st: &Status, name: &str) -> bool {
    st.modules.iter().any(|m| m.name == name && m.running)
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct User {
    pub name: String,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Login {
    pub token: String,
    pub user: User,
}

/// Storage module (`/api/v1/storage/roots`). Its schema belongs to that module; only what the page shows is read, the rest is ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StorageRoot {
    pub id: u64,
    pub label: String,
    /// local, smb, nfs or agent.
    pub kind: String,
    /// library, downloads or both.
    pub role: String,
    pub path: String,
    pub url: String,
    pub available: bool,
    pub health: StorageHealth,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StorageHealth {
    pub reachable: bool,
    pub writable: bool,
    pub mounted: bool,
    pub space: Option<Space>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Space {
    pub free: u64,
    pub total: u64,
}

/// Something done to a queued grab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueAct {
    Pause,
    Resume,
    /// Retry an import that was blocked.
    Retry,
    /// Remove it with its torrent and data; `true` also blocklists the release and searches again.
    Remove(bool),
}

#[derive(Deserialize)]
struct NewId {
    #[serde(default)]
    id: u64,
}

#[derive(Deserialize)]
struct Needed {
    needed: bool,
}

/// One server's REST API.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("NOCTURNE/", env!("CARGO_PKG_VERSION")))
}

impl Client {
    /// `timeout` bounds every call; interactive searches pass a long one.
    pub fn new(t: &Target, timeout: Duration) -> Result<Client, Error> {
        let b = builder().timeout(timeout);
        let (b, base, token) = match t {
            #[cfg(unix)]
            Target::Local(sock) => (
                b.unix_socket(sock.as_path()),
                "http://localhost".into(),
                None,
            ),
            #[cfg(not(unix))]
            Target::Local(_) => return Err(Error::Net("no local socket on this platform".into())),
            Target::Remote { url, token } => (b, url.clone(), token.clone()),
        };
        let http = b.build().map_err(|e| Error::Net(e.to_string()))?;
        Ok(Client { http, base, token })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, Error> {
        let mut req = self
            .http
            .request(method, format!("{}/api/v1{path}", self.base));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Net(e.without_url().to_string()))?;
        let code = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Net(e.without_url().to_string()))?;
        if !code.is_success() {
            return Err(error_of(code.as_u16(), &text));
        }
        // Routes that answer nothing useful deserialize into `()` from `null`.
        serde_json::from_str(if text.trim().is_empty() {
            "null"
        } else {
            &text
        })
        .map_err(|e| Error::Status(code.as_u16(), format!("Unexpected answer: {e}")))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        self.call(Method::GET, path, None).await
    }

    async fn post(&self, path: &str, body: Option<serde_json::Value>) -> Result<(), Error> {
        self.call::<serde_json::Value>(Method::POST, path, body)
            .await
            .map(drop)
    }

    pub async fn status(&self) -> Result<Status, Error> {
        self.get("/status").await
    }

    /// The server has no admin yet: the dashboard's first-run setup comes first.
    pub async fn setup_needed(&self) -> Result<bool, Error> {
        self.get::<Needed>("/setup").await.map(|n| n.needed)
    }

    pub async fn login(&self, user: &str, password: &str) -> Result<Login, Error> {
        let body = serde_json::json!({"username": user, "password": password});
        self.call(Method::POST, "/auth/login", Some(body)).await
    }

    /// Who the credential belongs to (checks an API key).
    pub async fn me(&self) -> Result<User, Error> {
        self.get("/auth/me").await
    }

    /// Revoke the session token (an API key stays valid).
    pub async fn logout(&self) -> Result<(), Error> {
        self.post("/auth/logout", None).await
    }

    pub async fn queue(&self) -> Result<Vec<QueueItem>, Error> {
        self.get("/library/queue").await
    }

    pub async fn queue_act(&self, id: u64, act: QueueAct) -> Result<(), Error> {
        match act {
            QueueAct::Pause => self.post(&format!("/library/queue/{id}/pause"), None).await,
            QueueAct::Resume => {
                self.post(&format!("/library/queue/{id}/resume"), None)
                    .await
            }
            QueueAct::Retry => self.post(&format!("/library/queue/{id}/retry"), None).await,
            QueueAct::Remove(again) => self
                .call::<serde_json::Value>(
                    Method::DELETE,
                    &format!("/library/queue/{id}?blocklist={again}&search={again}"),
                    None,
                )
                .await
                .map(drop),
        }
    }

    pub async fn titles(&self, kind: Option<MediaKind>) -> Result<Vec<Title>, Error> {
        match kind {
            Some(k) => {
                self.get(&format!("/library/titles?kind={}", kind_param(k)))
                    .await
            }
            None => self.get("/library/titles").await,
        }
    }

    pub async fn title(&self, id: u64) -> Result<TitleDetail, Error> {
        self.get(&format!("/library/titles/{id}")).await
    }

    pub async fn monitor(&self, id: u64, monitored: bool) -> Result<(), Error> {
        self.call::<serde_json::Value>(
            Method::PUT,
            &format!("/library/titles/{id}/monitor"),
            Some(serde_json::json!({"monitored": monitored})),
        )
        .await
        .map(drop)
    }

    /// Automatic search for every wanted unit; results arrive as events.
    pub async fn search(&self, id: u64) -> Result<(), Error> {
        self.post(&format!("/library/titles/{id}/search"), None)
            .await
    }

    /// Interactive search: every release with the decision on it.
    pub async fn releases(&self, id: u64) -> Result<Vec<Candidate>, Error> {
        self.call(
            Method::POST,
            &format!("/library/titles/{id}/releases"),
            None,
        )
        .await
    }

    pub async fn grab(&self, id: u64, r: &Release) -> Result<u64, Error> {
        let body = serde_json::to_value(r).map_err(|e| Error::Net(e.to_string()))?;
        self.call::<NewId>(
            Method::POST,
            &format!("/library/titles/{id}/grab"),
            Some(body),
        )
        .await
        .map(|s| s.id)
    }

    pub async fn requests(&self) -> Result<Vec<MediaRequest>, Error> {
        self.get("/requests").await
    }

    pub async fn approve(&self, id: u64) -> Result<(), Error> {
        self.post(&format!("/requests/{id}/approve"), None).await
    }

    pub async fn cancel_request(&self, id: u64) -> Result<(), Error> {
        self.call::<serde_json::Value>(Method::DELETE, &format!("/requests/{id}"), None)
            .await
            .map(drop)
    }

    pub async fn storage(&self) -> Result<Vec<StorageRoot>, Error> {
        self.get("/storage/roots").await
    }
}

/// `MediaKind` as its query value.
pub fn kind_param(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Movie => "movie",
        MediaKind::Series => "series",
        MediaKind::Music => "music",
        MediaKind::Book => "book",
    }
}

/// What the event stream reports to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Feed {
    Connected,
    /// The socket dropped; a reconnect follows after the backoff.
    Disconnected,
    /// The credential was refused: the stream stopped.
    Auth,
    Event(Box<Event>),
}

/// Seconds before reconnect attempt `n` (0-based): 1, 2, 4, 8, 16, then every 30.
pub fn backoff_secs(n: u32) -> u64 {
    (1u64 << n.min(5)).min(30)
}

/// One event message; anything else (a newer server's event type) is skipped.
pub fn parse_event(text: &str) -> Option<Event> {
    serde_json::from_str(text).ok()
}

fn ws_key() -> String {
    use base64::Engine;
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    base64::engine::general_purpose::STANDARD.encode(b)
}

type Ws = tokio_tungstenite::WebSocketStream<reqwest::Upgraded>;

/// The WebSocket handshake goes through reqwest (HTTP/1.1 upgrade), so the unix socket, TLS and proxies are those of every other call.
async fn connect(t: &Target) -> Result<Ws, Error> {
    let b = reqwest::Client::builder()
        .http1_only()
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("NOCTURNE/", env!("CARGO_PKG_VERSION")));
    let (b, base, token) = match t {
        #[cfg(unix)]
        Target::Local(sock) => (b.unix_socket(sock.as_path()), "http://localhost", None),
        #[cfg(not(unix))]
        Target::Local(_) => return Err(Error::Net("no local socket on this platform".into())),
        Target::Remote { url, token } => (b, url.as_str(), token.as_deref()),
    };
    let http = b.build().map_err(|e| Error::Net(e.to_string()))?;
    let mut req = http
        .get(format!("{base}/api/v1/events"))
        .header(header::CONNECTION, "Upgrade")
        .header(header::UPGRADE, "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", ws_key());
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = tokio::time::timeout(Duration::from_secs(10), req.send())
        .await
        .map_err(|_| Error::Net("timed out".into()))?
        .map_err(|e| Error::Net(e.without_url().to_string()))?;
    match resp.status() {
        StatusCode::SWITCHING_PROTOCOLS => {}
        s => return Err(error_of(s.as_u16(), "")),
    }
    let up = resp
        .upgrade()
        .await
        .map_err(|e| Error::Net(e.without_url().to_string()))?;
    Ok(tokio_tungstenite::WebSocketStream::from_raw_socket(
        up,
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        None,
    )
    .await)
}

/// Read one connection until it drops. `false`: the app stopped listening.
async fn session(mut ws: Ws, tx: &tokio::sync::mpsc::UnboundedSender<Feed>) -> bool {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    // A ping every 30 s; the server's pong (or any message) must arrive within 75 s.
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    let mut seen = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if tx.is_closed() {
                    return false;
                }
                if seen.elapsed() > Duration::from_secs(75)
                    || ws.send(Message::Ping(Vec::new().into())).await.is_err()
                {
                    return true;
                }
            }
            m = ws.next() => {
                seen = tokio::time::Instant::now();
                match m {
                    Some(Ok(Message::Text(t))) => {
                        if let Some(e) = parse_event(t.as_str())
                            && tx.send(Feed::Event(Box::new(e))).is_err()
                        {
                            return false;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return true,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

/// The event stream of `t`, reconnecting with [`backoff_secs`] until `tx`'s receiver is dropped or the credential is refused.
pub async fn events(t: Target, tx: tokio::sync::mpsc::UnboundedSender<Feed>) {
    let mut n = 0;
    loop {
        match connect(&t).await {
            Ok(ws) => {
                n = 0;
                if tx.send(Feed::Connected).is_err() || !session(ws, &tx).await {
                    return;
                }
            }
            Err(Error::Auth) => {
                let _ = tx.send(Feed::Auth);
                return;
            }
            Err(_) => {}
        }
        if tx.send(Feed::Disconnected).is_err() {
            return;
        }
        let wait = Duration::from_secs(backoff_secs(n));
        n += 1;
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = tx.closed() => return,
        }
    }
}

/// Seconds as `1 h 5 min`, `4 min`, `12 s` (a download's time left).
pub fn eta(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} min", secs / 60),
        _ => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::ModuleStatus;

    #[test]
    fn urls_are_normalized_and_bad_ones_refused() {
        assert_eq!(
            normalize_url(" nas.lan:8100/ ").unwrap(),
            "http://nas.lan:8100"
        );
        assert_eq!(
            normalize_url("https://media.example.org").unwrap(),
            "https://media.example.org"
        );
        assert!(normalize_url("").is_err());
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("http://x/api").is_err());
    }

    #[test]
    fn server_list_round_trips_and_forgets_the_active_one() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c/nocturne-servers.toml");
        assert_eq!(ServerList::load_from(&p), ServerList::default());
        let mut l = ServerList::default();
        l.add("http://a:8100", Some("ann".into()));
        l.add("http://b:8100", None);
        l.add("http://a:8100", Some("bob".into()));
        assert_eq!(l.servers.len(), 2);
        assert_eq!(l.servers[0].user.as_deref(), Some("bob"));
        assert_eq!(l.active.as_deref(), Some("http://a:8100"));
        l.save_to(&p).unwrap();
        let back = ServerList::load_from(&p);
        assert_eq!(back.version, 1);
        assert_eq!(back.servers, l.servers);
        assert!(!std::fs::read_to_string(&p).unwrap().contains("token"));
        l.remove("http://a:8100");
        assert_eq!((l.servers.len(), l.active.clone()), (1, None));
        std::fs::write(&p, "not toml [").unwrap();
        assert_eq!(ServerList::load_from(&p), ServerList::default());
    }

    #[test]
    fn errors_map_from_status_and_body() {
        assert_eq!(error_of(401, ""), Error::Auth);
        assert_eq!(error_of(503, "{}"), Error::Off);
        assert_eq!(
            error_of(409, r#"{"error":"already monitored"}"#),
            Error::Status(409, "already monitored".into())
        );
        assert_eq!(
            error_of(500, "oops"),
            Error::Status(500, "The server answered 500.".into())
        );
    }

    #[test]
    fn backoff_grows_then_caps() {
        let v: Vec<u64> = (0..8).map(backoff_secs).collect();
        assert_eq!(v, [1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn events_parse_and_unknown_ones_are_skipped() {
        let e = parse_event(r#"{"type":"request","data":7}"#).unwrap();
        assert_eq!(e, Event::Request(7));
        let h = parse_event(
            r#"{"type":"health","data":{"name":"downloads","enabled":true,"running":false,"error":null}}"#,
        )
        .unwrap();
        assert!(matches!(h, Event::Health(m) if m.name == "downloads" && !m.running));
        assert!(parse_event(r#"{"type":"from_the_future","data":1}"#).is_none());
        assert!(parse_event(r#"{"type":"queue","data":[]}"#).is_some());
    }

    #[test]
    fn modules_count_only_when_running() {
        let st = Status {
            version: "0.7.5".into(),
            uptime_secs: 1,
            setup_needed: false,
            modules: vec![
                ModuleStatus {
                    name: "library".into(),
                    enabled: true,
                    running: true,
                    error: None,
                },
                ModuleStatus {
                    name: "downloads".into(),
                    enabled: true,
                    running: false,
                    error: Some("port in use".into()),
                },
            ],
            titles: 0,
            missing: 0,
            downloading: 0,
            down_rate: 0,
            up_rate: 0,
            jobs: vec![],
        };
        assert!(module_on(&st, "library"));
        assert!(!module_on(&st, "downloads"));
        assert!(!module_on(&st, "storage"));
    }

    #[test]
    fn storage_roots_read_leniently() {
        let r: Vec<StorageRoot> = serde_json::from_str(
            r#"[{"id":1,"label":"Films","kind":"local","role":"library","path":"/m","url":"","remote":"","mount_point":"/","available":true,
                "health":{"reachable":true,"writable":true,"mounted":true,"space":{"free":5,"total":10},"error":null}}]"#,
        )
        .unwrap();
        assert_eq!(r[0].health.space, Some(Space { free: 5, total: 10 }));
        let bare: StorageRoot = serde_json::from_str(r#"{"id":2}"#).unwrap();
        assert!(bare.health.space.is_none());
    }

    #[test]
    fn eta_reads_naturally() {
        assert_eq!(eta(12), "12 s");
        assert_eq!(eta(240), "4 min");
        assert_eq!(eta(3900), "1 h 5 min");
    }

    /// Against a tiny HTTP server on a unix socket: the local target sends no credential, a remote one sends its token, and status codes map to errors.
    #[cfg(unix)]
    #[tokio::test]
    async fn calls_go_over_the_unix_socket_and_map_errors() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("server.sock");
        let l = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let (code, body) = if req.contains("authorization") {
                        ("400 Bad Request", r#"{"error":"local needs no token"}"#)
                    } else if req.starts_with("get /api/v1/setup ") {
                        ("200 OK", r#"{"needed":true}"#)
                    } else if req.starts_with("get /api/v1/requests ") {
                        ("503 Service Unavailable", r#"{"error":"module off"}"#)
                    } else {
                        ("404 Not Found", r#"{"error":"no such route"}"#)
                    };
                    let resp = format!(
                        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        let c = Client::new(&Target::Local(sock), Duration::from_secs(5)).unwrap();
        assert_eq!(c.setup_needed().await, Ok(true));
        assert_eq!(c.requests().await, Err(Error::Off));
        assert_eq!(
            c.title(9).await,
            Err(Error::Status(404, "no such route".into()))
        );
        let gone = Client::new(
            &Target::Remote {
                url: "http://127.0.0.1:9".into(),
                token: Some("t".into()),
            },
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(matches!(gone.status().await, Err(Error::Net(_))));
    }
}
