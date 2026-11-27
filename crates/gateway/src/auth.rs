//! Who may call the gateway: client token extraction (every spelling the plan lists), the one gateway user (Argon2id), tokens
//! (256 random bits, only SHA-256 stored, revocable), Quick Connect, and the sign-in routes.
//! Nothing here ever sees a server password or a server token.
use crate::server::{Access, ClientIp, Routes};
use crate::state::State;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::body::Bytes;
use axum::extract::{Query, State as St};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json};
use nocturne_core::gateway::{PendingCode, TokenInfo};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- client info and token extraction

/// What a client says about itself and the token it presented, from any of the accepted spellings.
#[derive(Clone, Default)]
pub struct ClientInfo {
    pub client: String,
    pub device: String,
    pub device_id: String,
    pub version: String,
    pub token: Option<String>,
}

impl std::fmt::Debug for ClientInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientInfo")
            .field("client", &self.client)
            .field("device", &self.device)
            .field("device_id", &self.device_id)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// `%XX` decoding; `plus` turns `+` into a space (query strings).
pub fn pct_decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = |c: u8| (c as char).to_digit(16);
                if let (Some(a), Some(c)) = (h(b[i + 1]), h(b[i + 2])) {
                    out.push((a * 16 + c) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse `MediaBrowser Client="x", Device="y", DeviceId="z", Version="1", Token="t"` (the scheme word is `MediaBrowser` or `Emby`, or absent). Values may be quoted, may hold commas inside quotes, and are percent-decoded.
pub fn parse_authorization(v: &str) -> ClientInfo {
    let mut ci = ClientInfo::default();
    let v = v.trim();
    let rest = match v.split_once(' ') {
        Some((scheme, rest)) if !scheme.contains('=') => rest,
        _ => v,
    };
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && matches!(b[i], b' ' | b',') {
            i += 1;
        }
        let ks = i;
        while i < b.len() && b[i] != b'=' && b[i] != b',' {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        let key = rest[ks..i].trim().to_ascii_lowercase();
        i += 1;
        let val = if i < b.len() && b[i] == b'"' {
            i += 1;
            let vs = i;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            let v = &rest[vs..i];
            i = (i + 1).min(b.len());
            v
        } else {
            let vs = i;
            while i < b.len() && b[i] != b',' {
                i += 1;
            }
            rest[vs..i].trim()
        };
        let val = pct_decode(val, false);
        match key.as_str() {
            "client" => ci.client = val,
            "device" => ci.device = val,
            "deviceid" => ci.device_id = val,
            "version" => ci.version = val,
            "token" if !val.is_empty() => ci.token = Some(val),
            _ => {}
        }
    }
    ci
}

fn fill(dst: &mut String, src: String) {
    if dst.is_empty() {
        *dst = src;
    }
}

/// Every token spelling the plan accepts, in this order of precedence: `Authorization`, `X-Emby-Authorization`, `X-Emby-Token`, `X-MediaBrowser-Token`, query `ApiKey` / `api_key`. Device fields come from the same headers, then from query `deviceId`. Names are matched case-insensitively.
pub fn client_info(headers: &HeaderMap, query: Option<&str>) -> ClientInfo {
    let mut ci = ClientInfo::default();
    for name in ["authorization", "x-emby-authorization"] {
        for v in headers.get_all(name) {
            let Ok(v) = v.to_str() else { continue };
            let p = parse_authorization(v);
            fill(&mut ci.client, p.client);
            fill(&mut ci.device, p.device);
            fill(&mut ci.device_id, p.device_id);
            fill(&mut ci.version, p.version);
            if ci.token.is_none() {
                ci.token = p.token;
            }
        }
    }
    for name in ["x-emby-token", "x-mediabrowser-token"] {
        if ci.token.is_none()
            && let Some(v) = headers.get(name).and_then(|v| v.to_str().ok())
            && !v.trim().is_empty()
        {
            ci.token = Some(v.trim().to_string());
        }
    }
    for pair in query.unwrap_or("").split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = pct_decode(v, true);
        match k.to_ascii_lowercase().as_str() {
            "apikey" | "api_key" if ci.token.is_none() && !v.is_empty() => ci.token = Some(v),
            "deviceid" => fill(&mut ci.device_id, v),
            _ => {}
        }
    }
    ci
}

// ---------------------------------------------------------------- randomness, time

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the system random source works");
    b
}

pub fn random_hex<const N: usize>() -> String {
    hex(&random_bytes::<N>())
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `2026-11-27T10:20:30.0000000Z`, the shape Jellyfin clients parse.
pub fn iso(unix: u64) -> String {
    let (days, rem) = (unix / 86_400, unix % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.0000000Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// ---------------------------------------------------------------- tokens

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenRec {
    /// Public handle (first 16 hex digits of the hash): what the UI shows and revokes by.
    pub id: String,
    /// SHA-256 of the token, hex. The token itself is never stored.
    hash: String,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub version: String,
    pub created: u64,
    pub last_used: u64,
}

impl TokenRec {
    pub fn info(&self) -> TokenInfo {
        TokenInfo {
            id: self.id.clone(),
            client: self.client.clone(),
            device_name: self.device_name.clone(),
            device_id: self.device_id.clone(),
            created: self.created,
            last_used: self.last_used,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct TokenFile {
    version: u32,
    tokens: Vec<TokenRec>,
}

const MAX_TOKENS: usize = 200;

pub struct Tokens {
    path: Option<PathBuf>,
    inner: Mutex<Vec<TokenRec>>,
    dirty: AtomicBool,
}

fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

impl Tokens {
    pub fn open(path: Option<PathBuf>) -> Self {
        let tokens = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<TokenFile>(&b).ok())
            .map(|f| f.tokens)
            .unwrap_or_default();
        Self {
            path,
            inner: Mutex::new(tokens),
            dirty: AtomicBool::new(false),
        }
    }

    /// A new token for this device (a second sign-in of the same device id replaces its old token). Returns the token (shown once, never stored) and its record.
    pub fn mint(&self, ci: &ClientInfo) -> (String, TokenRec) {
        let token = random_hex::<32>();
        let hash = sha256_hex(&token);
        let now = unix_now();
        let name = |s: &str, d: &str| {
            if s.is_empty() {
                d.to_string()
            } else {
                s.to_string()
            }
        };
        let rec = TokenRec {
            id: hash[..16].to_string(),
            hash,
            client: name(&ci.client, "Unknown client"),
            device_name: name(&ci.device, "Unknown device"),
            device_id: ci.device_id.clone(),
            version: ci.version.clone(),
            created: now,
            last_used: now,
        };
        {
            let mut v = self.inner.lock().unwrap();
            if !rec.device_id.is_empty() {
                v.retain(|t| t.device_id != rec.device_id);
            }
            v.push(rec.clone());
            if v.len() > MAX_TOKENS {
                v.sort_by_key(|t| t.last_used);
                let drop = v.len() - MAX_TOKENS;
                v.drain(..drop);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        let _ = self.persist();
        (token, rec)
    }

    /// The record of a presented token, if it is one of ours and not revoked. Notes the use.
    pub fn lookup(&self, presented: &str) -> Option<TokenRec> {
        if presented.len() != 64 || !presented.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        // Looked up by the hash of the token, so no comparison of secret bytes happens here.
        let hash = sha256_hex(&presented.to_ascii_lowercase());
        let mut v = self.inner.lock().unwrap();
        let t = v.iter_mut().find(|t| t.hash == hash)?;
        let now = unix_now();
        if now >= t.last_used + 60 {
            t.last_used = now;
            self.dirty.store(true, Ordering::Relaxed);
        }
        Some(t.clone())
    }

    pub fn revoke(&self, id: &str) -> bool {
        let mut v = self.inner.lock().unwrap();
        let n = v.len();
        v.retain(|t| t.id != id);
        let gone = v.len() != n;
        drop(v);
        if gone {
            self.dirty.store(true, Ordering::Relaxed);
            let _ = self.persist();
        }
        gone
    }

    pub fn revoke_all(&self) {
        self.inner.lock().unwrap().clear();
        self.dirty.store(true, Ordering::Relaxed);
        let _ = self.persist();
    }

    pub fn list(&self) -> Vec<TokenInfo> {
        let mut l: Vec<_> = self
            .inner
            .lock()
            .unwrap()
            .iter()
            .map(TokenRec::info)
            .collect();
        l.sort_by_key(|t| std::cmp::Reverse(t.last_used));
        l
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn persist(&self) -> std::io::Result<()> {
        let Some(p) = &self.path else { return Ok(()) };
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&TokenFile {
            version: 1,
            tokens: self.inner.lock().unwrap().clone(),
        })
        .map_err(std::io::Error::other)?;
        crate::config::write_private(p, &bytes)
            .inspect_err(|_| self.dirty.store(true, Ordering::Relaxed))
    }
}

// ---------------------------------------------------------------- the one gateway user

#[derive(Serialize, Deserialize, Clone)]
struct UserFile {
    server_id: String,
    user_id: String,
    name: String,
    /// Argon2id PHC string.
    hash: String,
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// 24 characters from a 32-letter alphabet: 120 bits.
pub fn generate_passphrase() -> String {
    random_bytes::<24>()
        .iter()
        .map(|b| ALPHABET[(b & 31) as usize] as char)
        .collect()
}

fn hash_password(pw: &str) -> String {
    let salt = SaltString::encode_b64(&random_bytes::<16>()).expect("16 bytes is a valid salt");
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .expect("argon2 hashes a short password")
        .to_string()
}

fn verify_hash(hash: &str, pw: &str) -> bool {
    PasswordHash::new(hash)
        .is_ok_and(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
}

pub struct Users {
    path: Option<PathBuf>,
    f: Mutex<UserFile>,
    unseen: Mutex<Option<String>>,
    /// At most two password checks at once: Argon2 is deliberately expensive.
    gate: tokio::sync::Semaphore,
}

impl Users {
    /// Load `users.toml`, or create it (random server id, user id, and a generated passphrase kept in memory for [`Users::take_unseen`]).
    pub fn open(path: Option<PathBuf>) -> std::io::Result<Self> {
        let existing = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str::<UserFile>(&t).ok());
        let (f, unseen) = match existing {
            Some(f) => (f, None),
            None => {
                let pw = generate_passphrase();
                let f = UserFile {
                    server_id: random_hex::<16>(),
                    user_id: random_hex::<16>(),
                    name: "nocturne".into(),
                    hash: hash_password(&pw),
                };
                if let Some(p) = &path {
                    crate::config::write_private(
                        p,
                        toml::to_string(&f)
                            .map_err(std::io::Error::other)?
                            .as_bytes(),
                    )?;
                }
                (f, Some(pw))
            }
        };
        Ok(Self {
            path,
            f: Mutex::new(f),
            unseen: Mutex::new(unseen),
            gate: tokio::sync::Semaphore::new(2),
        })
    }

    pub fn name(&self) -> String {
        self.f.lock().unwrap().name.clone()
    }
    pub fn user_id(&self) -> String {
        self.f.lock().unwrap().user_id.clone()
    }
    pub fn server_id(&self) -> String {
        self.f.lock().unwrap().server_id.clone()
    }

    /// Whether the name matches (case-insensitively) and the password is right. The hash is always checked, so the time does not tell a wrong name from a wrong password.
    pub async fn verify(&self, name: &str, pw: &str) -> bool {
        let (hash, name_ok) = {
            let f = self.f.lock().unwrap();
            (f.hash.clone(), f.name.eq_ignore_ascii_case(name.trim()))
        };
        let Ok(_permit) = self.gate.acquire().await else {
            return false;
        };
        let pw = pw.to_string();
        let ok = tokio::task::spawn_blocking(move || verify_hash(&hash, &pw))
            .await
            .unwrap_or(false);
        ok && name_ok
    }

    /// New passphrase (returned once). The caller revokes every token.
    pub async fn regenerate(&self) -> std::io::Result<String> {
        let pw = generate_passphrase();
        let pw2 = pw.clone();
        let hash = tokio::task::spawn_blocking(move || hash_password(&pw2))
            .await
            .map_err(std::io::Error::other)?;
        let text = {
            let mut f = self.f.lock().unwrap();
            f.hash = hash;
            toml::to_string(&*f).map_err(std::io::Error::other)?
        };
        if let Some(p) = &self.path {
            crate::config::write_private(p, text.as_bytes())?;
        }
        *self.unseen.lock().unwrap() = None;
        Ok(pw)
    }

    pub fn unseen(&self) -> bool {
        self.unseen.lock().unwrap().is_some()
    }

    /// The first-start passphrase, once.
    pub fn take_unseen(&self) -> Option<String> {
        self.unseen.lock().unwrap().take()
    }
}

// ---------------------------------------------------------------- Quick Connect

const QC_TTL: Duration = Duration::from_secs(300);
const QC_MAX_OPEN: usize = 3;

struct Qc {
    code: String,
    who: ClientInfo,
    created: Instant,
    approved: bool,
}

#[derive(Default)]
pub struct QuickConnect {
    inner: Mutex<HashMap<String, Qc>>,
}

pub struct QcView {
    pub secret: String,
    pub code: String,
    pub who: ClientInfo,
    pub approved: bool,
    pub created_unix: u64,
}

impl QuickConnect {
    fn prune(m: &mut HashMap<String, Qc>, now: Instant) {
        m.retain(|_, q| now.saturating_duration_since(q.created) < QC_TTL);
    }

    /// A new pairing request; `None` when three are already open.
    pub fn initiate(&self, who: ClientInfo, now: Instant) -> Option<QcView> {
        let mut m = self.inner.lock().unwrap();
        Self::prune(&mut m, now);
        if m.len() >= QC_MAX_OPEN {
            return None;
        }
        let code = loop {
            let n = u32::from_le_bytes(random_bytes::<4>()) % 1_000_000;
            let c = format!("{n:06}");
            if !m.values().any(|q| q.code == c) {
                break c;
            }
        };
        let secret = random_hex::<16>();
        m.insert(
            secret.clone(),
            Qc {
                code: code.clone(),
                who: who.clone(),
                created: now,
                approved: false,
            },
        );
        Some(QcView {
            secret,
            code,
            who,
            approved: false,
            created_unix: unix_now(),
        })
    }

    pub fn peek(&self, secret: &str, now: Instant) -> Option<QcView> {
        let mut m = self.inner.lock().unwrap();
        Self::prune(&mut m, now);
        m.get(secret).map(|q| QcView {
            secret: secret.into(),
            code: q.code.clone(),
            who: q.who.clone(),
            approved: q.approved,
            created_unix: unix_now(),
        })
    }

    /// Approve the request showing `code` (from Settings or an already signed-in client).
    pub fn approve(&self, code: &str, now: Instant) -> bool {
        let mut m = self.inner.lock().unwrap();
        Self::prune(&mut m, now);
        m.values_mut()
            .find(|q| q.code == code.trim())
            .is_some_and(|q| {
                q.approved = true;
                true
            })
    }

    /// Consume an approved request: the client may now sign in with its secret, once.
    pub fn redeem(&self, secret: &str, now: Instant) -> Option<ClientInfo> {
        let mut m = self.inner.lock().unwrap();
        Self::prune(&mut m, now);
        if m.get(secret)?.approved {
            m.remove(secret).map(|q| q.who)
        } else {
            None
        }
    }

    pub fn pending(&self, now: Instant) -> Vec<PendingCode> {
        let mut m = self.inner.lock().unwrap();
        Self::prune(&mut m, now);
        m.values()
            .filter(|q| !q.approved)
            .map(|q| PendingCode {
                code: q.code.clone(),
                device: if q.who.device.is_empty() {
                    q.who.client.clone()
                } else {
                    q.who.device.clone()
                },
            })
            .collect()
    }

    pub fn prune_expired(&self, now: Instant) {
        Self::prune(&mut self.inner.lock().unwrap(), now);
    }
}

// ---------------------------------------------------------------- the authenticated caller

/// Inserted by the gate into every request of an `Auth` or `Remote` route; take it in a handler as `Extension<AuthCtx>`.
#[derive(Clone, Debug)]
pub struct AuthCtx {
    /// The token's public id (never the token).
    pub token_id: String,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub version: String,
}

impl From<TokenRec> for AuthCtx {
    fn from(t: TokenRec) -> Self {
        Self {
            token_id: t.id,
            client: t.client,
            device_name: t.device_name,
            device_id: t.device_id,
            version: t.version,
        }
    }
}

// ---------------------------------------------------------------- JSON shapes

pub fn user_dto(st: &State) -> Value {
    let now = iso(unix_now());
    let remote = st.cfg().gw.remote_control;
    json!({
        "Name": st.users.name(),
        "ServerId": st.users.server_id(),
        "Id": st.users.user_id(),
        "HasPassword": true,
        "HasConfiguredPassword": true,
        "HasConfiguredEasyPassword": false,
        "EnableAutoLogin": false,
        "LastLoginDate": now,
        "LastActivityDate": now,
        "Configuration": {
            "PlayDefaultAudioTrack": true, "SubtitleLanguagePreference": "", "DisplayMissingEpisodes": false,
            "GroupedFolders": [], "SubtitleMode": "Default", "DisplayCollectionsView": false,
            "EnableLocalPassword": false, "OrderedViews": [], "LatestItemsExcludes": [],
            "MyMediaExcludes": [], "HidePlayedInLatest": true, "RememberAudioSelections": true,
            "RememberSubtitleSelections": true, "EnableNextEpisodeAutoPlay": true
        },
        "Policy": {
            "IsAdministrator": false, "IsHidden": true, "IsDisabled": false, "BlockedTags": [],
            "EnableUserPreferenceAccess": true, "AccessSchedules": [], "BlockUnratedItems": [],
            "EnableRemoteControlOfOtherUsers": remote, "EnableSharedDeviceControl": remote,
            "EnableRemoteAccess": true, "EnableLiveTvManagement": false, "EnableLiveTvAccess": false,
            "EnableMediaPlayback": true, "EnableAudioPlaybackTranscoding": true,
            "EnableVideoPlaybackTranscoding": true, "EnablePlaybackRemuxing": true,
            "ForceRemoteSourceTranscoding": false, "EnableContentDeletion": false,
            "EnableContentDeletionFromFolders": [], "EnableContentDownloading": false,
            "EnableSyncTranscoding": false, "EnableMediaConversion": false, "EnabledDevices": [],
            "EnableAllDevices": true, "EnabledChannels": [], "EnableAllChannels": true,
            "EnabledFolders": [], "EnableAllFolders": true, "InvalidLoginAttemptCount": 0,
            "LoginAttemptsBeforeLockout": -1, "MaxActiveSessions": 0, "EnablePublicSharing": false,
            "BlockedMediaFolders": [], "BlockedChannels": [], "RemoteClientBitrateLimit": 0,
            "AuthenticationProviderId": "Jellyfin.Server.Implementations.Users.DefaultAuthenticationProvider",
            "PasswordResetProviderId": "Jellyfin.Server.Implementations.Users.DefaultPasswordResetProvider",
            "SyncPlayAccess": "None"
        }
    })
}

/// The session object of the sign-in result. The session module owns the live session table; this is only the shape clients read right after signing in.
fn session_stub(st: &State, t: &TokenRec) -> Value {
    json!({
        "PlayState": {"CanSeek": false, "IsPaused": false, "IsMuted": false, "RepeatMode": "RepeatNone", "PlaybackOrder": "Default"},
        "AdditionalUsers": [],
        "Capabilities": {"PlayableMediaTypes": [], "SupportedCommands": [], "SupportsMediaControl": false, "SupportsPersistentIdentifier": true},
        "RemoteEndPoint": "",
        "PlayableMediaTypes": [],
        "Id": &sha256_hex(&format!("session:{}", t.id))[..32],
        "UserId": st.users.user_id(),
        "UserName": st.users.name(),
        "Client": t.client,
        "LastActivityDate": iso(t.last_used),
        "LastPlaybackCheckIn": "0001-01-01T00:00:00.0000000Z",
        "DeviceName": t.device_name,
        "DeviceId": t.device_id,
        "ApplicationVersion": t.version,
        "IsActive": true,
        "SupportsMediaControl": false,
        "SupportsRemoteControl": false,
        "NowPlayingQueue": [],
        "NowPlayingQueueFullItems": [],
        "HasCustomDeviceName": false,
        "ServerId": st.users.server_id(),
        "SupportedCommands": []
    })
}

fn auth_result(st: &State, token: &str, rec: &TokenRec) -> Value {
    json!({
        "User": user_dto(st),
        "SessionInfo": session_stub(st, rec),
        "AccessToken": token,
        "ServerId": st.users.server_id(),
    })
}

pub fn status_json(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({"title": msg, "status": code.as_u16()}))).into_response()
}

// ---------------------------------------------------------------- handlers

#[derive(Deserialize)]
struct LoginBody {
    username: Option<String>,
    pw: Option<String>,
    password: Option<String>,
}

async fn by_name(
    St(st): St<State>,
    Extension(ip): Extension<ClientIp>,
    Extension(ci): Extension<ClientInfo>,
    body: Bytes,
) -> Response {
    let Some(b) = crate::jsonbody::parse::<LoginBody>(&body) else {
        return status_json(StatusCode::BAD_REQUEST, "Invalid request");
    };
    let name = b.username.unwrap_or_default();
    let pw = b.pw.or(b.password).unwrap_or_default();
    let now = Instant::now();
    if let Err(wait) = st.limits.login.check(ip.0, &name, now) {
        return too_many(wait);
    }
    if !st.users.verify(&name, &pw).await {
        st.limits.login.failed(ip.0, &name, now);
        return status_json(StatusCode::UNAUTHORIZED, "Invalid username or password");
    }
    st.limits.login.succeeded(ip.0, &name);
    let (token, rec) = st.tokens.mint(&ci);
    Json(auth_result(&st, &token, &rec)).into_response()
}

fn too_many(wait: Duration) -> Response {
    let mut r = status_json(StatusCode::TOO_MANY_REQUESTS, "Too many attempts");
    if let Ok(v) = wait.as_secs().max(1).to_string().parse() {
        r.headers_mut().insert(header::RETRY_AFTER, v);
    }
    r
}

fn qc_json(q: &QcView) -> Value {
    json!({
        "Authenticated": q.approved,
        "Secret": q.secret,
        "Code": q.code,
        "DeviceId": q.who.device_id,
        "DeviceName": q.who.device,
        "AppName": q.who.client,
        "AppVersion": q.who.version,
        "DateAdded": iso(q.created_unix),
    })
}

async fn qc_initiate(
    St(st): St<State>,
    Extension(ip): Extension<ClientIp>,
    Extension(ci): Extension<ClientInfo>,
) -> Response {
    let now = Instant::now();
    if !st
        .limits
        .quick_connect
        .allow(&crate::limits::limit_key(ip.0), now)
    {
        return too_many(Duration::from_secs(2));
    }
    match st.qc.initiate(ci, now) {
        Some(q) => Json(qc_json(&q)).into_response(),
        None => status_json(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many open Quick Connect requests",
        ),
    }
}

#[derive(Deserialize)]
struct SecretQuery {
    secret: String,
}

async fn qc_connect(St(st): St<State>, Query(q): Query<SecretQuery>) -> Response {
    match st.qc.peek(&q.secret, Instant::now()) {
        Some(v) => Json(qc_json(&v)).into_response(),
        None => status_json(StatusCode::NOT_FOUND, "Unknown secret"),
    }
}

#[derive(Deserialize)]
struct CodeQuery {
    code: String,
}

async fn qc_authorize(St(st): St<State>, Query(q): Query<CodeQuery>) -> Response {
    Json(st.qc.approve(&q.code, Instant::now())).into_response()
}

#[derive(Deserialize)]
struct QcLogin {
    secret: String,
}

async fn qc_login(
    St(st): St<State>,
    Extension(ci): Extension<ClientInfo>,
    body: Bytes,
) -> Response {
    let Some(b) = crate::jsonbody::parse::<QcLogin>(&body) else {
        return status_json(StatusCode::BAD_REQUEST, "Invalid request");
    };
    match st.qc.redeem(&b.secret, Instant::now()) {
        Some(mut who) => {
            // The device the pairing request named wins; fall back to the headers of this call.
            if who.device_id.is_empty() {
                who = ci;
            }
            let (token, rec) = st.tokens.mint(&who);
            Json(auth_result(&st, &token, &rec)).into_response()
        }
        None => status_json(StatusCode::UNAUTHORIZED, "Not authorized yet"),
    }
}

async fn me(St(st): St<State>) -> Response {
    Json(user_dto(&st)).into_response()
}

async fn user_by_id(
    St(st): St<State>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match crate::idmap::Gid::parse(&id) {
        Some(g) if Some(g) == crate::idmap::Gid::parse(&st.users.user_id()) => {
            Json(user_dto(&st)).into_response()
        }
        _ => status_json(StatusCode::NOT_FOUND, "Not found"),
    }
}

async fn logout(St(st): St<State>, Extension(a): Extension<AuthCtx>) -> StatusCode {
    st.tokens.revoke(&a.token_id);
    StatusCode::NO_CONTENT
}

/// Sign-in, Quick Connect and the user routes. Registered by `server::build`.
pub fn routes(r: Routes) -> Routes {
    r.add("/Users/AuthenticateByName", Access::Public, post(by_name))
        .add(
            "/QuickConnect/Enabled",
            Access::Public,
            get(|| async { Json(true) }),
        )
        .add(
            "/QuickConnect/Initiate",
            Access::Public,
            post(qc_initiate).get(qc_initiate),
        )
        .add("/QuickConnect/Connect", Access::Public, get(qc_connect))
        .add("/QuickConnect/Authorize", Access::Auth, post(qc_authorize))
        .add(
            "/Users/AuthenticateWithQuickConnect",
            Access::Public,
            post(qc_login),
        )
        .add("/Users/Me", Access::Auth, get(me))
        .add("/Users/{userId}", Access::Auth, get(user_by_id))
        .add("/Sessions/Logout", Access::Auth, post(logout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn hm(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    const T: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn authorization_header_forms() {
        let ci = parse_authorization(
            r#"MediaBrowser Client="Jellyfin Web", Device="Fire%20Fox", DeviceId="abc 1", Version="10.11.6", Token="tok""#,
        );
        assert_eq!(
            (
                ci.client.as_str(),
                ci.device.as_str(),
                ci.device_id.as_str()
            ),
            ("Jellyfin Web", "Fire Fox", "abc 1")
        );
        assert_eq!(
            (ci.version.as_str(), ci.token.as_deref()),
            ("10.11.6", Some("tok"))
        );
        assert_eq!(
            parse_authorization(r#"MediaBrowser Token="only""#)
                .token
                .as_deref(),
            Some("only")
        );
        assert_eq!(
            parse_authorization(r#"Emby Token="e", Client="c""#).client,
            "c"
        );
        assert_eq!(
            parse_authorization(r#"MediaBrowser Device="Pixel 7, test", Token="t""#).device,
            "Pixel 7, test"
        );
        assert_eq!(
            parse_authorization("MediaBrowser Client=Plain, Token=raw")
                .token
                .as_deref(),
            Some("raw")
        );
        assert_eq!(
            parse_authorization(r#"MediaBrowser Client="x""#).token,
            None
        );
        assert_eq!(parse_authorization(r#"MediaBrowser Token="""#).token, None);
        assert_eq!(
            parse_authorization(r#"Token="t""#).token.as_deref(),
            Some("t")
        );
        assert_eq!(parse_authorization("garbage ==,,\"\"").token, None);
        assert_eq!(parse_authorization("").token, None);
    }

    #[test]
    fn every_token_spelling_is_found() {
        let a = format!(r#"MediaBrowser Client="c", Token="{T}""#);
        for (h, q) in [
            (hm(&[("authorization", &a)]), None),
            (hm(&[("x-emby-authorization", &a)]), None),
            (hm(&[("X-Emby-Token", T)]), None),
            (hm(&[("x-mediabrowser-token", T)]), None),
            (hm(&[]), Some(format!("ApiKey={T}"))),
            (hm(&[]), Some(format!("x=1&api_key={T}&y=2"))),
            (hm(&[]), Some(format!("APIKEY={T}"))),
        ] {
            let ci = client_info(&h, q.as_deref());
            assert_eq!(ci.token.as_deref(), Some(T), "{h:?} {q:?}");
        }
        assert_eq!(client_info(&hm(&[]), Some("foo=bar")).token, None);
        assert_eq!(client_info(&hm(&[]), None).token, None);
        assert_eq!(client_info(&hm(&[]), Some("ApiKey=")).token, None);
    }

    #[test]
    fn header_beats_query_and_devices_merge() {
        let h = hm(&[(
            "authorization",
            r#"MediaBrowser Token="fromheader", DeviceId="d1""#,
        )]);
        let ci = client_info(&h, Some("ApiKey=fromquery&deviceId=d2&x=%41+b"));
        assert_eq!(
            (ci.token.as_deref(), ci.device_id.as_str()),
            (Some("fromheader"), "d1")
        );
        assert_eq!(
            client_info(&hm(&[]), Some("deviceId=a%20b+c")).device_id,
            "a b c"
        );
        assert!(
            !format!("{ci:?}").contains("fromheader"),
            "Debug hides the token"
        );
    }

    #[test]
    fn percent_decoding_edges() {
        assert_eq!(pct_decode("a%2", false), "a%2");
        assert_eq!(pct_decode("%zz", false), "%zz");
        assert_eq!(pct_decode("%41%42", false), "AB");
        assert_eq!(pct_decode("%", false), "%");
        assert_eq!(pct_decode("a+b", false), "a+b");
        assert_eq!(pct_decode("%c3%a9", false), "é");
    }

    #[test]
    fn dates_are_iso() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.0000000Z");
        assert_eq!(iso(1_796_000_000), "2026-11-30T00:53:20.0000000Z");
        assert_eq!(iso(951_782_400), "2000-02-29T00:00:00.0000000Z");
    }

    #[test]
    fn tokens_hash_revoke_and_replace() {
        let t = Tokens::open(None);
        let ci = ClientInfo {
            device_id: "dev1".into(),
            device: "Phone".into(),
            ..Default::default()
        };
        let (tok, rec) = t.mint(&ci);
        assert_eq!(tok.len(), 64);
        assert_ne!(tok, rec.hash, "only the hash is kept");
        assert!(!serde_json::to_string(&rec).unwrap().contains(&tok));
        assert_eq!(t.lookup(&tok).unwrap().id, rec.id);
        assert_eq!(
            t.lookup(&tok.to_uppercase()).unwrap().id,
            rec.id,
            "hex case does not matter"
        );
        assert!(t.lookup(&"0".repeat(64)).is_none());
        assert!(t.lookup("short").is_none());
        assert!(t.lookup(&format!("{}x", &tok[..63])).is_none());
        // The same device signing in again gets a new token and the old one dies.
        let (tok2, rec2) = t.mint(&ci);
        assert_ne!(tok, tok2);
        assert!(t.lookup(&tok).is_none() && t.lookup(&tok2).is_some());
        assert_eq!(t.len(), 1);
        assert!(!t.revoke("nope") && t.revoke(&rec2.id));
        assert!(t.lookup(&tok2).is_none() && t.is_empty());
        t.mint(&ClientInfo {
            device_id: "a".into(),
            ..Default::default()
        });
        t.mint(&ClientInfo {
            device_id: "b".into(),
            ..Default::default()
        });
        t.revoke_all();
        assert!(t.is_empty());
    }

    #[test]
    fn tokens_persist_hashes_only() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("tokens.json");
        let t = Tokens::open(Some(p.clone()));
        let (tok, _) = t.mint(&ClientInfo::default());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains(&tok), "the token never reaches the disk");
        let again = Tokens::open(Some(p));
        assert!(
            again.lookup(&tok).is_some(),
            "but it still validates after a restart"
        );
    }

    #[test]
    fn token_table_is_bounded() {
        let t = Tokens::open(None);
        for i in 0..MAX_TOKENS + 20 {
            t.mint(&ClientInfo {
                device_id: format!("d{i}"),
                ..Default::default()
            });
        }
        assert_eq!(t.len(), MAX_TOKENS);
    }

    #[tokio::test]
    async fn user_password_lifecycle() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("users.toml");
        let u = Users::open(Some(p.clone())).unwrap();
        assert!(u.unseen());
        let pw = u.take_unseen().unwrap();
        assert_eq!(pw.len(), 24);
        assert!(u.take_unseen().is_none(), "shown once");
        assert!(u.verify("Nocturne", &pw).await);
        assert!(!u.verify("nocturne", "wrong").await);
        assert!(!u.verify("someone", &pw).await);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("$argon2id$") && !text.contains(&pw));
        // A restart keeps the user and shows no new password.
        let u2 = Users::open(Some(p)).unwrap();
        assert!(!u2.unseen() && u2.user_id() == u.user_id() && u2.server_id() == u.server_id());
        assert!(u2.verify("nocturne", &pw).await);
        let new = u2.regenerate().await.unwrap();
        assert_ne!(new, pw);
        assert!(!u2.verify("nocturne", &pw).await && u2.verify("nocturne", &new).await);
    }

    #[test]
    fn passphrases_differ_and_use_the_alphabet() {
        let (a, b) = (generate_passphrase(), generate_passphrase());
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| ALPHABET.contains(&c)));
    }

    #[test]
    fn quick_connect_flow_and_limits() {
        let q = QuickConnect::default();
        let t = Instant::now();
        let who = ClientInfo {
            device: "TV".into(),
            device_id: "tv1".into(),
            ..Default::default()
        };
        let a = q.initiate(who.clone(), t).unwrap();
        assert_eq!(a.code.len(), 6);
        assert!(!q.peek(&a.secret, t).unwrap().approved);
        assert!(q.redeem(&a.secret, t).is_none(), "not approved yet");
        assert_eq!(q.pending(t)[0].device, "TV");
        assert!(!q.approve("nope", t));
        assert!(q.approve(&a.code, t));
        assert!(q.peek(&a.secret, t).unwrap().approved && q.pending(t).is_empty());
        assert_eq!(q.redeem(&a.secret, t).unwrap().device_id, "tv1");
        assert!(q.redeem(&a.secret, t).is_none(), "once");
        // At most three open codes, five minute lifetime.
        for _ in 0..3 {
            assert!(q.initiate(who.clone(), t).is_some());
        }
        assert!(q.initiate(who.clone(), t).is_none());
        assert!(
            q.initiate(who, t + QC_TTL + Duration::from_secs(1))
                .is_some()
        );
    }
}
