//! `sources`: the media servers this server connects to, as the desktop app does: Jellyfin servers (sign-in by password, Quick Connect or API key) and other NOCTURNE servers (password or API key). Records in the `sources` table, tokens only in `sh.secrets` (`source:<id>`), health checked on demand and every few minutes while the module runs.
//!
//! The read handle works whether or not the module runs (the gateway and the library module need the sources even with this module's routes off): [`list`] gives every source with its token, [`subscribe`] tells when the list changed. The module itself adds the management routes and the health checks.
use crate::api::{ApiErr, ApiResult, ok, saved};
use crate::auth::Caller;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::rec;
use crate::state::{Shared, now};
use axum::extract::{Path, State};
use axum::{Extension, Json};
use nocturne_jellyfin_user::{Error as JfError, UserClient};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::watch;

const NAME: &str = "sources";
/// How often a running module checks every source.
const CHECK_EVERY: Duration = Duration::from_secs(300);
const TIMEOUT: Duration = Duration::from_secs(20);
const UA: &str = concat!("NOCTURNE/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Jellyfin,
    Nocturne,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignIn {
    #[default]
    Password,
    QuickConnect,
    ApiKey,
    /// Taken over from the desktop app's keyring at first start.
    Imported,
}

/// One source. No secret in here: the token is in `sh.secrets` under `source:<id>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceRec {
    pub id: u64,
    pub kind: SourceKind,
    pub label: String,
    /// Base address, no trailing slash.
    pub url: String,
    pub enabled: bool,
    /// The account on that server (Jellyfin user id; empty for a NOCTURNE server).
    pub user_id: String,
    pub user_name: String,
    /// The server's own id (Jellyfin `ServerId`); the gateway's id map keys on it. A NOCTURNE server: its address.
    pub server_id: String,
    pub sign_in: SignIn,
    /// Unix seconds.
    pub added: u64,
}
rec!(SourceRec, "sources");

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Health {
    pub reachable: bool,
    pub signed_in: bool,
    /// Unix seconds; 0 = never checked.
    pub checked: u64,
    pub error: Option<String>,
}

/// A source's token. Never printed: `Debug` is redacted; pass [`Token::expose`] only into an auth header.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// What the gateway and the library module read.
#[derive(Clone, Debug)]
pub struct Source {
    pub rec: SourceRec,
    /// `None`: signed out (no stored token).
    pub token: Option<Token>,
    pub health: Health,
}

/// The shared cache behind [`list`] (`sh.ext::<Store>()`).
pub struct Store {
    cache: Mutex<Option<Vec<Source>>>,
    changed: watch::Sender<u64>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            cache: Mutex::new(None),
            changed: watch::channel(0).0,
        }
    }
}

fn secret_name(id: u64) -> String {
    format!("source:{id}")
}

/// Every source in id order, with its token and last health. The first call reads the database and the keyring (blocking: from a runtime thread use [`load`] first); later calls are a cache read.
pub fn list(sh: &Shared) -> Vec<Source> {
    let store = sh.ext::<Store>();
    let mut c = store.cache.lock().unwrap();
    if c.is_none() {
        let recs = sh.db.all::<SourceRec>().unwrap_or_default();
        *c = Some(
            recs.into_iter()
                .map(|rec| Source {
                    token: sh.secrets.get(&secret_name(rec.id)).map(Token),
                    rec,
                    health: Health::default(),
                })
                .collect(),
        );
    }
    c.clone().unwrap_or_default()
}

/// Fill the cache off the runtime threads (the keyring is a blocking D-Bus round trip).
pub async fn load(sh: &Shared) -> Vec<Source> {
    let sh2 = sh.clone();
    tokio::task::spawn_blocking(move || list(&sh2))
        .await
        .unwrap_or_default()
}

/// Ticks whenever a source is added, changed, removed, or its health changes.
pub fn subscribe(sh: &Shared) -> watch::Receiver<u64> {
    sh.ext::<Store>().changed.subscribe()
}

fn changed(sh: &Shared) {
    sh.ext::<Store>().changed.send_modify(|n| *n += 1);
}

fn forget_cache(sh: &Shared) {
    *sh.ext::<Store>().cache.lock().unwrap() = None;
}

fn set_health(sh: &Shared, id: u64, h: Health) {
    let store = sh.ext::<Store>();
    let mut moved = false;
    if let Some(v) = store.cache.lock().unwrap().as_mut()
        && let Some(s) = v.iter_mut().find(|s| s.rec.id == id)
    {
        moved = s.health.reachable != h.reachable || s.health.signed_in != h.signed_in;
        s.health = h.clone();
    }
    if moved {
        sh.module_event(NAME, "health", json!({"id": id, "health": h}));
        changed(sh);
    }
}

/// Store a new or re-signed-in source: the record, then its token.
fn save(sh: &Shared, mut rec: SourceRec, token: &str) -> Result<u64, String> {
    let id = sh.db.put(&mut rec)?;
    sh.secrets.set(&secret_name(id), Some(token))?;
    forget_cache(sh);
    changed(sh);
    sh.module_event(NAME, "changed", json!({"id": id}));
    Ok(id)
}

// ---------------------------------------------------------------- talking to a source

/// `http(s)://host[:port][/base]`, no trailing slash, no query.
pub fn normalize_url(u: &str) -> Result<String, String> {
    let u = u.trim().trim_end_matches('/');
    let ok = (u.starts_with("http://") || u.starts_with("https://"))
        && u.split("://")
            .nth(1)
            .is_some_and(|r| !r.is_empty() && !r.starts_with('/'))
        && !u.contains(['?', '#', ' ']);
    if ok {
        Ok(u.to_string())
    } else {
        Err("the address must start with http:// or https://".into())
    }
}

fn jf_err(e: JfError) -> String {
    match e {
        JfError::InvalidCredentials => "wrong user name or password".into(),
        JfError::Status { status, .. } => format!("the server answered {status}"),
        JfError::Http(_) => "the server cannot be reached".into(),
        JfError::Config(m) => m,
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        // Servers behind Cloudflare refuse requests without a User-Agent (error 1010).
        .user_agent(UA)
        .build()
        .expect("static client")
}

/// A Jellyfin server's id and name (`/System/Info/Public`, no sign-in).
async fn jf_public(url: &str) -> Result<(String, String), String> {
    let i = UserClient::anonymous(url)
        .map_err(jf_err)?
        .public_info()
        .await
        .map_err(jf_err)?;
    Ok((i.id, i.server_name))
}

/// A Jellyfin API key belongs to no user: find the account by name with `/Users`.
async fn jf_api_key_user(url: &str, key: &str, user: &str) -> Result<(String, String), String> {
    let r = http()
        .get(format!("{url}/Users"))
        .header(
            "Authorization",
            format!(r#"MediaBrowser Client="NOCTURNE Server", Device="server", DeviceId="nocturne-server", Version="{}", Token="{key}""#, env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|_| "the server cannot be reached".to_string())?;
    if r.status() == 401 {
        return Err("the server does not accept this API key".into());
    }
    let users: Value = r.json().await.map_err(|_| "not a Jellyfin server")?;
    users
        .as_array()
        .into_iter()
        .flatten()
        .find(|u| {
            u["Name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(user))
        })
        .map(|u| {
            (
                u["Id"].as_str().unwrap_or("").to_string(),
                u["Name"].as_str().unwrap_or("").to_string(),
            )
        })
        .ok_or_else(|| format!("no user {user:?} on that server"))
}

/// A NOCTURNE server: the account behind a token (`/api/v1/auth/me`).
async fn nocturne_me(url: &str, token: &str) -> Result<String, Option<String>> {
    let r = http()
        .get(format!("{url}/api/v1/auth/me"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| Some("the server cannot be reached".to_string()))?;
    if r.status() == 401 {
        return Err(None);
    }
    let v: Value = r
        .json()
        .await
        .map_err(|_| Some("not a NOCTURNE server".to_string()))?;
    Ok(v["name"].as_str().unwrap_or("").to_string())
}

async fn nocturne_login(url: &str, user: &str, pw: &str) -> Result<String, String> {
    let r = http()
        .post(format!("{url}/api/v1/auth/login"))
        .json(&json!({"username": user, "password": pw}))
        .send()
        .await
        .map_err(|_| "the server cannot be reached".to_string())?;
    if r.status() == 401 {
        return Err("wrong user name or password".into());
    }
    let v: Value = r.json().await.map_err(|_| "not a NOCTURNE server")?;
    v["token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "not a NOCTURNE server".into())
}

/// Ask the source whether it is up and the token still works.
pub async fn check(s: &Source) -> Health {
    let mut h = Health {
        checked: now(),
        ..Default::default()
    };
    let Some(t) = &s.token else {
        h.reachable = match s.rec.kind {
            SourceKind::Jellyfin => jf_public(&s.rec.url).await.is_ok(),
            SourceKind::Nocturne => http()
                .get(format!("{}/api/v1/health", s.rec.url))
                .send()
                .await
                .is_ok(),
        };
        h.error = Some("signed out".into());
        return h;
    };
    match s.rec.kind {
        SourceKind::Jellyfin => {
            // `/Users/{id}` works for a user session and an API key alike.
            let r = http()
                .get(format!("{}/Users/{}", s.rec.url, s.rec.user_id))
                .header(
                    "Authorization",
                    format!(r#"MediaBrowser Client="NOCTURNE Server", Device="server", DeviceId="nocturne-server", Version="{}", Token="{}""#, env!("CARGO_PKG_VERSION"), t.expose()),
                )
                .send()
                .await;
            match r {
                Ok(r) if r.status().is_success() => {
                    (h.reachable, h.signed_in) = (true, true);
                }
                Ok(r) if r.status() == 401 => {
                    h.reachable = true;
                    h.error = Some("the server no longer accepts the sign-in".into());
                }
                Ok(r) => {
                    h.reachable = true;
                    h.error = Some(format!("the server answered {}", r.status().as_u16()));
                }
                Err(_) => h.error = Some("the server cannot be reached".into()),
            }
        }
        SourceKind::Nocturne => match nocturne_me(&s.rec.url, t.expose()).await {
            Ok(_) => (h.reachable, h.signed_in) = (true, true),
            Err(None) => {
                h.reachable = true;
                h.error = Some("the server no longer accepts the sign-in".into());
            }
            Err(Some(e)) => h.error = Some(e),
        },
    }
    h
}

async fn check_all(sh: &Shared) {
    for s in load(sh).await.into_iter().filter(|s| s.rec.enabled) {
        let h = check(&s).await;
        set_health(sh, s.rec.id, h);
    }
}

// ---------------------------------------------------------------- catalog for the library manager

/// What the Jellyfin sources hold, for the library manager ([`crate::library::Catalog`]): a monitored movie or episode found here counts as available and is never grabbed. Built from one listing per source (movies, series, episodes with provider ids) and rebuilt with every health round.
/// ponytail: movies and episodes only; music and books are never matched (add albums and books when the library manager monitors them from Jellyfin sources).
#[derive(Default)]
pub struct SourceCatalog {
    movies: std::sync::RwLock<std::collections::HashSet<String>>,
    episodes: std::sync::RwLock<std::collections::HashSet<(String, u32, u32)>>,
}

/// Match keys of a title: provider ids, then lowercased name with year.
fn keys(ids: &nocturne_core::server::types::Ids, name: &str, year: Option<u16>) -> Vec<String> {
    let mut k = vec![];
    if let Some(i) = &ids.imdb {
        k.push(format!("imdb:{}", i.to_ascii_lowercase()));
    }
    if let Some(i) = ids.tmdb {
        k.push(format!("tmdb:{i}"));
    }
    if let Some(i) = ids.tvdb {
        k.push(format!("tvdb:{i}"));
    }
    if !name.is_empty() {
        k.push(format!(
            "name:{}:{}",
            name.trim().to_lowercase(),
            year.unwrap_or(0)
        ));
    }
    k
}

/// The same keys from a Jellyfin item (`ProviderIds`, `Name`, `ProductionYear`).
fn item_keys(v: &Value) -> Vec<String> {
    let p = &v["ProviderIds"];
    let pid = |n: &str| {
        p.as_object()
            .and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)))
            .and_then(|(_, v)| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let ids = nocturne_core::server::types::Ids {
        imdb: pid("Imdb"),
        tmdb: pid("Tmdb").and_then(|s| s.parse().ok()),
        tvdb: pid("Tvdb").and_then(|s| s.parse().ok()),
        ..Default::default()
    };
    let year = v["ProductionYear"]
        .as_u64()
        .and_then(|y| u16::try_from(y).ok());
    keys(&ids, v["Name"].as_str().unwrap_or(""), year)
}

impl crate::library::Catalog for SourceCatalog {
    fn has(&self, t: &nocturne_core::server::Title, u: &nocturne_core::server::Unit) -> bool {
        use nocturne_core::server::types::MediaKind;
        let tk = keys(&t.ids, &t.name, t.year);
        match t.kind {
            MediaKind::Movie => {
                let m = self.movies.read().unwrap();
                tk.iter().any(|k| m.contains(k))
            }
            MediaKind::Series => {
                let (Some(se), Some(ep)) = (u.season, u.episode) else {
                    return false;
                };
                let e = self.episodes.read().unwrap();
                tk.iter().any(|k| e.contains(&(k.clone(), se, ep)))
            }
            _ => false,
        }
    }
}

async fn jf_items(s: &Source, t: &Token, query: &str) -> Vec<Value> {
    let r = http()
        .get(format!("{}/Items?userId={}&Recursive=true&{query}", s.rec.url, s.rec.user_id))
        .timeout(Duration::from_secs(120))
        .header(
            "Authorization",
            format!(r#"MediaBrowser Client="NOCTURNE Server", Device="server", DeviceId="nocturne-server", Version="{}", Token="{}""#, env!("CARGO_PKG_VERSION"), t.expose()),
        )
        .send()
        .await;
    let Some(r) = r.ok().filter(|r| r.status().is_success()) else {
        return vec![];
    };
    r.json::<Value>()
        .await
        .ok()
        .and_then(|v| v["Items"].as_array().cloned())
        .unwrap_or_default()
}

/// Rebuild the catalog from every enabled, signed-in Jellyfin source.
async fn refresh_catalog(sh: &Shared) {
    let (mut movies, mut episodes) = (
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    );
    for s in load(sh)
        .await
        .into_iter()
        .filter(|s| s.rec.enabled && s.rec.kind == SourceKind::Jellyfin)
    {
        let Some(t) = &s.token else { continue };
        for m in jf_items(
            &s,
            t,
            "IncludeItemTypes=Movie&Fields=ProviderIds,ProductionYear",
        )
        .await
        {
            movies.extend(item_keys(&m));
        }
        let series: std::collections::HashMap<String, Vec<String>> = jf_items(
            &s,
            t,
            "IncludeItemTypes=Series&Fields=ProviderIds,ProductionYear",
        )
        .await
        .iter()
        .filter_map(|v| Some((v["Id"].as_str()?.to_string(), item_keys(v))))
        .collect();
        for e in jf_items(
            &s,
            t,
            "IncludeItemTypes=Episode&Fields=ParentIndexNumber,IndexNumber",
        )
        .await
        {
            let (Some(sid), Some(se), Some(ep)) = (
                e["SeriesId"].as_str(),
                e["ParentIndexNumber"].as_u64(),
                e["IndexNumber"].as_u64(),
            ) else {
                continue;
            };
            for k in series.get(sid).into_iter().flatten() {
                episodes.insert((k.clone(), se as u32, ep as u32));
            }
        }
    }
    let c = sh.ext::<SourceCatalog>();
    *c.movies.write().unwrap() = movies;
    *c.episodes.write().unwrap() = episodes;
}

// ---------------------------------------------------------------- import from the desktop app

/// Once per server data directory: the desktop app's Jellyfin accounts (`sources.toml` next to `server.toml`, tokens from the app's keyring entries, which are only read, never changed or deleted). Only with the real keyring: tests (memory secrets) never touch the user's keyring. Returns how many were added.
pub fn import_app_accounts(sh: &Shared) -> usize {
    const FLAG: &str = "import:app_accounts";
    if !matches!(sh.secrets, crate::state::Secrets::Keyring)
        || sh.db.meta(FLAG).ok().flatten().is_some()
    {
        return 0;
    }
    let mut n = 0;
    let path = sh.paths.config.with_file_name("sources.toml");
    let app = nocturne_core::profile::Sources::load_from(&path)
        .ok()
        .flatten()
        .unwrap_or_default();
    let have = list(sh);
    for p in &app.profiles {
        let nocturne_core::profile::ProfileKind::Jellyfin {
            url,
            user_id,
            user_name,
        } = &p.kind
        else {
            continue;
        };
        let Ok(url) = normalize_url(url) else {
            continue;
        };
        if have
            .iter()
            .any(|s| s.rec.url == url && s.rec.user_id == *user_id)
        {
            continue;
        }
        let session = match nocturne_session::load_account(&url, user_id) {
            Ok(Some(s)) => Some(s),
            _ => nocturne_session::load(&url)
                .ok()
                .flatten()
                .filter(|s| s.user_id == *user_id),
        };
        let Some(s) = session else { continue };
        let rec = SourceRec {
            kind: SourceKind::Jellyfin,
            label: p.label.clone(),
            url,
            enabled: p.enabled,
            user_id: user_id.clone(),
            user_name: user_name.clone(),
            server_id: s.server_id.clone(),
            sign_in: SignIn::Imported,
            added: now(),
            ..Default::default()
        };
        if save(sh, rec, &s.access_token).is_ok() {
            n += 1;
        }
    }
    let _ = sh.db.set_meta(FLAG, 1);
    n
}

// ---------------------------------------------------------------- routes

fn admin(c: &Caller) -> Result<(), ApiErr> {
    if c.admin() {
        Ok(())
    } else {
        Err(ApiErr::forbidden("only an administrator manages sources"))
    }
}

/// A source as the API shows it: never a token.
fn view(s: &Source) -> Value {
    let mut v = serde_json::to_value(&s.rec).unwrap_or_default();
    v["signed_in"] = json!(s.token.is_some());
    v["health"] = serde_json::to_value(&s.health).unwrap_or_default();
    v
}

async fn get_list(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    Ok(Json(json!(
        load(&sh).await.iter().map(view).collect::<Vec<_>>()
    )))
}

async fn get_one(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    admin(&c)?;
    let s = load(&sh)
        .await
        .into_iter()
        .find(|s| s.rec.id == id)
        .ok_or_else(ApiErr::not_found)?;
    Ok(Json(view(&s)))
}

/// Sign-in details of `POST /sources` and `POST /sources/{id}/signin`: user name and password, or an API key (Jellyfin: with the user name it acts for).
#[derive(Deserialize, Default)]
#[serde(default)]
struct AddBody {
    kind: SourceKind,
    url: String,
    label: Option<String>,
    username: String,
    password: Option<String>,
    api_key: Option<String>,
}

/// Sign in to a source: the record (without id) and the token.
async fn sign_in(b: &AddBody) -> Result<(SourceRec, String), String> {
    let url = normalize_url(&b.url)?;
    let mut rec = SourceRec {
        kind: b.kind,
        url: url.clone(),
        enabled: true,
        added: now(),
        ..Default::default()
    };
    let token = match (b.kind, &b.api_key, &b.password) {
        (SourceKind::Jellyfin, Some(key), _) => {
            let (sid, name) = jf_public(&url).await?;
            let (uid, uname) = jf_api_key_user(&url, key, &b.username).await?;
            (rec.server_id, rec.label, rec.user_id, rec.user_name) = (sid, name, uid, uname);
            rec.sign_in = SignIn::ApiKey;
            key.clone()
        }
        (SourceKind::Jellyfin, None, Some(pw)) => {
            let (_, name) = jf_public(&url).await?;
            let s = UserClient::authenticate_by_name(&url, &b.username, pw)
                .await
                .map_err(jf_err)?;
            (rec.server_id, rec.label, rec.user_id, rec.user_name) = (
                s.server_id.clone(),
                name,
                s.user_id.clone(),
                s.user_name.clone(),
            );
            s.access_token
        }
        (SourceKind::Nocturne, key, pw) => {
            let token = match (key, pw) {
                (Some(k), _) => k.clone(),
                (None, Some(pw)) => nocturne_login(&url, &b.username, pw).await?,
                _ => return Err("give a password or an API key".into()),
            };
            rec.user_name = nocturne_me(&url, &token).await.map_err(|e| {
                e.unwrap_or_else(|| "the server does not accept this sign-in".into())
            })?;
            rec.sign_in = if key.is_some() {
                SignIn::ApiKey
            } else {
                SignIn::Password
            };
            rec.server_id = url.clone();
            rec.label = url.split("://").nth(1).unwrap_or(&url).to_string();
            token
        }
        _ => return Err("give a password or an API key".into()),
    };
    if let Some(l) = b.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        rec.label = l.to_string();
    }
    Ok((rec, token))
}

fn duplicate(sh: &Shared, rec: &SourceRec, except: u64) -> bool {
    list(sh).iter().any(|s| {
        s.rec.id != except
            && s.rec.kind == rec.kind
            && s.rec.server_id == rec.server_id
            && s.rec.user_id == rec.user_id
    })
}

async fn add(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(b): Json<AddBody>,
) -> ApiResult<Value> {
    admin(&c)?;
    // Before signing in: a second sign-in of the same user and device ends the first session on a Jellyfin server.
    if b.kind == SourceKind::Jellyfin
        && let Ok(url) = normalize_url(&b.url)
        && let Ok((sid, _)) = jf_public(&url).await
        && load(&sh).await.iter().any(|s| {
            s.rec.kind == SourceKind::Jellyfin
                && s.rec.server_id == sid
                && s.rec.user_name.eq_ignore_ascii_case(b.username.trim())
        })
    {
        return Err(ApiErr::conflict(
            "this account on this server is already a source",
        ));
    }
    let (rec, token) = sign_in(&b).await?;
    load(&sh).await;
    if duplicate(&sh, &rec, 0) {
        return Err(ApiErr::conflict(
            "this account on this server is already a source",
        ));
    }
    saved(save(&sh, rec, &token)?)
}

/// Sign in again (the token stopped working, or another account): keeps the source's id, label and switch.
async fn re_sign_in(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<u64>,
    Json(mut b): Json<AddBody>,
) -> ApiResult<Value> {
    admin(&c)?;
    let old = load(&sh)
        .await
        .into_iter()
        .find(|s| s.rec.id == id)
        .ok_or_else(ApiErr::not_found)?;
    (b.kind, b.url) = (old.rec.kind, old.rec.url.clone());
    let (mut rec, token) = sign_in(&b).await?;
    if rec.server_id != old.rec.server_id {
        return Err(ApiErr::conflict(
            "that address is now a different server; add it as a new source",
        ));
    }
    (rec.id, rec.label, rec.enabled, rec.added) =
        (id, old.rec.label, old.rec.enabled, old.rec.added);
    saved(save(&sh, rec, &token)?)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Patch {
    label: Option<String>,
    enabled: Option<bool>,
}

async fn patch(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<u64>,
    Json(p): Json<Patch>,
) -> ApiResult<Value> {
    admin(&c)?;
    let mut rec = sh.db.get::<SourceRec>(id)?.ok_or_else(ApiErr::not_found)?;
    if let Some(l) = p.label.as_deref().map(str::trim) {
        if l.is_empty() {
            return Err(ApiErr::bad("the label cannot be empty"));
        }
        rec.label = l.to_string();
    }
    if let Some(e) = p.enabled {
        rec.enabled = e;
    }
    sh.db.put(&mut rec)?;
    forget_cache(&sh);
    changed(&sh);
    ok()
}

/// Remove the source and its token here. Nothing is sent to the server (its session simply stays until it expires there).
async fn remove(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    admin(&c)?;
    sh.db.get::<SourceRec>(id)?.ok_or_else(ApiErr::not_found)?;
    sh.db.delete::<SourceRec>(&[id])?;
    sh.secrets.set(&secret_name(id), None)?;
    forget_cache(&sh);
    changed(&sh);
    sh.module_event(NAME, "removed", json!({"id": id}));
    ok()
}

async fn check_one(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<u64>,
) -> ApiResult<Health> {
    admin(&c)?;
    let s = load(&sh)
        .await
        .into_iter()
        .find(|s| s.rec.id == id)
        .ok_or_else(ApiErr::not_found)?;
    let h = check(&s).await;
    set_health(&sh, id, h.clone());
    Ok(Json(h))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct QcBody {
    url: String,
    secret: String,
    label: Option<String>,
}

/// Quick Connect, step 1: a code for the user to approve in a client already signed in to that Jellyfin server. The secret comes back to this client only and is needed for step 2.
async fn qc_start(Extension(c): Extension<Caller>, Json(b): Json<QcBody>) -> ApiResult<Value> {
    admin(&c)?;
    let url = normalize_url(&b.url)?;
    let cl = UserClient::anonymous(&url).map_err(jf_err)?;
    if !cl.quick_connect_enabled().await.map_err(jf_err)? {
        return Err(ApiErr::bad("Quick Connect is off on that server"));
    }
    let q = cl.quick_connect_initiate().await.map_err(jf_err)?;
    Ok(Json(json!({"code": q.code, "secret": q.secret})))
}

/// Quick Connect, step 2: once the code is approved, the session becomes a source (409 while it is not approved yet: ask again).
async fn qc_finish(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(b): Json<QcBody>,
) -> ApiResult<Value> {
    admin(&c)?;
    let url = normalize_url(&b.url)?;
    let cl = UserClient::anonymous(&url).map_err(jf_err)?;
    if !cl.quick_connect_poll(&b.secret).await.map_err(jf_err)? {
        return Err(ApiErr::conflict("not approved yet"));
    }
    let s = cl
        .authenticate_with_quick_connect(&b.secret)
        .await
        .map_err(jf_err)?;
    let (_, name) = jf_public(&url).await?;
    let rec = SourceRec {
        kind: SourceKind::Jellyfin,
        label: b.label.filter(|l| !l.trim().is_empty()).unwrap_or(name),
        url,
        enabled: true,
        user_id: s.user_id.clone(),
        user_name: s.user_name.clone(),
        server_id: s.server_id.clone(),
        sign_in: SignIn::QuickConnect,
        added: now(),
        ..Default::default()
    };
    load(&sh).await;
    if duplicate(&sh, &rec, 0) {
        return Err(ApiErr::conflict(
            "this account on this server is already a source",
        ));
    }
    saved(save(&sh, rec, &s.access_token)?)
}

// ---------------------------------------------------------------- module

#[derive(Default)]
pub struct Sources {
    stop: Mutex<Option<watch::Sender<bool>>>,
}

impl Module for Sources {
    fn name(&self) -> &'static str {
        NAME
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("", get_list)
            .post("", add)
            .post("/quickconnect", qc_start)
            .post("/quickconnect/complete", qc_finish)
            .get("/{id}", get_one)
            .put("/{id}", patch)
            .delete("/{id}", remove)
            .post("/{id}/signin", re_sign_in)
            .post("/{id}/check", check_one)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            // Not the desktop app's device id: signing in here must not sign the app out of the same server.
            let machine: String = std::fs::read_to_string("/etc/machine-id")
                .unwrap_or_default()
                .trim()
                .chars()
                .take(12)
                .collect();
            nocturne_jellyfin_user::set_device_id(&format!("nocturne-server-{machine}"));
            let sh2 = sh.clone();
            let _ = tokio::task::spawn_blocking(move || import_app_accounts(&sh2)).await;
            let (tx, mut rx) = watch::channel(false);
            *self.stop.lock().unwrap() = Some(tx);
            let sh = sh.clone();
            tokio::spawn(async move {
                *sh.ext::<crate::library::CatalogSlot>().0.write().unwrap() =
                    Some(sh.ext::<SourceCatalog>() as std::sync::Arc<dyn crate::library::Catalog>);
                loop {
                    check_all(&sh).await;
                    refresh_catalog(&sh).await;
                    tokio::select! {
                        () = sh.stopped() => return,
                        _ = rx.changed() => return,
                        () = tokio::time::sleep(CHECK_EVERY) => {}
                    }
                }
            });
            Ok(())
        })
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async move {
            *sh.ext::<crate::library::CatalogSlot>().0.write().unwrap() = None;
            if let Some(tx) = self.stop.lock().unwrap().take() {
                let _ = tx.send(true);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalized_or_refused() {
        assert_eq!(
            normalize_url(" https://jf.example/ ").unwrap(),
            "https://jf.example"
        );
        assert_eq!(
            normalize_url("http://10.0.0.2:8096/jf/").unwrap(),
            "http://10.0.0.2:8096/jf"
        );
        for bad in [
            "jf.example",
            "ftp://x",
            "http://",
            "http:///x",
            "http://x/?a=1",
            "http://x/#f",
        ] {
            assert!(normalize_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_catalog_matches_by_provider_id_then_name_and_year() {
        use crate::library::Catalog;
        use nocturne_core::server::types::{Ids, MediaKind};
        use nocturne_core::server::{Title, Unit};
        let c = SourceCatalog::default();
        c.movies.write().unwrap().extend(item_keys(
            &json!({"Name": "Heat", "ProductionYear": 1995, "ProviderIds": {"Imdb": "tt0113277"}}),
        ));
        c.movies.write().unwrap().extend(item_keys(
            &json!({"Name": "Local Film", "ProductionYear": 2010}),
        ));
        let series = item_keys(&json!({"Name": "Dark", "ProviderIds": {"Tvdb": "334824"}}));
        c.episodes
            .write()
            .unwrap()
            .extend(series.into_iter().map(|k| (k, 1, 2)));
        let title = |kind, name: &str, year, ids| Title {
            id: 1,
            kind,
            name: name.into(),
            year,
            ids,
            profile: 0,
            library: 0,
            monitored: true,
            added: 0,
        };
        let unit = |season, episode| Unit {
            id: 1,
            title: 1,
            name: String::new(),
            season,
            episode,
            year: None,
            air_date: None,
            ids: Ids::default(),
            monitored: true,
            file: None,
        };
        let imdb = Ids {
            imdb: Some("TT0113277".into()),
            ..Default::default()
        };
        assert!(
            c.has(
                &title(MediaKind::Movie, "Other name", None, imdb),
                &unit(None, None)
            ),
            "by imdb id"
        );
        assert!(
            c.has(
                &title(MediaKind::Movie, "local film", Some(2010), Ids::default()),
                &unit(None, None)
            ),
            "by name and year"
        );
        assert!(!c.has(
            &title(MediaKind::Movie, "local film", Some(2011), Ids::default()),
            &unit(None, None)
        ));
        let tvdb = Ids {
            tvdb: Some(334824),
            ..Default::default()
        };
        assert!(c.has(
            &title(MediaKind::Series, "Dark", None, tvdb.clone()),
            &unit(Some(1), Some(2))
        ));
        assert!(!c.has(
            &title(MediaKind::Series, "Dark", None, tvdb),
            &unit(Some(1), Some(3))
        ));
    }

    #[test]
    fn a_token_never_prints() {
        let s = Source {
            rec: SourceRec::default(),
            token: Some(Token("secret-token".into())),
            health: Health::default(),
        };
        assert!(!format!("{s:?}").contains("secret-token"));
        assert!(!view(&s).to_string().contains("secret-token"));
    }
}
