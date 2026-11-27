//! Trakt (opt-in, docs/TRAKT.md): device-code sign-in with the user's own Trakt API app, scrobbling of
//! what plays, and an import of watched history. Nothing ships credentials: the client id and secret
//! are the user's, kept with the tokens in the keyring. Blocking keyring calls run on
//! `spawn_blocking`; every request carries `NOCTURNE/<ver>` as User-Agent and stays under Trakt's
//! limit of one write per second.
use crate::online::http::{Gate, user_agent};
use crate::source::{Kind, Media, ProviderIds};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

pub const KEY_ID: &str = "trakt-client-id";
pub const KEY_TOKENS: &str = "trakt-tokens";

/// What the dialog says is sent, and to whom.
pub const PRIVACY: &str = "While Trakt is on, NOCTURNE sends api.trakt.tv the provider ids (IMDb / TMDB / TVDB) of the movie or episode you are playing, the season and episode numbers, and how far in you are, when you start, pause and stop. History import reads your watched list. Titles, file names, paths and server addresses are never sent. You sign in on trakt.tv in your browser; NOCTURNE never sees your Trakt password, and its sign-in and your app's client id stay in the system keyring on this computer.";

/// Shown with every Trakt feature. Plain text name only (no logo is used).
pub const CREDIT: &str = "Trakt is a trademark of Trakt, Inc. NOCTURNE uses the Trakt API but is not affiliated with or endorsed by Trakt. NOCTURNE does not use or store Trakt images or comments.";

/// Trakt's sign-in and token host (PKCE guide); the API itself is api.trakt.tv.
pub const AUTH_BASE: &str = "https://auth.trakt.tv";
/// The public Trakt client id NOCTURNE ships (PKCE, so no secret is involved). The placeholder means "none": the user then enters their own.
pub const BUILTIN_CLIENT_ID: &str = "x8Eug1YSxLXsb3Dcnbhv3naoRiuHlD_f6IsL5UzY4iI";

/// What `BUILTIN_CLIENT_ID` holds in a source tree that ships no id.
const BUILTIN_PLACEHOLDER: &str = "NOCTURNE_TRAKT_BUILTIN_CLIENT_ID_PLACEHOLDER";

/// Pick the client id: a user override (the env var, then the one saved in the keyring) beats the built-in one; blank values and the placeholder count as absent.
pub fn resolve_client_id(env: Option<&str>, saved: Option<&str>, builtin: &str) -> Option<String> {
    let builtin = Some(builtin).filter(|b| *b != BUILTIN_PLACEHOLDER);
    [env, saved, builtin]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(String::from)
}

/// The built-in id alone (what "Connect Trakt" uses when the user set nothing).
pub fn builtin_client_id() -> Option<String> {
    resolve_client_id(None, None, BUILTIN_CLIENT_ID)
}

/// The loopback port of the redirect URI the user registers in their Trakt app (RFC 8252, fixed port).
pub const REDIRECT_PORT: u16 = 38917;

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/callback")
}

/// Refresh this long before the access token runs out.
const REFRESH_MARGIN: u64 = 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// 401 / 403 / 400 on refresh: the app credentials or the sign-in are no longer valid.
    Auth,
    NotFound,
    Failed,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Debug builds may point at a mock (`NOCTURNE_TRAKT_URL`); a release build never leaves api.trakt.tv.
pub fn base_url() -> String {
    if cfg!(debug_assertions)
        && let Ok(u) = std::env::var("NOCTURNE_TRAKT_URL")
    {
        return u.trim_end_matches('/').to_string();
    }
    "https://api.trakt.tv".into()
}

/// The sign-in / token host: [`AUTH_BASE`], or the same mock as the API in debug builds.
pub fn auth_base() -> String {
    if cfg!(debug_assertions)
        && let Ok(u) = std::env::var("NOCTURNE_TRAKT_URL")
    {
        return u.trim_end_matches('/').to_string();
    }
    AUTH_BASE.into()
}

// ---- ids and request bodies ------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ids {
    pub imdb: Option<String>,
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
}

impl Ids {
    pub fn from_provider(p: &ProviderIds) -> Self {
        let n = |v: &Option<String>| v.as_deref().and_then(|s| s.trim().parse().ok());
        Self {
            imdb: p
                .imdb
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase),
            tmdb: n(&p.tmdb),
            tvdb: n(&p.tvdb),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.imdb.is_none() && self.tmdb.is_none() && self.tvdb.is_none()
    }

    /// Any shared id.
    pub fn matches(&self, p: &ProviderIds) -> bool {
        let o = Ids::from_provider(p);
        (self.imdb.is_some() && self.imdb == o.imdb)
            || (self.tmdb.is_some() && self.tmdb == o.tmdb)
            || (self.tvdb.is_some() && self.tvdb == o.tvdb)
    }

    fn json(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(v) = &self.imdb {
            m.insert("imdb".into(), json!(v));
        }
        if let Some(v) = self.tmdb {
            m.insert("tmdb".into(), json!(v));
        }
        if let Some(v) = self.tvdb {
            m.insert("tvdb".into(), json!(v));
        }
        Value::Object(m)
    }
}

/// What is being scrobbled: a movie by its ids, an episode by its show's ids + season + number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Movie(Ids),
    Episode { show: Ids, season: u32, number: u32 },
}

impl Target {
    /// `series` are the provider ids of an episode's series (read from its own source).
    pub fn of(m: &Media, series: Option<&ProviderIds>) -> Option<Self> {
        let t = match m.kind {
            Kind::Movie => Self::Movie(Ids::from_provider(&m.ids)),
            Kind::Episode => Self::Episode {
                show: Ids::from_provider(series?),
                season: m.season_number?,
                number: m.number?,
            },
            _ => return None,
        };
        let empty = match &t {
            Self::Movie(i) => i.is_empty(),
            Self::Episode { show, .. } => show.is_empty(),
        };
        (!empty).then_some(t)
    }

    pub fn body(&self, progress: f64) -> Value {
        let progress = (progress.clamp(0.0, 100.0) * 100.0).round() / 100.0;
        match self {
            Self::Movie(i) => json!({"movie": {"ids": i.json()}, "progress": progress}),
            Self::Episode {
                show,
                season,
                number,
            } => json!({
                "show": {"ids": show.json()},
                "episode": {"season": season, "number": number},
                "progress": progress,
            }),
        }
    }
}

// ---- scrobble state machine --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    Pause,
    Stop,
}

impl Action {
    fn path(self) -> &'static str {
        match self {
            Self::Start => "/scrobble/start",
            Self::Pause => "/scrobble/pause",
            Self::Stop => "/scrobble/stop",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    Started,
    /// Periodic / pause / seek report.
    Progress {
        paused: bool,
    },
    Stopped,
}

/// Turns the player's reports into scrobble calls: start once, pause / start again when the pause
/// state flips, stop at the end. Nothing is sent every ten seconds (Trakt expires a scrobble by itself).
#[derive(Default)]
pub struct Scrobbler(Option<(String, bool)>);

impl Scrobbler {
    pub fn on(&mut self, key: &str, ev: Ev) -> Option<Action> {
        match (ev, &mut self.0) {
            (Ev::Started, _) => {
                self.0 = Some((key.into(), false));
                Some(Action::Start)
            }
            (Ev::Stopped, cur) if cur.as_ref().is_some_and(|(k, _)| k == key) => {
                *cur = None;
                Some(Action::Stop)
            }
            (Ev::Progress { paused }, Some((k, p))) if k == key && *p != paused => {
                *p = paused;
                Some(if paused { Action::Pause } else { Action::Start })
            }
            _ => None,
        }
    }
}

// ---- tokens ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: u64,
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    #[serde(default)]
    created_at: u64,
}

impl From<TokenReply> for Tokens {
    fn from(r: TokenReply) -> Self {
        let made = if r.created_at > 0 {
            r.created_at
        } else {
            now()
        };
        Self {
            access: r.access_token,
            refresh: r.refresh_token,
            expires_at: made + r.expires_in,
        }
    }
}

impl Tokens {
    fn to_json(&self) -> String {
        json!({"a": self.access, "r": self.refresh, "e": self.expires_at}).to_string()
    }

    fn from_json(s: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(s).ok()?;
        Some(Self {
            access: v["a"].as_str()?.into(),
            refresh: v["r"].as_str()?.into(),
            expires_at: v["e"].as_u64()?,
        })
    }

    fn stale(&self, at: u64) -> bool {
        self.expires_at <= at + REFRESH_MARGIN
    }
}

// ---- sign-in: authorization code + PKCE (RFC 7636 / RFC 8252) ----------------------------------------------

/// One sign-in attempt's secrets: the verifier stays here, only its S256 challenge goes to Trakt.
#[derive(Clone, Debug)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

fn b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// `BASE64URL(SHA256(verifier))`.
pub fn challenge_of(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    b64(&Sha256::digest(verifier.as_bytes()))
}

impl Pkce {
    pub fn new() -> Self {
        let (mut v, mut s) = ([0u8; 32], [0u8; 16]);
        // An OS random source that fails is not something to limp on from.
        getrandom::fill(&mut v).expect("os randomness");
        getrandom::fill(&mut s).expect("os randomness");
        Self::from_bytes(&v, &s)
    }

    /// 32 random bytes give a 43-character verifier (RFC 7636: 43 to 128).
    pub fn from_bytes(verifier: &[u8], state: &[u8]) -> Self {
        let verifier = b64(verifier);
        Self {
            challenge: challenge_of(&verifier),
            verifier,
            state: b64(state),
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// The page to open in the browser.
pub fn authorize_url(auth: &str, client_id: &str, redirect: &str, p: &Pkce) -> String {
    reqwest::Url::parse_with_params(
        &format!("{}/oauth/authorize", auth.trim_end_matches('/')),
        [
            ("response_type", "code"),
            ("client_id", client_id.trim()),
            ("redirect_uri", redirect),
            ("code_challenge", p.challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", p.state.as_str()),
        ],
    )
    .map_or_else(|_| String::new(), |u| u.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    /// The user said no on trakt.tv.
    Denied,
    PortBusy,
    Timeout,
    Cancelled,
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>NOCTURNE</title><body style=\"font-family:sans-serif;background:#0b0d12;color:#e8eaf0;text-align:center;padding-top:20vh\"><h2>You can close this tab</h2><p>Return to NOCTURNE.</p>";

async fn reply(s: &mut tokio::net::TcpStream, status: &str, body: &str) {
    use tokio::io::AsyncWriteExt;
    let _ = s
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
}

/// One-shot listener on `127.0.0.1:port` for Trakt's redirect to `/callback?code=..&state=..`.
/// Requests with another path or a wrong `state` are answered and ignored; it ends on the right
/// callback, a denial, `timeout` or `cancel`.
pub async fn wait_callback(
    port: u16,
    state: &str,
    timeout: Duration,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Callback {
    use std::sync::atomic::Ordering;
    use tokio::io::AsyncReadExt;
    let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await else {
        return Callback::PortBusy;
    };
    let end = std::time::Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Callback::Cancelled;
        }
        if std::time::Instant::now() >= end {
            return Callback::Timeout;
        }
        let Ok(Ok((mut s, _))) = tokio::time::timeout(Duration::from_millis(250), l.accept()).await
        else {
            continue;
        };
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let target = head.split_whitespace().nth(1).unwrap_or("");
        let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
            reply(&mut s, "400 Bad Request", "bad request").await;
            continue;
        };
        if url.path() != "/callback" {
            reply(&mut s, "404 Not Found", "not found").await;
            continue;
        }
        let q = |k: &str| {
            url.query_pairs()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.into_owned())
        };
        if q("state").as_deref() != Some(state) {
            reply(&mut s, "400 Bad Request", "wrong state").await;
            continue;
        }
        if q("error").is_some() {
            reply(&mut s, "200 OK", "Cancelled. You can close this tab.").await;
            return Callback::Denied;
        }
        match q("code").filter(|c| !c.is_empty()) {
            Some(c) => {
                reply(&mut s, "200 OK", DONE_PAGE).await;
                return Callback::Code(c);
            }
            None => reply(&mut s, "400 Bad Request", "no code").await,
        }
    }
}

// ---- history ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Show {
    pub ids: Ids,
    /// (season, episode) pairs watched.
    pub episodes: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub movies: Vec<Ids>,
    pub shows: Vec<Show>,
}

fn ids_of(v: &Value) -> Ids {
    Ids {
        imdb: v["imdb"].as_str().map(str::to_lowercase),
        tmdb: v["tmdb"].as_u64(),
        tvdb: v["tvdb"].as_u64(),
    }
}

pub fn parse_movies(v: &Value) -> Vec<Ids> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|e| ids_of(&e["movie"]["ids"]))
        .filter(|i| !i.is_empty())
        .collect()
}

pub fn parse_shows(v: &Value) -> Vec<Show> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|e| Show {
            ids: ids_of(&e["show"]["ids"]),
            episodes: e["seasons"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|s| {
                    let n = s["number"].as_u64().unwrap_or(0) as u32;
                    s["episodes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(move |x| Some((n, x["number"].as_u64()? as u32)))
                })
                .collect(),
        })
        .filter(|s| !s.ids.is_empty())
        .collect()
}

// ---- client --------------------------------------------------------------------------------------------

/// One episode on the user's Trakt calendar (`/calendars/my/shows`): the show's ids, the episode and its air time (UTC, ISO 8601).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Airing {
    pub show: String,
    pub ids: Ids,
    pub season: u32,
    pub number: u32,
    pub title: String,
    pub first_aired: String,
}

pub fn parse_calendar(v: &Value) -> Vec<Airing> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            Some(Airing {
                show: e["show"]["title"].as_str()?.into(),
                ids: ids_of(&e["show"]["ids"]),
                season: e["episode"]["season"].as_u64()? as u32,
                number: e["episode"]["number"].as_u64()? as u32,
                title: e["episode"]["title"].as_str().unwrap_or_default().into(),
                first_aired: e["first_aired"].as_str()?.into(),
            })
        })
        .collect()
}

pub struct Trakt {
    http: reqwest::Client,
    base: String,
    auth: String,
    id: String,
    gate: Gate,
    tokens: tokio::sync::Mutex<Option<Tokens>>,
    /// Keep refreshed tokens in the keyring (off in tests).
    persist: bool,
}

impl Trakt {
    pub fn new(base: &str, auth: &str, id: &str, tokens: Option<Tokens>, persist: bool) -> Self {
        Self::paced(base, auth, id, tokens, persist, Duration::from_millis(1100))
    }

    fn paced(
        base: &str,
        auth: &str,
        id: &str,
        tokens: Option<Tokens>,
        persist: bool,
        every: Duration,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(user_agent())
                .timeout(Duration::from_secs(20))
                .build()
                .expect("http client"),
            base: base.trim_end_matches('/').into(),
            auth: auth.trim_end_matches('/').into(),
            id: id.trim().into(),
            gate: Gate::new(every),
            tokens: tokio::sync::Mutex::new(tokens),
            persist,
        }
    }

    /// The user's app credentials and sign-in from the keyring; `None` when not connected. Debug builds with `NOCTURNE_TRAKT_MOCK_TOKEN` set use a fake sign-in (for a mock server named by `NOCTURNE_TRAKT_URL`).
    pub async fn load() -> Option<Self> {
        if cfg!(debug_assertions) && std::env::var_os("NOCTURNE_TRAKT_MOCK_TOKEN").is_some() {
            let t = Tokens {
                access: "mock".into(),
                refresh: "mock".into(),
                expires_at: now() + 86_400,
            };
            return Some(Self::new(&base_url(), &auth_base(), "mock", Some(t), false));
        }
        let (id, tok) = tokio::task::spawn_blocking(|| {
            let tok = nocturne_session::load_secret(KEY_TOKENS).ok().flatten();
            (Self::client_id_blocking(), tok)
        })
        .await
        .ok()?;
        let tokens = Tokens::from_json(&tok?)?;
        Some(Self::new(
            &base_url(),
            &auth_base(),
            &id?,
            Some(tokens),
            true,
        ))
    }

    pub fn has_tokens_blocking() -> bool {
        nocturne_session::load_secret(KEY_TOKENS)
            .ok()
            .flatten()
            .is_some()
    }

    /// Forget the app credentials and the sign-in (blocking keyring calls).
    pub fn forget_blocking() {
        for k in [KEY_ID, KEY_TOKENS] {
            let _ = nocturne_session::delete_secret(k);
        }
    }

    /// Keep the credentials entered in the dialog (blocking keyring calls).
    pub fn save_app_blocking(id: &str) -> Result<(), String> {
        crate::online::set_secret(KEY_ID, id)
    }

    /// The id to use without asking: env override, saved override, then the built-in one (blocking keyring call).
    pub fn client_id_blocking() -> Option<String> {
        let env = std::env::var("NOCTURNE_TRAKT_CLIENT_ID").ok();
        resolve_client_id(
            env.as_deref(),
            Self::saved_app_blocking().as_deref(),
            BUILTIN_CLIENT_ID,
        )
    }

    pub fn saved_app_blocking() -> Option<String> {
        nocturne_session::load_secret(KEY_ID).ok().flatten()
    }

    fn headers(&self, rb: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
        let rb = rb
            .header("trakt-api-version", "2")
            .header("trakt-api-key", &self.id);
        match token {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }

    /// One call, spaced by the gate; a 429 waits for `Retry-After` (at most 30 s) and tries once more.
    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
        for attempt in 0..2 {
            self.gate.wait().await;
            let r = rb.try_clone().ok_or(Error::Failed)?.send().await;
            let r = r.map_err(|_| Error::Failed)?;
            if r.status().as_u16() == 429 && attempt == 0 {
                let s = r
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(2);
                tokio::time::sleep(Duration::from_secs(s.min(30))).await;
                continue;
            }
            return Ok(r);
        }
        Err(Error::Failed)
    }

    fn check(r: &reqwest::Response) -> Result<(), Error> {
        match r.status().as_u16() {
            200..=299 | 409 => Ok(()), // 409: the same scrobble within the hour; nothing to do
            401 | 403 => Err(Error::Auth),
            404 => Err(Error::NotFound),
            _ => Err(Error::Failed),
        }
    }

    // -- sign-in

    /// Trade the authorization `code` for tokens (PKCE: the verifier, no client secret).
    pub async fn exchange(&self, code: &str, p: &Pkce, redirect: &str) -> Result<Tokens, Error> {
        let rb = self
            .headers(self.http.post(format!("{}/oauth/token", self.auth)), None)
            .json(&json!({
                "code": code,
                "client_id": self.id,
                "redirect_uri": redirect,
                "grant_type": "authorization_code",
                "code_verifier": p.verifier,
            }));
        let r = self.send(rb).await?;
        if matches!(r.status().as_u16(), 400 | 401 | 403) {
            return Err(Error::Auth);
        }
        Self::check(&r)?;
        Ok(r.json::<TokenReply>()
            .await
            .map_err(|_| Error::Failed)?
            .into())
    }

    /// Adopt a granted sign-in and keep it in the keyring.
    pub async fn set_tokens(&self, t: Tokens) {
        self.store(&t).await;
        *self.tokens.lock().await = Some(t);
    }

    async fn store(&self, t: &Tokens) {
        if self.persist {
            let j = t.to_json();
            let _ = tokio::task::spawn_blocking(move || crate::online::set_secret(KEY_TOKENS, &j))
                .await;
        }
    }

    /// A valid access token, refreshed first when it is about to run out.
    async fn access(&self) -> Result<String, Error> {
        let mut g = self.tokens.lock().await;
        let t = g.clone().ok_or(Error::Auth)?;
        if !t.stale(now()) {
            return Ok(t.access);
        }
        // Refresh needs only the client id and the refresh token (no secret under PKCE).
        let rb = self
            .headers(self.http.post(format!("{}/oauth/token", self.auth)), None)
            .json(&json!({
                "refresh_token": t.refresh,
                "client_id": self.id,
                "redirect_uri": redirect_uri(REDIRECT_PORT),
                "grant_type": "refresh_token",
            }));
        let r = self.send(rb).await?;
        if matches!(r.status().as_u16(), 400 | 401 | 403) {
            return Err(Error::Auth);
        }
        Self::check(&r)?;
        let n: Tokens = r
            .json::<TokenReply>()
            .await
            .map_err(|_| Error::Failed)?
            .into();
        self.store(&n).await;
        *g = Some(n.clone());
        Ok(n.access)
    }

    /// Revoke the token at Trakt (best effort) and drop it.
    pub async fn revoke(&self) {
        if let Ok(tok) = self.access().await {
            let rb = self
                .headers(self.http.post(format!("{}/oauth/revoke", self.auth)), None)
                .json(&json!({"token": tok, "client_id": self.id}));
            let _ = self.send(rb).await;
        }
        *self.tokens.lock().await = None;
    }

    // -- authenticated calls

    async fn authed_get(&self, path: &str) -> Result<Value, Error> {
        let t = self.access().await?;
        let rb = self.headers(self.http.get(format!("{}{path}", self.base)), Some(&t));
        let r = self.send(rb).await?;
        Self::check(&r)?;
        r.json().await.map_err(|_| Error::Failed)
    }

    /// A public list or search (trending, popular, search): needs the app's client id, no sign-in. Read only.
    pub async fn public_get(&self, path: &str, q: &[(String, String)]) -> Result<Value, Error> {
        let rb = self.headers(self.http.get(format!("{}{path}", self.base)).query(q), None);
        let r = self.send(rb).await?;
        Self::check(&r)?;
        r.json().await.map_err(|_| Error::Failed)
    }

    pub async fn username(&self) -> Result<String, Error> {
        let v = self.authed_get("/users/settings").await?;
        v["user"]["username"]
            .as_str()
            .map(str::to_string)
            .ok_or(Error::Failed)
    }

    /// `progress` in percent.
    pub async fn scrobble(&self, a: Action, t: &Target, progress: f64) -> Result<(), Error> {
        let tok = self.access().await?;
        let rb = self
            .headers(
                self.http.post(format!("{}{}", self.base, a.path())),
                Some(&tok),
            )
            .json(&t.body(progress));
        Self::check(&self.send(rb).await?)
    }

    /// The signed-in user's shows airing from `start` (`YYYY-MM-DD`) for `days` days (Trakt allows at most 33). Read only.
    pub async fn calendar(&self, start: &str, days: u32) -> Result<Vec<Airing>, Error> {
        let v = self
            .authed_get(&format!("/calendars/my/shows/{start}/{}", days.min(33)))
            .await?;
        Ok(parse_calendar(&v))
    }

    pub async fn history(&self) -> Result<History, Error> {
        Ok(History {
            movies: parse_movies(&self.authed_get("/sync/watched/movies").await?),
            shows: parse_shows(&self.authed_get("/sync/watched/shows").await?),
        })
    }
}

// ---- scrobble worker -------------------------------------------------------------------------------------

pub struct Job {
    pub media: Media,
    /// The item's own source, to read its series' provider ids (episodes).
    pub backend: Option<crate::backend::Backend>,
    pub action: Action,
    pub progress: f64,
}

/// Sends scrobbles in order from one task, so start / pause / stop never overtake each other.
pub struct Worker {
    tx: tokio::sync::mpsc::UnboundedSender<Job>,
    /// Trakt refused the sign-in: the user has to connect again.
    pub rejected: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Worker {
    pub fn spawn(client: std::sync::Arc<Trakt>) -> Self {
        use std::sync::atomic::Ordering;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
        let rejected = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = rejected.clone();
        crate::bridge::spawn(async move {
            let mut series: std::collections::HashMap<
                (crate::source::SourceId, String),
                ProviderIds,
            > = Default::default();
            while let Some(j) = rx.recv().await {
                let m = &j.media;
                let mut ids = None;
                if m.kind == Kind::Episode
                    && let (Some(sid), Some(b)) = (&m.series_id, &j.backend)
                {
                    let key = (m.origin, sid.clone());
                    if !series.contains_key(&key)
                        && let Ok(f) = b.provider_ids(std::slice::from_ref(sid)).await
                        && let Some(p) = f.get(sid)
                    {
                        series.insert(key.clone(), p.clone());
                    }
                    ids = series.get(&key).cloned();
                }
                let Some(t) = Target::of(m, ids.as_ref()) else {
                    continue;
                };
                if client.scrobble(j.action, &t, j.progress).await == Err(Error::Auth) {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        });
        Self { tx, rejected }
    }

    pub fn send(&self, j: Job) {
        let _ = self.tx.send(j);
    }
}

// ---- history import ------------------------------------------------------------------------------------

/// Every item of `kind` of a source, a page at a time (capped: 20 000 items).
async fn list(b: &crate::backend::Backend, kind: &str) -> Vec<Media> {
    let mut out: Vec<Media> = vec![];
    while out.len() < 20_000 {
        let q = nocturne_jellyfin::BrowseQuery {
            include_types: vec![kind.into()],
            recursive: true,
            start_index: Some(out.len() as u32),
            limit: Some(500),
            ..Default::default()
        };
        match b.browse(&q).await {
            Ok((v, _)) if !v.is_empty() => out.extend(v),
            _ => break,
        }
    }
    out
}

/// The items of a source that Trakt says were watched but the source still shows as unwatched:
/// movies by provider id, episodes by their show's ids + season + number. The caller marks them
/// (local state; servers only when allowed) and never cascades them to other sources.
pub async fn unwatched_in(b: &crate::backend::Backend, h: &History) -> Vec<Media> {
    let mut out = vec![];
    if !h.movies.is_empty() {
        out.extend(
            list(b, "Movie")
                .await
                .into_iter()
                .filter(|m| !m.played && h.movies.iter().any(|i| i.matches(&m.ids))),
        );
    }
    if h.shows.is_empty() {
        return out;
    }
    for s in list(b, "Series").await {
        let Some(show) = h.shows.iter().find(|x| x.ids.matches(&s.ids)) else {
            continue;
        };
        let Ok(seasons) = b.seasons(&s.id).await else {
            continue;
        };
        for season in seasons {
            let Ok(eps) = b.episodes(&s.id, &season.id).await else {
                continue;
            };
            out.extend(eps.into_iter().filter(|e| {
                !e.played
                    && matches!(
                        (e.season_number.or(season.number), e.number),
                        (Some(a), Some(b)) if show.episodes.contains(&(a, b))
                    )
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Log = Arc<Mutex<Vec<(String, String)>>>; // (request head, body)

    /// `(path, [(status, body)])`: the list is consumed front to back and the last entry repeats.
    async fn mock(routes: Vec<(&'static str, Vec<(u16, String)>)>) -> (String, Log) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let log: Log = Default::default();
        let routes: Arc<Vec<_>> = Arc::new(
            routes
                .into_iter()
                .map(|(p, r)| (p, Mutex::new(VecDeque::from(r))))
                .collect(),
        );
        let lg = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let (routes, lg) = (routes.clone(), lg.clone());
                tokio::spawn(async move {
                    let mut buf = vec![];
                    let mut chunk = [0u8; 4096];
                    let end = loop {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let want: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    while buf.len() < end + want {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let body = String::from_utf8_lossy(&buf[end..]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                    lg.lock().unwrap().push((head, body));
                    let (status, out) = routes
                        .iter()
                        .find(|(p, _)| path.starts_with(p))
                        .map(|(_, q)| {
                            let mut q = q.lock().unwrap();
                            if q.len() > 1 {
                                q.pop_front().unwrap()
                            } else {
                                q[0].clone()
                            }
                        })
                        .unwrap_or((404, "{}".into()));
                    let extra = if status == 429 {
                        "Retry-After: 0\r\n"
                    } else {
                        ""
                    };
                    let _ = s
                        .write_all(
                            format!(
                                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{out}",
                                out.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        });
        (base, log)
    }

    fn client(base: &str, t: Option<Tokens>) -> Trakt {
        Trakt::paced(base, base, "cid", t, false, Duration::from_millis(1))
    }

    fn good() -> Tokens {
        Tokens {
            access: "acc".into(),
            refresh: "ref".into(),
            expires_at: now() + 86_400,
        }
    }

    fn movie() -> Target {
        Target::Movie(Ids {
            imdb: Some("tt0000001".into()),
            tmdb: Some(603),
            tvdb: None,
        })
    }

    #[test]
    fn bodies_use_ids_only() {
        let b = movie().body(12.345);
        assert_eq!(
            b,
            json!({"movie": {"ids": {"imdb": "tt0000001", "tmdb": 603}}, "progress": 12.35})
        );
        let e = Target::Episode {
            show: Ids {
                tvdb: Some(81189),
                ..Default::default()
            },
            season: 2,
            number: 5,
        };
        assert_eq!(
            e.body(150.0),
            json!({"show": {"ids": {"tvdb": 81189}}, "episode": {"season": 2, "number": 5}, "progress": 100.0})
        );
    }

    #[test]
    fn target_needs_ids() {
        let mut m = Media::from_item(&nocturne_jellyfin::Item::default());
        m.kind = Kind::Movie;
        assert_eq!(Target::of(&m, None), None, "no ids, nothing is sent");
        m.ids.tmdb = Some("603".into());
        assert!(Target::of(&m, None).is_some());
        m.kind = Kind::Episode;
        assert_eq!(
            Target::of(&m, None),
            None,
            "an episode needs its series ids"
        );
    }

    #[test]
    fn scrobbler_starts_pauses_stops_once() {
        let mut s = Scrobbler::default();
        assert_eq!(s.on("a", Ev::Progress { paused: true }), None);
        assert_eq!(s.on("a", Ev::Started), Some(Action::Start));
        assert_eq!(s.on("a", Ev::Progress { paused: false }), None);
        assert_eq!(
            s.on("a", Ev::Progress { paused: true }),
            Some(Action::Pause)
        );
        assert_eq!(s.on("a", Ev::Progress { paused: true }), None);
        assert_eq!(
            s.on("a", Ev::Progress { paused: false }),
            Some(Action::Start)
        );
        assert_eq!(s.on("b", Ev::Stopped), None);
        assert_eq!(s.on("a", Ev::Stopped), Some(Action::Stop));
        assert_eq!(s.on("a", Ev::Stopped), None);
    }

    #[test]
    fn history_parses_trakt_shapes() {
        let m = json!([{"plays": 2, "movie": {"title": "x", "year": 1999, "ids": {"trakt": 1, "imdb": "TT1", "tmdb": 603}}}]);
        assert_eq!(parse_movies(&m)[0].imdb.as_deref(), Some("tt1"));
        let s = json!([{"plays": 3, "show": {"ids": {"trakt": 2, "tvdb": 81189}},
            "seasons": [{"number": 1, "episodes": [{"number": 1, "plays": 1}, {"number": 2, "plays": 1}]}]}]);
        let p = parse_shows(&s);
        assert_eq!(p[0].episodes, [(1, 1), (1, 2)]);
        let ids = ProviderIds {
            tvdb: Some("81189".into()),
            ..Default::default()
        };
        assert!(p[0].ids.matches(&ids));
    }

    #[test]
    fn pkce_matches_rfc7636_vector_and_lengths() {
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let a = Pkce::new();
        let b = Pkce::new();
        assert_eq!(a.verifier.len(), 43);
        assert_eq!(a.challenge, challenge_of(&a.verifier));
        assert_ne!((&a.verifier, &a.state), (&b.verifier, &b.state));
        assert!(!a.verifier.contains(['+', '/', '=']));
    }

    #[test]
    fn client_id_resolution_prefers_overrides_and_ignores_placeholder() {
        let r = resolve_client_id;
        assert_eq!(
            r(Some("env"), Some("saved"), "built").as_deref(),
            Some("env")
        );
        assert_eq!(r(None, Some(" saved "), "built").as_deref(), Some("saved"));
        assert_eq!(r(Some(" "), None, "built").as_deref(), Some("built"));
        assert_eq!(r(None, None, BUILTIN_PLACEHOLDER), None);
        assert_eq!(r(None, None, ""), None);
        assert_eq!(
            r(Some("env"), None, BUILTIN_PLACEHOLDER).as_deref(),
            Some("env")
        );
        let id = r(None, None, "built").unwrap();
        let u = authorize_url("https://a", &id, "http://127.0.0.1:1/cb", &Pkce::new());
        assert!(u.contains("client_id=built"));
    }

    #[test]
    fn authorize_url_carries_the_pkce_parameters_and_no_secret() {
        let p = Pkce::from_bytes(&[7; 32], &[9; 16]);
        let u = authorize_url("https://auth.trakt.tv", "cid", &redirect_uri(38917), &p);
        let url = reqwest::Url::parse(&u).unwrap();
        assert_eq!(url.path(), "/oauth/authorize");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:38917/callback");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], p.challenge);
        assert_eq!(q["state"], p.state);
        assert!(!q.contains_key("client_secret") && !u.contains(&p.verifier));
    }

    async fn free_port() -> u16 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn loopback_callback_checks_state_and_ignores_strays() {
        let port = free_port().await;
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = tokio::spawn(wait_callback(
            port,
            "st",
            Duration::from_secs(10),
            cancel.clone(),
        ));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let get = |p: &str| {
            let u = format!("http://127.0.0.1:{port}{p}");
            async move { reqwest::get(u).await.unwrap() }
        };
        assert_eq!(get("/favicon.ico").await.status(), 404);
        assert_eq!(get("/callback?code=evil&state=nope").await.status(), 400);
        let ok = get("/callback?code=abc%20d&state=st").await;
        assert_eq!(ok.status(), 200);
        assert!(ok.text().await.unwrap().contains("You can close this tab"));
        assert_eq!(w.await.unwrap(), Callback::Code("abc d".into()));
    }

    #[tokio::test]
    async fn loopback_callback_denial_timeout_cancel_and_busy_port() {
        let port = free_port().await;
        let no = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = tokio::spawn(wait_callback(
            port,
            "st",
            Duration::from_secs(10),
            no.clone(),
        ));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = reqwest::get(format!(
            "http://127.0.0.1:{port}/callback?error=access_denied&state=st"
        ))
        .await;
        assert_eq!(w.await.unwrap(), Callback::Denied);

        let port = free_port().await;
        assert_eq!(
            wait_callback(port, "st", Duration::from_millis(400), no.clone()).await,
            Callback::Timeout
        );
        let yes = Arc::new(std::sync::atomic::AtomicBool::new(true));
        assert_eq!(
            wait_callback(port, "st", Duration::from_secs(5), yes).await,
            Callback::Cancelled
        );
        let hold = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = hold.local_addr().unwrap().port();
        assert_eq!(
            wait_callback(busy, "st", Duration::from_secs(1), no).await,
            Callback::PortBusy
        );
    }

    #[tokio::test]
    async fn calendar_is_one_authenticated_get_and_parses_a_fake_reply() {
        let reply = json!([
            {"first_aired": "2026-12-01T02:00:00.000Z",
             "episode": {"season": 2, "number": 5, "title": "Pilot", "ids": {"trakt": 9}},
             "show": {"title": "Demo Show", "year": 2020, "ids": {"trakt": 1, "tvdb": 81189, "imdb": "TT7"}}},
            // No air time or no episode: skipped.
            {"episode": {"season": 1, "number": 1}, "show": {"title": "x", "ids": {}}},
            {"first_aired": "2026-12-02T02:00:00.000Z", "show": {"title": "x", "ids": {}}}
        ]);
        let (base, log) = mock(vec![(
            "/calendars/my/shows/",
            vec![(200, reply.to_string())],
        )])
        .await;
        let c = client(&base, Some(good()));
        let v = c.calendar("2026-11-30", 99).await.unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].season, v[0].number), (2, 5));
        assert_eq!(v[0].show, "Demo Show");
        assert_eq!(v[0].ids.tvdb, Some(81189));
        assert_eq!(v[0].ids.imdb.as_deref(), Some("tt7"));
        let log = log.lock().unwrap().clone();
        assert!(
            log[0]
                .0
                .starts_with("GET /calendars/my/shows/2026-11-30/33 ")
        );
        assert!(
            log[0]
                .0
                .to_lowercase()
                .contains("authorization: bearer acc")
        );
        let none = client(&base, None);
        assert!(matches!(
            none.calendar("2026-11-30", 7).await,
            Err(Error::Auth)
        ));
    }

    #[tokio::test]
    async fn code_exchange_sends_verifier_and_no_secret() {
        let tok = r#"{"access_token":"A","token_type":"bearer","expires_in":86400,"refresh_token":"R","scope":"public","created_at":4102444800}"#;
        let (base, log) = mock(vec![("/oauth/token", vec![(200, tok.into())])]).await;
        let c = client(&base, None);
        let p = Pkce::from_bytes(&[1; 32], &[2; 16]);
        let t = c
            .exchange("thecode", &p, &redirect_uri(REDIRECT_PORT))
            .await
            .unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_str()), ("A", "R"));
        let log = log.lock().unwrap().clone();
        let h = log[0].0.to_lowercase();
        assert!(h.starts_with("post /oauth/token") && h.contains("user-agent: nocturne/"));
        assert!(h.contains("trakt-api-key: cid") && h.contains("trakt-api-version: 2"));
        let b: Value = serde_json::from_str(&log[0].1).unwrap();
        assert_eq!(b["grant_type"], "authorization_code");
        assert_eq!(b["code"], "thecode");
        assert_eq!(b["code_verifier"], p.verifier);
        assert_eq!(b["client_id"], "cid");
        assert_eq!(b["redirect_uri"], "http://127.0.0.1:38917/callback");
        assert!(b.get("client_secret").is_none());
        // a refused code is an auth error
        let (base, _) = mock(vec![("/oauth/token", vec![(400, "{}".into())])]).await;
        let r = client(&base, None)
            .exchange("x", &p, "http://127.0.0.1:1/callback")
            .await;
        assert_eq!(r, Err(Error::Auth));
    }

    #[tokio::test]
    async fn scrobble_sends_headers_and_treats_409_as_done() {
        let (base, log) = mock(vec![(
            "/scrobble/",
            vec![(201, "{}".into()), (409, "{}".into()), (401, "{}".into())],
        )])
        .await;
        let c = client(&base, Some(good()));
        c.scrobble(Action::Start, &movie(), 0.0).await.unwrap();
        c.scrobble(Action::Pause, &movie(), 40.0).await.unwrap();
        assert_eq!(
            c.scrobble(Action::Stop, &movie(), 95.0).await,
            Err(Error::Auth)
        );
        let log = log.lock().unwrap();
        let h = log[0].0.to_lowercase();
        assert!(h.starts_with("post /scrobble/start"));
        assert!(h.contains("trakt-api-key: cid") && h.contains("trakt-api-version: 2"));
        assert!(h.contains("authorization: bearer acc"));
        assert!(
            log[1].0.starts_with("POST /scrobble/pause") && log[1].1.contains("\"progress\":40.0")
        );
    }

    #[tokio::test]
    async fn rate_limited_call_is_retried_once() {
        let (base, log) = mock(vec![(
            "/users/settings",
            vec![
                (429, "".into()),
                (200, r#"{"user":{"username":"sam"}}"#.into()),
            ],
        )])
        .await;
        let c = client(&base, Some(good()));
        assert_eq!(c.username().await.unwrap(), "sam");
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn expiring_token_is_refreshed_before_use() {
        let new = r#"{"access_token":"A2","refresh_token":"R2","expires_in":86400,"created_at":4102444800}"#;
        let (base, log) = mock(vec![
            ("/oauth/token", vec![(200, new.into())]),
            ("/sync/watched/movies", vec![(200, "[]".into())]),
            ("/sync/watched/shows", vec![(200, "[]".into())]),
        ])
        .await;
        let old = Tokens {
            expires_at: now() + 60,
            ..good()
        };
        let c = client(&base, Some(old));
        assert_eq!(c.history().await.unwrap(), History::default());
        let log = log.lock().unwrap();
        assert!(log[0].0.starts_with("POST /oauth/token"));
        assert!(
            log[0].1.contains("\"grant_type\":\"refresh_token\"")
                && log[0].1.contains("\"refresh_token\":\"ref\"")
        );
        assert!(log[1].0.to_lowercase().contains("authorization: bearer a2"));
        // the refreshed pair is reused, not refreshed again
        assert_eq!(log.len(), 3);
    }

    #[tokio::test]
    async fn refused_refresh_means_reconnect() {
        let (base, _) = mock(vec![("/oauth/token", vec![(401, "{}".into())])]).await;
        let old = Tokens {
            expires_at: 1,
            ..good()
        };
        let c = client(&base, Some(old));
        assert_eq!(c.username().await, Err(Error::Auth));
    }

    #[test]
    fn tokens_round_trip_and_staleness() {
        let t = good();
        assert_eq!(Tokens::from_json(&t.to_json()), Some(t.clone()));
        assert!(!t.stale(now()));
        assert!(
            Tokens {
                expires_at: now() + 100,
                ..t
            }
            .stale(now())
        );
    }
}
