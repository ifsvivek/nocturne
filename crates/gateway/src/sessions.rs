//! The merged session table and remote control (plan section 4).
//!
//! Two kinds of device appear in `GET /Sessions`: **native** sessions (a client signed in to the gateway with a `/socket` open; commands travel over that socket and no server is involved) and **upstream** sessions (a device signed in to one real server; commands are forwarded to that server with the stored account's token). Upstream sessions with the same `DeviceId` and `Client` (NOCTURNE registers on every server it is signed in to) are collapsed into one gateway session.
//!
//! Gating: the control routes are `Access::Remote` (`gateway_remote_control`); anything forwarded to a server also needs `allow_server_writes` ([`gate`]). A command only drives a player: the `GeneralCommand` names are a fixed allow-list ([`ALLOWED_COMMANDS`]), nothing here can change library data or answer a confirmation, and nothing here ever creates a `Confirmed` / `UserConfirmed` token.
use crate::auth::{AuthCtx, hex, iso, random_hex, status_json, unix_now};
use crate::idmap::{self, Gid, IdMap, Member};
use crate::server::{Access, Routes};
use crate::state::{State, UpKind, UpState, Upstream};
use axum::body::Bytes;
use axum::extract::{Path, Query, State as St};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

/// How long the fan-out to the servers is reused (the plan's 2 s cache).
const UP_TTL: Duration = Duration::from_secs(2);
/// The activity window asked of each server (what the web client uses).
const ACTIVE_WITHIN: u64 = 960;
/// A native session without a socket is forgotten after this long.
const IDLE_FORGET: u64 = 3600;
const UP_BODY_CAP: usize = 2 << 20;

// ---------------------------------------------------------------- gating and allow-list

/// `GeneralCommand` names a remote caller may send. Fixed on purpose: player and navigation only (the Jellyfin names), nothing that reaches a library, an account or a setting.
pub const ALLOWED_COMMANDS: &[&str] = &[
    "MoveUp",
    "MoveDown",
    "MoveLeft",
    "MoveRight",
    "PageUp",
    "PageDown",
    "PreviousLetter",
    "NextLetter",
    "ToggleOsd",
    "ToggleContextMenu",
    "Select",
    "Back",
    "GoHome",
    "GoToSettings",
    "GoToSearch",
    "Guide",
    "ToggleFullscreen",
    "ToggleStats",
    "VolumeUp",
    "VolumeDown",
    "Mute",
    "Unmute",
    "ToggleMute",
    "SetVolume",
    "SetAudioStreamIndex",
    "SetSubtitleStreamIndex",
    "SetRepeatMode",
    "SetShuffleQueue",
    "DisplayContent",
    "DisplayMessage",
    "ChannelUp",
    "ChannelDown",
];

const PLAY_COMMANDS: &[&str] = &[
    "PlayNow",
    "PlayNext",
    "PlayLast",
    "PlayInstantMix",
    "PlayShuffle",
];
const PLAYSTATE_COMMANDS: &[&str] = &[
    "Stop",
    "Pause",
    "Unpause",
    "NextTrack",
    "PreviousTrack",
    "Seek",
    "Rewind",
    "FastForward",
    "PlayPause",
];
const MAX_PLAY_ITEMS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Native,
    Upstream,
}

/// The two-tier gate of plan section 4: remote control on for anything; server changes on as well for a device on a real server (the command is a call to that server, WRITES.md's remote-control row).
pub fn gate(
    kind: Kind,
    remote_control: bool,
    allow_server_writes: bool,
) -> Result<(), (StatusCode, &'static str)> {
    if !remote_control {
        return Err((StatusCode::FORBIDDEN, "Remote control is off"));
    }
    if kind == Kind::Upstream && !allow_server_writes {
        return Err((
            StatusCode::FORBIDDEN,
            "Changing things on servers is off in NOCTURNE, so players on a server cannot be controlled",
        ));
    }
    Ok(())
}

type Fail = (StatusCode, String);

fn bad(msg: &str) -> Fail {
    (StatusCode::BAD_REQUEST, msg.into())
}

// ---------------------------------------------------------------- commands

/// What a caller asked of a session, already checked ([`Cmd::validate`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Play {
        command: String,
        /// Gateway item ids as the client sent them.
        ids: Vec<String>,
        start_ticks: Option<i64>,
        start_index: Option<i64>,
        audio: Option<i64>,
        subtitle: Option<i64>,
    },
    Playstate {
        command: String,
        seek_ticks: Option<i64>,
    },
    General {
        name: String,
        args: BTreeMap<String, String>,
    },
    Message {
        header: String,
        text: String,
        timeout_ms: Option<i64>,
    },
    Viewing {
        item_type: String,
        item_id: String,
        item_name: String,
    },
}

impl Cmd {
    /// Reject what the allow-lists and size caps do not allow. Called before anything is delivered or forwarded.
    pub fn validate(&self) -> Result<(), Fail> {
        match self {
            Cmd::Play { command, ids, .. } => {
                if !PLAY_COMMANDS.contains(&command.as_str()) {
                    return Err(bad("Unknown PlayCommand"));
                }
                if ids.is_empty() || ids.len() > MAX_PLAY_ITEMS {
                    return Err(bad("ItemIds must list between 1 and 200 items"));
                }
            }
            Cmd::Playstate {
                command,
                seek_ticks,
            } => {
                if !PLAYSTATE_COMMANDS.contains(&command.as_str()) {
                    return Err(bad("Unknown playstate command"));
                }
                if command == "Seek" && seek_ticks.is_none_or(|t| t < 0) {
                    return Err(bad("Seek needs SeekPositionTicks"));
                }
            }
            Cmd::General { name, args } => {
                if !ALLOWED_COMMANDS.contains(&name.as_str()) {
                    return Err(bad("That command is not allowed through the gateway"));
                }
                if args.len() > 8
                    || args.iter().any(|(k, v)| {
                        k.len() > 40
                            || !k.bytes().all(|c| c.is_ascii_alphanumeric())
                            || v.chars().count() > 500
                    })
                {
                    return Err(bad("Invalid command arguments"));
                }
            }
            Cmd::Message { header, text, .. } => {
                if text.trim().is_empty()
                    || text.chars().count() > 500
                    || header.chars().count() > 100
                {
                    return Err(bad("Invalid message"));
                }
            }
            Cmd::Viewing {
                item_type,
                item_id,
                item_name,
            } => {
                if item_id.is_empty() || item_type.len() > 40 || item_name.chars().count() > 200 {
                    return Err(bad("Invalid ItemId"));
                }
            }
        }
        Ok(())
    }

    /// Item ids (gateway ids) this command refers to, in order.
    fn item_ids(&self) -> Vec<String> {
        match self {
            Cmd::Play { ids, .. } => ids.clone(),
            Cmd::Viewing { item_id, .. } => vec![item_id.clone()],
            Cmd::General { args, .. } => args.get("ItemId").cloned().into_iter().collect(),
            _ => vec![],
        }
    }
}

fn num(v: Option<&String>) -> Option<i64> {
    v.and_then(|s| s.trim().parse().ok())
}

/// Query keys of Jellyfin are case-insensitive; repeated keys are joined with commas.
fn qmap(q: Vec<(String, String)>) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();
    for (k, v) in q {
        m.entry(k.to_ascii_lowercase())
            .and_modify(|e| {
                e.push(',');
                e.push_str(&v)
            })
            .or_insert(v);
    }
    m
}

fn play_cmd(q: &HashMap<String, String>) -> Cmd {
    Cmd::Play {
        command: q.get("playcommand").cloned().unwrap_or_default(),
        ids: q
            .get("itemids")
            .map(|s| {
                s.split(',')
                    .map(|i| i.trim().to_string())
                    .filter(|i| !i.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        start_ticks: num(q.get("startpositionticks")),
        start_index: num(q.get("startindex")),
        audio: num(q.get("audiostreamindex")),
        subtitle: num(q.get("subtitlestreamindex")),
    }
}

fn args_of(v: &Value) -> Result<BTreeMap<String, String>, Fail> {
    let Some(o) = v.as_object() else {
        return Ok(BTreeMap::new());
    };
    o.iter()
        .map(|(k, v)| match v {
            Value::String(s) => Ok((k.clone(), s.clone())),
            Value::Number(n) => Ok((k.clone(), n.to_string())),
            Value::Bool(b) => Ok((k.clone(), b.to_string())),
            Value::Null => Ok((k.clone(), String::new())),
            _ => Err(bad("Invalid command arguments")),
        })
        .collect()
}

// ---------------------------------------------------------------- what leaves toward a native client or a server

fn message(server_id: &str, kind: &str, data: Value) -> String {
    json!({"MessageType": kind, "MessageId": random_hex::<16>(), "ServerId": server_id, "Data": data}).to_string()
}

fn opt(m: &mut Map<String, Value>, k: &str, v: Option<i64>) {
    if let Some(v) = v {
        m.insert(k.into(), json!(v));
    }
}

/// The socket frame a native session gets for `cmd` (bodies shaped like `PlayRequest`, `PlaystateRequest`, `GeneralCommand`). `ids` are the validated gateway item ids.
pub fn native_message(cmd: &Cmd, ids: &[String], server_id: &str, user_id: &str) -> String {
    match cmd {
        Cmd::Play {
            command,
            start_ticks,
            start_index,
            audio,
            subtitle,
            ..
        } => {
            let mut d = Map::new();
            d.insert("ItemIds".into(), json!(ids));
            d.insert("PlayCommand".into(), json!(command));
            d.insert("ControllingUserId".into(), json!(user_id));
            opt(&mut d, "StartPositionTicks", *start_ticks);
            opt(&mut d, "StartIndex", *start_index);
            opt(&mut d, "AudioStreamIndex", *audio);
            opt(&mut d, "SubtitleStreamIndex", *subtitle);
            message(server_id, "Play", Value::Object(d))
        }
        Cmd::Playstate {
            command,
            seek_ticks,
        } => {
            let mut d = Map::new();
            d.insert("Command".into(), json!(command));
            d.insert("ControllingUserId".into(), json!(user_id));
            opt(&mut d, "SeekPositionTicks", *seek_ticks);
            message(server_id, "Playstate", Value::Object(d))
        }
        Cmd::General { name, args } => {
            let mut args = args.clone();
            if let Some(first) = ids.first()
                && args.contains_key("ItemId")
            {
                args.insert("ItemId".into(), first.clone());
            }
            message(
                server_id,
                "GeneralCommand",
                json!({"Name": name, "ControllingUserId": user_id, "Arguments": args}),
            )
        }
        Cmd::Message {
            header,
            text,
            timeout_ms,
        } => message(
            server_id,
            "GeneralCommand",
            json!({"Name": "DisplayMessage", "ControllingUserId": user_id,
                   "Arguments": {"Header": header, "Text": text, "TimeoutMs": timeout_ms.unwrap_or(5000).to_string()}}),
        ),
        Cmd::Viewing {
            item_type,
            item_name,
            ..
        } => message(
            server_id,
            "GeneralCommand",
            json!({"Name": "DisplayContent", "ControllingUserId": user_id,
                   "Arguments": {"ItemId": ids.first().cloned().unwrap_or_default(), "ItemType": item_type, "ItemName": item_name}}),
        ),
    }
}

/// The call that carries a command to the server that owns a session.
#[derive(Clone, Debug, PartialEq)]
pub struct UpCall {
    /// Path below the server's base address, starting with `/Sessions/`.
    pub path: String,
    pub query: Vec<(&'static str, String)>,
    pub body: Option<Value>,
}

/// `real_ids` are that server's own ids for the items of the command, in the order of [`Cmd::item_ids`]; `user_id` is the server account the gateway acts as.
pub fn up_call(cmd: &Cmd, real_session: &str, real_ids: &[String], user_id: &str) -> UpCall {
    let base = format!("/Sessions/{real_session}");
    let mut query: Vec<(&'static str, String)> = vec![];
    let mut push = |k: &'static str, v: Option<i64>| {
        if let Some(v) = v {
            query.push((k, v.to_string()));
        }
    };
    match cmd {
        Cmd::Play {
            command,
            start_ticks,
            start_index,
            audio,
            subtitle,
            ..
        } => {
            push("StartPositionTicks", *start_ticks);
            push("StartIndex", *start_index);
            push("AudioStreamIndex", *audio);
            push("SubtitleStreamIndex", *subtitle);
            // A gateway media-source id means nothing to the server, so none is passed on.
            query.push(("PlayCommand", command.clone()));
            query.push(("ItemIds", real_ids.join(",")));
            query.push(("ControllingUserId", user_id.into()));
            UpCall {
                path: format!("{base}/Playing"),
                query,
                body: None,
            }
        }
        Cmd::Playstate {
            command,
            seek_ticks,
        } => {
            push("SeekPositionTicks", *seek_ticks);
            query.push(("ControllingUserId", user_id.into()));
            UpCall {
                path: format!("{base}/Playing/{command}"),
                query,
                body: None,
            }
        }
        Cmd::General { name, args } => {
            let mut args = args.clone();
            if let Some(first) = real_ids.first()
                && args.contains_key("ItemId")
            {
                args.insert("ItemId".into(), first.clone());
            }
            UpCall {
                path: format!("{base}/Command"),
                query,
                body: Some(json!({"Name": name, "ControllingUserId": user_id, "Arguments": args})),
            }
        }
        Cmd::Message {
            header,
            text,
            timeout_ms,
        } => UpCall {
            path: format!("{base}/Message"),
            query,
            body: Some(
                json!({"Header": header, "Text": text, "TimeoutMs": timeout_ms.unwrap_or(5000)}),
            ),
        },
        Cmd::Viewing {
            item_type,
            item_name,
            ..
        } => {
            query.push(("ItemType", item_type.clone()));
            query.push(("ItemId", real_ids.first().cloned().unwrap_or_default()));
            query.push(("ItemName", item_name.clone()));
            UpCall {
                path: format!("{base}/Viewing"),
                query,
                body: None,
            }
        }
    }
}

// ---------------------------------------------------------------- the session table

/// What a native client told us it can do (`POST /Sessions/Capabilities[/Full]`), reduced to what the table shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caps {
    pub playable: Vec<String>,
    pub commands: Vec<String>,
    pub media_control: bool,
}

fn short_list<'a>(it: impl Iterator<Item = &'a str>, max: usize) -> Vec<String> {
    it.map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 40 && s.bytes().all(|c| c.is_ascii_alphanumeric()))
        .take(max)
        .map(str::to_string)
        .collect()
}

impl Caps {
    /// From the JSON body of `/Capabilities/Full`; anything odd is dropped.
    pub fn from_json(v: &Value) -> Self {
        let list = |k: &str, max| -> Vec<String> {
            v[k].as_array()
                .map(|a| short_list(a.iter().filter_map(Value::as_str), max))
                .unwrap_or_default()
        };
        Self {
            playable: list("PlayableMediaTypes", 16),
            commands: list("SupportedCommands", 64),
            media_control: v["SupportsMediaControl"].as_bool().unwrap_or(false),
        }
    }

    /// From the query of `/Capabilities` (comma separated lists).
    pub fn from_query(q: &HashMap<String, String>) -> Self {
        let list = |k: &str, max| -> Vec<String> {
            q.get(k)
                .map(|s| short_list(s.split(','), max))
                .unwrap_or_default()
        };
        Self {
            playable: list("playablemediatypes", 16),
            commands: list("supportedcommands", 64),
            media_control: q
                .get("supportsmediacontrol")
                .is_some_and(|s| s.eq_ignore_ascii_case("true")),
        }
    }
}

#[derive(Clone)]
struct Conn {
    id: u64,
    tx: mpsc::Sender<String>,
}

/// One gateway token's session.
#[derive(Clone)]
pub struct Native {
    pub token_id: String,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub version: String,
    pub caps: Caps,
    /// `PlayState` and `NowPlayingItem` as the client last reported them (already gateway-shaped; see [`Hub::set_playing`]).
    pub play_state: Option<Value>,
    pub playing: Option<Value>,
    pub last_activity: u64,
    conns: Vec<Conn>,
}

impl Native {
    fn new(c: &AuthCtx) -> Self {
        Self {
            token_id: c.token_id.clone(),
            client: c.client.clone(),
            device_name: c.device_name.clone(),
            device_id: c.device_id.clone(),
            version: c.version.clone(),
            caps: Caps::default(),
            play_state: None,
            playing: None,
            last_activity: unix_now(),
            conns: vec![],
        }
    }

    pub fn online(&self) -> bool {
        !self.conns.is_empty()
    }
}

/// The id of a native session: the one the sign-in result already carried (`auth::session_stub`).
pub fn native_session_id(token_id: &str) -> String {
    hex(&Sha256::digest(format!("session:{token_id}").as_bytes()))[..32].to_string()
}

/// The id of a collapsed upstream session: stable across polls and server restarts, derived from the device.
pub fn group_session_id(device_id: &str, client: &str) -> String {
    Gid::of(&idmap::key_session("dev", &format!("{device_id}:{client}"))).to_string()
}

/// Events the socket layer turns into pushes.
#[derive(Clone, Debug)]
pub enum Event {
    Library,
    UserData(Vec<String>),
}

/// One session of a real server as it came back from `GET /Sessions`.
#[derive(Clone, Debug)]
pub struct UpSession {
    pub source_key: String,
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpMember {
    pub source: String,
    /// The server's own session id.
    pub session: String,
    pub playing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Native(String),
    Upstream(Vec<UpMember>),
}

/// Who the table is shown to and what is switched on.
pub struct MergeCtx<'a> {
    pub ids: &'a IdMap,
    pub user_id: &'a str,
    pub user_name: &'a str,
    pub server_id: &'a str,
    /// `allow_server_writes`: without it sessions of real servers are listed but not controllable.
    pub writes: bool,
}

pub struct Hub {
    natives: Mutex<HashMap<String, Native>>,
    next_conn: AtomicU64,
    events: broadcast::Sender<Event>,
    up: tokio::sync::Mutex<Option<(Instant, std::sync::Arc<Vec<UpSession>>)>>,
    http: OnceLock<reqwest::Client>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            natives: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            events: broadcast::channel(64).0,
            up: tokio::sync::Mutex::new(None),
            http: OnceLock::new(),
        }
    }
}

impl Hub {
    /// A socket opened for this token. The receiver yields the frames to write; call [`Hub::detach`] when the socket ends.
    pub fn attach(&self, c: &AuthCtx) -> (u64, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(64);
        let id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let mut m = self.natives.lock().unwrap();
        let n = m
            .entry(c.token_id.clone())
            .or_insert_with(|| Native::new(c));
        n.client = c.client.clone();
        n.device_name = c.device_name.clone();
        n.device_id = c.device_id.clone();
        n.version = c.version.clone();
        n.last_activity = unix_now();
        n.conns.push(Conn { id, tx });
        (id, rx)
    }

    pub fn detach(&self, token_id: &str, conn: u64) {
        if let Some(n) = self.natives.lock().unwrap().get_mut(token_id) {
            n.conns.retain(|c| c.id != conn);
            n.last_activity = unix_now();
        }
    }

    /// Mark the token active (a socket frame or a command arrived).
    pub fn touch(&self, token_id: &str) {
        if let Some(n) = self.natives.lock().unwrap().get_mut(token_id) {
            n.last_activity = unix_now();
        }
    }

    pub fn set_caps(&self, c: &AuthCtx, caps: Caps) {
        let mut m = self.natives.lock().unwrap();
        let n = m
            .entry(c.token_id.clone())
            .or_insert_with(|| Native::new(c));
        n.caps = caps;
        n.last_activity = unix_now();
    }

    /// For the playback-reporting routes: what the client is playing now (`PlayState` and `NowPlayingItem` with gateway ids, no `Path`), or `None, None` when it stopped.
    pub fn set_playing(&self, c: &AuthCtx, play_state: Option<Value>, now_playing: Option<Value>) {
        let mut m = self.natives.lock().unwrap();
        let n = m
            .entry(c.token_id.clone())
            .or_insert_with(|| Native::new(c));
        n.play_state = play_state;
        n.playing = now_playing;
        n.last_activity = unix_now();
    }

    /// Send a frame to every open socket of the token; returns how many took it.
    pub fn deliver(&self, token_id: &str, frame: &str) -> usize {
        let m = self.natives.lock().unwrap();
        m.get(token_id).map_or(0, |n| {
            n.conns
                .iter()
                .filter(|c| c.tx.try_send(frame.to_string()).is_ok())
                .count()
        })
    }

    /// Native sessions that can be shown: those with a socket, plus the caller's own (a client reads its own session even before it opens one). Forgets long-idle ones.
    pub fn snapshot(&self, own: Option<&AuthCtx>) -> Vec<Native> {
        let now = unix_now();
        let mut m = self.natives.lock().unwrap();
        m.retain(|_, n| n.online() || now.saturating_sub(n.last_activity) < IDLE_FORGET);
        let mut v: Vec<Native> = m
            .values()
            .filter(|n| n.online() || own.is_some_and(|o| o.token_id == n.token_id))
            .cloned()
            .collect();
        if let Some(o) = own
            && !v.iter().any(|n| n.token_id == o.token_id)
        {
            v.push(Native::new(o));
        }
        v.sort_by(|a, b| a.token_id.cmp(&b.token_id));
        v
    }

    fn find_native(&self, session_id: &str) -> Option<Native> {
        self.natives
            .lock()
            .unwrap()
            .values()
            .find(|n| native_session_id(&n.token_id) == session_id)
            .cloned()
    }

    pub fn library_changed(&self) {
        let _ = self.events.send(Event::Library);
    }

    pub fn user_data_changed(&self, item_ids: Vec<String>) {
        let _ = self.events.send(Event::UserData(item_ids));
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    fn http(&self) -> &reqwest::Client {
        self.http.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("http client")
        })
    }

    /// The servers' sessions, fetched concurrently and reused for 2 s. `fresh` skips the reuse.
    async fn up_sessions(&self, st: &State, fresh: bool) -> std::sync::Arc<Vec<UpSession>> {
        let mut g = self.up.lock().await;
        if !fresh
            && let Some((t, v)) = &*g
            && t.elapsed() < UP_TTL
        {
            return v.clone();
        }
        let mut set = tokio::task::JoinSet::new();
        for u in st.upstreams().iter() {
            if u.state != UpState::Ready || !matches!(u.kind, UpKind::Server { .. }) {
                continue;
            }
            let (u, http) = (u.clone(), self.http().clone());
            set.spawn(async move {
                let r = fetch_sessions(&http, &u).await;
                (u, r)
            });
        }
        let mut out = vec![];
        while let Some(Ok((u, r))) = set.join_next().await {
            match r {
                Ok(list) => {
                    let UpKind::Server { source_key, .. } = &u.kind else {
                        continue;
                    };
                    out.extend(list.into_iter().map(|raw| UpSession {
                        source_key: source_key.clone(),
                        raw,
                    }));
                }
                Err(401) => st.mark_signed_out(u.id),
                Err(_) => {}
            }
        }
        let v = std::sync::Arc::new(out);
        *g = Some((Instant::now(), v.clone()));
        v
    }
}

fn auth_header(token: &str) -> Option<header::HeaderValue> {
    let mut v = header::HeaderValue::from_str(&format!(
        "MediaBrowser Client=\"NOCTURNE Gateway\", Device=\"Gateway\", DeviceId=\"nocturne-gateway\", Version=\"{}\", Token=\"{token}\"",
        env!("CARGO_PKG_VERSION")
    ))
    .ok()?;
    v.set_sensitive(true);
    Some(v)
}

fn server_parts(u: &Upstream) -> Option<(&str, &str, &str)> {
    match &u.kind {
        UpKind::Server {
            url,
            user_id,
            token: Some(t),
            ..
        } => Some((url.as_str(), user_id.as_str(), t.expose())),
        _ => None,
    }
}

/// `Err(status)` carries the HTTP status (0: no answer).
async fn fetch_sessions(http: &reqwest::Client, u: &Upstream) -> Result<Vec<Value>, u16> {
    let (url, user, token) = server_parts(u).ok_or(0u16)?;
    let resp = http
        .get(format!("{}/Sessions", url.trim_end_matches('/')))
        .query(&[
            ("ControllableByUserId", user.to_string()),
            ("ActiveWithinSeconds", ACTIVE_WITHIN.to_string()),
        ])
        .header(header::AUTHORIZATION, auth_header(token).ok_or(0u16)?)
        .send()
        .await
        .map_err(|_| 0u16)?;
    if !resp.status().is_success() {
        return Err(resp.status().as_u16());
    }
    let body = resp.bytes().await.map_err(|_| 0u16)?;
    if body.len() > UP_BODY_CAP {
        return Err(0);
    }
    serde_json::from_slice::<Vec<Value>>(&body).map_err(|_| 0)
}

// ---------------------------------------------------------------- merging

/// Keys that never reach a client: server paths, URLs that can carry a token, the server's idea of who is on the other end.
const STRIP: &[&str] = &[
    "Path",
    "TranscodingUrl",
    "DeliveryUrl",
    "RemoteEndPoint",
    "MediaSources",
    "MediaSourceId",
    "UserPrimaryImageTag",
];
const ID_KEYS: &[&str] = &[
    "Id", "SeriesId", "SeasonId", "AlbumId", "ParentId", "ItemId",
];

fn strip_deep(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for k in STRIP {
                o.remove(*k);
            }
            o.values_mut().for_each(strip_deep);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_deep),
        _ => {}
    }
}

/// An item shown in a session (`NowPlayingItem`, queue entries): ids become gateway ids, server paths and URLs go.
fn rewrite_item(ids: &IdMap, source_key: &str, v: &mut Value) {
    strip_deep(v);
    if let Value::Object(o) = v {
        for k in ID_KEYS {
            if let Some(Value::String(s)) = o.get_mut(*k)
                && !s.is_empty()
            {
                *s = ids.register_single(source_key, s).to_string();
            }
        }
    }
}

fn rewrite_upstream(
    c: &MergeCtx,
    source_key: &str,
    raw: &Value,
    id: &str,
    controllable: bool,
) -> Value {
    let mut v = raw.clone();
    strip_deep(&mut v);
    let Some(o) = v.as_object_mut() else {
        return v;
    };
    o.insert("Id".into(), json!(id));
    o.insert("UserId".into(), json!(c.user_id));
    o.insert("UserName".into(), json!(c.user_name));
    o.insert("ServerId".into(), json!(c.server_id));
    o.insert("RemoteEndPoint".into(), json!(""));
    o.insert("AdditionalUsers".into(), json!([]));
    o.insert("SupportsRemoteControl".into(), json!(controllable));
    for k in ["NowPlayingItem", "NowViewingItem"] {
        if let Some(it) = o.get_mut(k) {
            rewrite_item(c.ids, source_key, it);
        }
    }
    if let Some(Value::Array(q)) = o.get_mut("NowPlayingQueue") {
        q.iter_mut()
            .for_each(|e| rewrite_item(c.ids, source_key, e));
    }
    if let Some(Value::Array(q)) = o.get_mut("NowPlayingQueueFullItems") {
        q.iter_mut()
            .for_each(|e| rewrite_item(c.ids, source_key, e));
    }
    v
}

/// A native session in the shape of `SessionInfoDto`.
pub fn native_json(c: &MergeCtx, n: &Native) -> Value {
    let mut v = json!({
        "PlayState": {"CanSeek": false, "IsPaused": false, "IsMuted": false, "RepeatMode": "RepeatNone", "PlaybackOrder": "Default"},
        "AdditionalUsers": [],
        "Capabilities": {"PlayableMediaTypes": n.caps.playable, "SupportedCommands": n.caps.commands,
                         "SupportsMediaControl": n.caps.media_control, "SupportsPersistentIdentifier": true},
        "RemoteEndPoint": "",
        "PlayableMediaTypes": n.caps.playable,
        "Id": native_session_id(&n.token_id),
        "UserId": c.user_id,
        "UserName": c.user_name,
        "Client": n.client,
        "LastActivityDate": iso(n.last_activity),
        "LastPlaybackCheckIn": "0001-01-01T00:00:00.0000000Z",
        "DeviceName": n.device_name,
        "DeviceId": n.device_id,
        "ApplicationVersion": n.version,
        "IsActive": true,
        "SupportsMediaControl": n.caps.media_control,
        "SupportsRemoteControl": n.caps.media_control && n.online(),
        "NowPlayingQueue": [],
        "NowPlayingQueueFullItems": [],
        "HasCustomDeviceName": false,
        "ServerId": c.server_id,
        "SupportedCommands": n.caps.commands,
    });
    if let Some(p) = &n.play_state {
        v["PlayState"] = p.clone();
    }
    if let Some(p) = &n.playing {
        v["NowPlayingItem"] = p.clone();
    }
    v
}

/// Merge the native sessions and the servers' sessions into the table clients see, plus the routing for each gateway session id.
///
/// * Natives with the same device and client collapse to the most recently active.
/// * Upstream sessions with the same `DeviceId` and `Client` collapse into one gateway session carrying each server's own session id; one that a native session already covers is dropped (the socket path needs no server).
/// * Upstream sessions that cannot be remote controlled are not listed; with `writes` off the others are listed read-only.
pub fn merge(
    c: &MergeCtx,
    natives: &[Native],
    ups: &[UpSession],
) -> (Vec<Value>, HashMap<String, Target>) {
    let mut out = vec![];
    let mut routes = HashMap::new();
    let mut latest: BTreeMap<(String, String), &Native> = BTreeMap::new();
    for n in natives {
        let e = latest
            .entry((n.device_id.clone(), n.client.clone()))
            .or_insert(n);
        if n.last_activity > e.last_activity {
            *e = n;
        }
    }
    for n in latest.values() {
        routes.insert(
            native_session_id(&n.token_id),
            Target::Native(n.token_id.clone()),
        );
        out.push(native_json(c, n));
    }
    let mut groups: BTreeMap<(String, String), Vec<&UpSession>> = BTreeMap::new();
    for u in ups {
        if u.raw["SupportsRemoteControl"].as_bool() == Some(false) {
            continue;
        }
        let (Some(dev), Some(client), Some(_)) = (
            u.raw["DeviceId"].as_str(),
            u.raw["Client"].as_str(),
            u.raw["Id"].as_str(),
        ) else {
            continue;
        };
        if latest.contains_key(&(dev.to_string(), client.to_string())) {
            continue;
        }
        groups
            .entry((dev.to_string(), client.to_string()))
            .or_default()
            .push(u);
    }
    for ((dev, client), members) in groups {
        // The member that shows the group: the one playing, else the most recently active.
        let lead = members
            .iter()
            .max_by_key(|m| {
                (
                    m.raw.get("NowPlayingItem").is_some_and(|v| !v.is_null()),
                    m.raw["LastActivityDate"].as_str().unwrap_or("").to_string(),
                )
            })
            .copied()
            .expect("group has a member");
        let id = group_session_id(&dev, &client);
        out.push(rewrite_upstream(
            c,
            &lead.source_key,
            &lead.raw,
            &id,
            c.writes,
        ));
        routes.insert(
            id,
            Target::Upstream(
                members
                    .iter()
                    .map(|m| UpMember {
                        source: m.source_key.clone(),
                        session: m.raw["Id"].as_str().unwrap_or_default().to_string(),
                        playing: m.raw.get("NowPlayingItem").is_some_and(|v| !v.is_null()),
                    })
                    .collect(),
            ),
        );
    }
    (out, routes)
}

fn merge_ctx<'a>(
    st: &'a State,
    user_id: &'a str,
    user_name: &'a str,
    server_id: &'a str,
) -> MergeCtx<'a> {
    MergeCtx {
        ids: &st.ids,
        user_id,
        user_name,
        server_id,
        writes: st.cfg().allow_server_writes,
    }
}

/// The table for one caller: with remote control off only the caller's own session, nothing is asked of any server.
pub async fn list(st: &State, caller: &AuthCtx) -> Vec<Value> {
    let hub = st.ext::<Hub>();
    let (uid, name, sid) = (st.users.user_id(), st.users.name(), st.users.server_id());
    let c = merge_ctx(st, &uid, &name, &sid);
    if !st.cfg().gw.remote_control {
        let own: Vec<Native> = hub
            .snapshot(Some(caller))
            .into_iter()
            .filter(|n| n.token_id == caller.token_id)
            .collect();
        return merge(&c, &own, &[]).0;
    }
    let natives = hub.snapshot(Some(caller));
    let ups = hub.up_sessions(st, false).await;
    merge(&c, &natives, &ups).0
}

/// Where a gateway session id leads. A stale answer is refreshed once.
async fn find_target(st: &State, session_id: &str) -> Option<Target> {
    let hub = st.ext::<Hub>();
    if let Some(n) = hub.find_native(session_id) {
        return n.online().then_some(Target::Native(n.token_id));
    }
    let (uid, name, sid) = (st.users.user_id(), st.users.name(), st.users.server_id());
    for fresh in [false, true] {
        let ups = hub.up_sessions(st, fresh).await;
        let c = merge_ctx(st, &uid, &name, &sid);
        if let Some(t) = merge(&c, &[], &ups).1.remove(session_id) {
            return Some(t);
        }
    }
    None
}

// ---------------------------------------------------------------- routing a command to an upstream member

/// The member of an upstream session that must receive a command and the real ids of its items: the first member whose server holds every item; for a command without items, the member that is playing, else the first.
pub fn pick_member<'a>(
    group: &'a [UpMember],
    items: &[Vec<Member>],
    transport: bool,
) -> Result<(&'a UpMember, Vec<String>), Fail> {
    let holds = |m: &UpMember| -> Option<Vec<String>> {
        items
            .iter()
            .map(|ms| {
                ms.iter()
                    .find(|x| x.source == m.source)
                    .map(|x| x.id.clone())
            })
            .collect()
    };
    let order: Vec<&UpMember> = if transport {
        group
            .iter()
            .filter(|m| m.playing)
            .chain(group.iter().filter(|m| !m.playing))
            .collect()
    } else {
        group.iter().collect()
    };
    order
        .into_iter()
        .find_map(|m| holds(m).map(|ids| (m, ids)))
        .ok_or_else(|| {
            bad("That device is signed in to a server that does not have these items; pick a device on the item's own server or play it on a device of this gateway")
        })
}

fn safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

async fn forward(st: &State, hub: &Hub, source: &str, call: &UpCall) -> Result<(), Fail> {
    let ups = st.upstreams();
    let Some(u) = ups
        .iter()
        .find(|u| matches!(&u.kind, UpKind::Server { source_key, .. } if source_key == source))
    else {
        return Err((
            StatusCode::BAD_GATEWAY,
            "That server is not available".into(),
        ));
    };
    let Some((url, _, token)) = server_parts(u) else {
        return Err((StatusCode::BAD_GATEWAY, "That server is signed out".into()));
    };
    let mut req = hub
        .http()
        .post(format!("{}{}", url.trim_end_matches('/'), call.path))
        .query(&call.query)
        .header(
            header::AUTHORIZATION,
            auth_header(token).ok_or_else(|| bad("Invalid token"))?,
        );
    if let Some(b) = &call.body {
        req = req.json(b);
    }
    let resp = req
        .send()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "That server did not answer".into()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => {
            st.mark_signed_out(u.id);
            Err((
                StatusCode::BAD_GATEWAY,
                "That server rejected the gateway's sign-in".into(),
            ))
        }
        404 => Err((StatusCode::NOT_FOUND, "That session is gone".into())),
        _ => Err((
            StatusCode::BAD_GATEWAY,
            "That server refused the command".into(),
        )),
    }
}

/// Run one command against a session. Checks, in order: rate limit, command validity, session, gating, item ownership.
pub async fn dispatch(st: &State, ctx: &AuthCtx, session_id: &str, cmd: Cmd) -> Result<(), Fail> {
    if !st.limits.command.allow(&ctx.token_id, Instant::now()) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "Too many commands".into()));
    }
    cmd.validate()?;
    let hub = st.ext::<Hub>();
    hub.touch(&ctx.token_id);
    let cfg = st.cfg();
    let Some(target) = find_target(st, session_id).await else {
        return Err((StatusCode::NOT_FOUND, "No such session".into()));
    };
    let kind = match target {
        Target::Native(_) => Kind::Native,
        Target::Upstream(_) => Kind::Upstream,
    };
    gate(kind, cfg.gw.remote_control, cfg.allow_server_writes)
        .map_err(|(c, m)| (c, m.to_string()))?;
    // Every item must be an id this gateway handed out.
    let items: Vec<Vec<Member>> = cmd
        .item_ids()
        .iter()
        .map(|i| {
            st.ids
                .resolve_str(i)
                .map(|(_, m)| m)
                .ok_or_else(|| (StatusCode::NOT_FOUND, "Unknown item".to_string()))
        })
        .collect::<Result<_, _>>()?;
    let (uid, sid) = (st.users.user_id(), st.users.server_id());
    match target {
        Target::Native(token_id) => {
            let gids: Vec<String> = cmd
                .item_ids()
                .iter()
                .filter_map(|i| Gid::parse(i).map(|g| g.to_string()))
                .collect();
            let frame = native_message(&cmd, &gids, &sid, &uid);
            if hub.deliver(&token_id, &frame) == 0 {
                return Err((StatusCode::NOT_FOUND, "That session is offline".into()));
            }
            Ok(())
        }
        Target::Upstream(group) => {
            let transport = items.is_empty();
            let (m, real_ids) = pick_member(&group, &items, transport)?;
            if !safe_segment(&m.session) {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "That server sent an unusable session id".into(),
                ));
            }
            let server_user = st
                .upstreams()
                .iter()
                .find_map(|u| match &u.kind {
                    UpKind::Server {
                        source_key,
                        user_id,
                        ..
                    } if *source_key == m.source => Some(user_id.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let call = up_call(&cmd, &m.session, &real_ids, &server_user);
            forward(st, &hub, &m.source, &call).await
        }
    }
}

// ---------------------------------------------------------------- routes

fn done(r: Result<(), Fail>) -> Response {
    match r {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((c, m)) => status_json(c, &m),
    }
}

async fn get_sessions(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let q = qmap(q);
    let mut v = list(&st, &ctx).await;
    if let Some(d) = q.get("deviceid") {
        v.retain(|s| s["DeviceId"].as_str() == Some(d.as_str()));
    }
    Json(v).into_response()
}

fn store_caps(st: &State, ctx: &AuthCtx, caps: Caps) -> Response {
    // Kept even while remote control is off: the table and the control routes are what the switch gates, and a client then keeps working when the switch is turned on later.
    st.ext::<Hub>().set_caps(ctx, caps);
    StatusCode::NO_CONTENT.into_response()
}

async fn caps_query(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    store_caps(&st, &ctx, Caps::from_query(&qmap(q)))
}

async fn caps_full(St(st): St<State>, Extension(ctx): Extension<AuthCtx>, body: Bytes) -> Response {
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    store_caps(&st, &ctx, Caps::from_json(&v))
}

async fn play(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Path(sid): Path<String>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    done(dispatch(&st, &ctx, &sid, play_cmd(&qmap(q))).await)
}

async fn playstate(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Path((sid, command)): Path<(String, String)>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let q = qmap(q);
    let cmd = Cmd::Playstate {
        command,
        seek_ticks: num(q.get("seekpositionticks")),
    };
    done(dispatch(&st, &ctx, &sid, cmd).await)
}

async fn command_named(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Path((sid, name)): Path<(String, String)>,
) -> Response {
    done(
        dispatch(
            &st,
            &ctx,
            &sid,
            Cmd::General {
                name,
                args: BTreeMap::new(),
            },
        )
        .await,
    )
}

async fn command_body(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Path(sid): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(v) = serde_json::from_slice::<Value>(&body) else {
        return status_json(StatusCode::BAD_REQUEST, "Invalid request");
    };
    let args = match args_of(&v["Arguments"]) {
        Ok(a) => a,
        Err((c, m)) => return status_json(c, &m),
    };
    let name = v["Name"].as_str().unwrap_or_default().to_string();
    done(dispatch(&st, &ctx, &sid, Cmd::General { name, args }).await)
}

async fn message_body(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Path(sid): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(v) = serde_json::from_slice::<Value>(&body) else {
        return status_json(StatusCode::BAD_REQUEST, "Invalid request");
    };
    let cmd = Cmd::Message {
        header: v["Header"].as_str().unwrap_or_default().to_string(),
        text: v["Text"].as_str().unwrap_or_default().to_string(),
        timeout_ms: v["TimeoutMs"].as_i64(),
    };
    done(dispatch(&st, &ctx, &sid, cmd).await)
}

async fn viewing(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    Path(sid): Path<String>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let q = qmap(q);
    let g = |k: &str| q.get(k).cloned().unwrap_or_default();
    let cmd = Cmd::Viewing {
        item_type: g("itemtype"),
        item_id: g("itemid"),
        item_name: g("itemname"),
    };
    done(dispatch(&st, &ctx, &sid, cmd).await)
}

pub fn routes(r: Routes) -> Routes {
    // The list and the capability posts are open to every signed-in client (the official apps call them on start-up); with remote control off the list holds only the caller's own session and the capabilities change nothing a caller can use. Everything that drives a player is Remote.
    r.add("/Sessions", Access::Auth, get(get_sessions))
        .add("/Sessions/Capabilities", Access::Auth, post(caps_query))
        .add("/Sessions/Capabilities/Full", Access::Auth, post(caps_full))
        .add("/Sessions/{sessionId}/Playing", Access::Remote, post(play))
        .add(
            "/Sessions/{sessionId}/Playing/{command}",
            Access::Remote,
            post(playstate),
        )
        .add(
            "/Sessions/{sessionId}/Command",
            Access::Remote,
            post(command_body),
        )
        .add(
            "/Sessions/{sessionId}/Command/{command}",
            Access::Remote,
            post(command_named),
        )
        .add(
            "/Sessions/{sessionId}/Message",
            Access::Remote,
            post(message_body),
        )
        .add(
            "/Sessions/{sessionId}/Viewing",
            Access::Remote,
            post(viewing),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(source: &str, id: &str, dev: &str, client: &str, playing: Option<&str>) -> UpSession {
        let mut raw = json!({
            "Id": id, "DeviceId": dev, "Client": client, "DeviceName": "Living room",
            "UserId": "serveruser", "UserName": "realname", "ServerId": "serverid",
            "RemoteEndPoint": "192.168.1.77", "SupportsRemoteControl": true,
            "UserPrimaryImageTag": "tag", "LastActivityDate": "2026-11-27T10:00:00.0000000Z",
            "PlayState": {"IsPaused": false, "MediaSourceId": "ms1"},
            "AdditionalUsers": [{"UserId": "other"}],
        });
        if let Some(item) = playing {
            raw["NowPlayingItem"] = json!({
                "Id": item, "Name": "A film", "Path": "/media/private/film.mkv", "SeriesId": "",
                "MediaSources": [{"Id": "x", "Path": "/media/private/film.mkv", "TranscodingUrl": "/videos/1/master.m3u8?api_key=SECRETTOKEN"}],
                "MediaStreams": [{"DeliveryUrl": "/Videos/1/Subtitles/2/0/Stream.vtt?api_key=SECRETTOKEN", "Index": 2}],
            });
            raw["NowPlayingQueue"] = json!([{"Id": item, "PlaylistItemId": "p1"}]);
            raw["NowPlayingQueueFullItems"] =
                json!([{"Id": item, "Path": "/media/private/film.mkv"}]);
        }
        UpSession {
            source_key: source.into(),
            raw,
        }
    }

    fn native(token: &str, dev: &str, client: &str, at: u64, sockets: bool) -> Native {
        let mut n = Native::new(&AuthCtx {
            token_id: token.into(),
            client: client.into(),
            device_name: "Phone".into(),
            device_id: dev.into(),
            version: "1".into(),
        });
        n.last_activity = at;
        n.caps.media_control = true;
        if sockets {
            n.conns.push(Conn {
                id: 1,
                tx: mpsc::channel(1).0,
            });
        }
        n
    }

    fn ctx(ids: &IdMap, writes: bool) -> MergeCtx<'_> {
        MergeCtx {
            ids,
            user_id: "gwuser",
            user_name: "nocturne",
            server_id: "gwserver",
            writes,
        }
    }

    #[test]
    fn upstream_sessions_collapse_by_device_and_client_and_are_rewritten() {
        let ids = IdMap::in_memory();
        let ups = [
            up("srvA", "sessA", "dev1", "NOCTURNE", None),
            up("srvB", "sessB", "dev1", "NOCTURNE", Some("itemB")),
            up("srvA", "sessC", "dev2", "Phone app", None),
        ];
        let (list, routes) = merge(&ctx(&ids, true), &[], &ups);
        assert_eq!(list.len(), 2, "same DeviceId and Client is one session");
        let id = group_session_id("dev1", "NOCTURNE");
        let one = list.iter().find(|s| s["Id"] == id.as_str()).unwrap();
        assert_eq!(one["UserId"], "gwuser");
        assert_eq!(one["UserName"], "nocturne");
        assert_eq!(one["ServerId"], "gwserver");
        assert_eq!(one["RemoteEndPoint"], "");
        assert_eq!(one["AdditionalUsers"], json!([]));
        assert_eq!(one["SupportsRemoteControl"], true);
        // The lead is the member that is playing; its item id is a gateway id the map can resolve.
        let item = one["NowPlayingItem"]["Id"].as_str().unwrap();
        assert_eq!(item, ids.register_single("srvB", "itemB").to_string());
        assert_eq!(one["NowPlayingQueue"][0]["Id"], item);
        assert_eq!(one["NowPlayingQueueFullItems"][0]["Id"], item);
        let text = one.to_string();
        for leak in [
            "SECRETTOKEN",
            "/media/private",
            "\"Path\"",
            "TranscodingUrl",
            "DeliveryUrl",
            "192.168",
            "realname",
            "serverid",
            "UserPrimaryImageTag",
            "MediaSourceId",
        ] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
        // Both members are reachable; the playing one is marked.
        let Target::Upstream(m) = &routes[&id] else {
            panic!()
        };
        assert_eq!(m.len(), 2);
        assert!(
            m.iter()
                .any(|x| x.source == "srvB" && x.session == "sessB" && x.playing)
        );
        assert!(
            m.iter()
                .any(|x| x.source == "srvA" && x.session == "sessA" && !x.playing)
        );
    }

    #[test]
    fn writes_off_lists_upstream_read_only_and_uncontrollable_ones_are_hidden() {
        let ids = IdMap::in_memory();
        let mut off = up("srvA", "s2", "dev9", "Cast", None);
        off.raw["SupportsRemoteControl"] = json!(false);
        let (list, _) = merge(
            &ctx(&ids, false),
            &[],
            &[up("srvA", "s1", "dev1", "NOCTURNE", None), off],
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["SupportsRemoteControl"], false);
    }

    #[test]
    fn natives_collapse_and_cover_the_same_device_upstream() {
        let ids = IdMap::in_memory();
        let ns = [
            native("tokA1", "devA", "Official", 100, true),
            native("tokA2", "devA", "Official", 200, true),
            native("tokB", "devB", "Other", 50, true),
        ];
        let (list, routes) = merge(
            &ctx(&ids, true),
            &ns,
            &[up("srvA", "s1", "devA", "Official", None)],
        );
        assert_eq!(
            list.len(),
            2,
            "two tokens of one device show once; the upstream copy of it is dropped"
        );
        assert_eq!(
            routes[&native_session_id("tokA2")],
            Target::Native("tokA2".into())
        );
        assert!(!routes.contains_key(&native_session_id("tokA1")));
        let a = list
            .iter()
            .find(|s| s["Id"] == native_session_id("tokA2").as_str())
            .unwrap();
        assert_eq!(a["SupportsRemoteControl"], true);
        assert_eq!(a["UserId"], "gwuser");
        assert_eq!(a["Client"], "Official");
    }

    #[test]
    fn session_ids_are_stable() {
        assert_eq!(group_session_id("d", "c"), group_session_id("d", "c"));
        assert_ne!(group_session_id("d", "c"), group_session_id("d", "c2"));
        assert_eq!(native_session_id("abc").len(), 32);
    }

    #[test]
    fn gating_table() {
        use Kind::*;
        let ok = |k, r, w| gate(k, r, w).is_ok();
        // remote control off: nothing is controllable
        for (k, w) in [
            (Native, false),
            (Native, true),
            (Upstream, false),
            (Upstream, true),
        ] {
            assert!(!ok(k, false, w));
            assert_eq!(gate(k, false, w).unwrap_err().0, StatusCode::FORBIDDEN);
        }
        // on: natives never need server changes; a real server's player does
        assert!(ok(Native, true, false) && ok(Native, true, true));
        assert!(!ok(Upstream, true, false) && ok(Upstream, true, true));
    }

    #[test]
    fn general_command_allow_list() {
        let cmd = |n: &str| Cmd::General {
            name: n.into(),
            args: BTreeMap::new(),
        };
        for ok in [
            "MoveUp",
            "Select",
            "SetVolume",
            "ToggleMute",
            "DisplayMessage",
            "SetAudioStreamIndex",
            "GoHome",
        ] {
            assert!(cmd(ok).validate().is_ok(), "{ok}");
        }
        for no in [
            "",
            "Shutdown",
            "RestartApp",
            "TakeScreenshot",
            "SendString",
            "SendKey",
            "PlayMediaSource",
            "ClearPlaylist",
            "moveup",
            "../x",
            "SetPassword",
        ] {
            assert_eq!(
                cmd(no).validate().unwrap_err().0,
                StatusCode::BAD_REQUEST,
                "{no}"
            );
        }
        // the list is fixed and holds no duplicates
        let mut v = ALLOWED_COMMANDS.to_vec();
        v.sort_unstable();
        v.dedup();
        assert_eq!(v.len(), ALLOWED_COMMANDS.len());
        let args = |n: usize| (0..n).map(|i| (format!("k{i}"), "v".to_string())).collect();
        assert!(
            Cmd::General {
                name: "SetVolume".into(),
                args: args(8)
            }
            .validate()
            .is_ok()
        );
        assert!(
            Cmd::General {
                name: "SetVolume".into(),
                args: args(9)
            }
            .validate()
            .is_err()
        );
        let long = BTreeMap::from([("Text".to_string(), "x".repeat(501))]);
        assert!(
            Cmd::General {
                name: "DisplayMessage".into(),
                args: long
            }
            .validate()
            .is_err()
        );
        let odd = BTreeMap::from([("a b".to_string(), "x".to_string())]);
        assert!(
            Cmd::General {
                name: "SetVolume".into(),
                args: odd
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn play_and_playstate_validation() {
        let play = |c: &str, n: usize| Cmd::Play {
            command: c.into(),
            ids: vec!["a".into(); n],
            start_ticks: None,
            start_index: None,
            audio: None,
            subtitle: None,
        };
        assert!(play("PlayNow", 1).validate().is_ok() && play("PlayLast", 200).validate().is_ok());
        assert!(play("PlayNow", 0).validate().is_err() && play("PlayNow", 201).validate().is_err());
        assert!(play("Download", 1).validate().is_err());
        let ps = |c: &str, t| Cmd::Playstate {
            command: c.into(),
            seek_ticks: t,
        };
        assert!(ps("Pause", None).validate().is_ok() && ps("Seek", Some(10)).validate().is_ok());
        assert!(ps("Seek", None).validate().is_err() && ps("Seek", Some(-1)).validate().is_err());
        assert!(ps("Delete", None).validate().is_err());
        assert!(
            Cmd::Message {
                header: "h".into(),
                text: " ".into(),
                timeout_ms: None
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn query_parsing_is_case_insensitive_and_joins_repeats() {
        let q = qmap(vec![
            ("ItemIds".into(), "a,b".into()),
            ("itemids".into(), "c".into()),
            ("PlayCommand".into(), "PlayNow".into()),
            ("StartPositionTicks".into(), "100".into()),
        ]);
        let Cmd::Play {
            ids,
            command,
            start_ticks,
            ..
        } = play_cmd(&q)
        else {
            panic!()
        };
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!((command.as_str(), start_ticks), ("PlayNow", Some(100)));
    }

    #[test]
    fn native_frames_have_the_shapes_clients_parse() {
        let play = Cmd::Play {
            command: "PlayNow".into(),
            ids: vec!["x".into()],
            start_ticks: Some(5),
            start_index: None,
            audio: Some(1),
            subtitle: None,
        };
        let v: Value =
            serde_json::from_str(&native_message(&play, &["abc".into()], "srv", "usr")).unwrap();
        assert_eq!(v["MessageType"], "Play");
        assert_eq!(v["ServerId"], "srv");
        assert_eq!(v["Data"]["ItemIds"], json!(["abc"]));
        assert_eq!(v["Data"]["StartPositionTicks"], 5);
        assert_eq!(v["Data"]["AudioStreamIndex"], 1);
        assert!(v["Data"].get("StartIndex").is_none());
        let seek = Cmd::Playstate {
            command: "Seek".into(),
            seek_ticks: Some(90),
        };
        let v: Value = serde_json::from_str(&native_message(&seek, &[], "srv", "usr")).unwrap();
        assert_eq!(v["MessageType"], "Playstate");
        assert_eq!(v["Data"]["Command"], "Seek");
        assert_eq!(v["Data"]["SeekPositionTicks"], 90);
        let msg = Cmd::Message {
            header: "H".into(),
            text: "T".into(),
            timeout_ms: None,
        };
        let v: Value = serde_json::from_str(&native_message(&msg, &[], "srv", "usr")).unwrap();
        assert_eq!(v["MessageType"], "GeneralCommand");
        assert_eq!(v["Data"]["Name"], "DisplayMessage");
        assert_eq!(v["Data"]["Arguments"]["Text"], "T");
    }

    #[test]
    fn upstream_calls_carry_real_ids_and_no_gateway_media_source() {
        let play = Cmd::Play {
            command: "PlayNow".into(),
            ids: vec!["gw1".into(), "gw2".into()],
            start_ticks: Some(7),
            start_index: Some(1),
            audio: None,
            subtitle: None,
        };
        let c = up_call(&play, "realsess", &["r1".into(), "r2".into()], "acct");
        assert_eq!(c.path, "/Sessions/realsess/Playing");
        assert!(c.query.contains(&("ItemIds", "r1,r2".into())));
        assert!(c.query.contains(&("PlayCommand", "PlayNow".into())));
        assert!(c.query.contains(&("ControllingUserId", "acct".into())));
        assert!(
            c.query.iter().all(|(k, _)| *k != "MediaSourceId") && !format!("{c:?}").contains("gw1")
        );
        let c = up_call(
            &Cmd::Playstate {
                command: "Pause".into(),
                seek_ticks: None,
            },
            "s",
            &[],
            "acct",
        );
        assert_eq!(c.path, "/Sessions/s/Playing/Pause");
        assert!(c.body.is_none());
        let vol = Cmd::General {
            name: "SetVolume".into(),
            args: BTreeMap::from([("Volume".into(), "40".into())]),
        };
        let c = up_call(&vol, "s", &[], "acct");
        assert_eq!(c.path, "/Sessions/s/Command");
        assert_eq!(c.body.unwrap()["Arguments"]["Volume"], "40");
        let view = Cmd::Viewing {
            item_type: "Movie".into(),
            item_id: "g".into(),
            item_name: "N".into(),
        };
        let c = up_call(&view, "s", &["real".into()], "acct");
        assert!(c.query.contains(&("ItemId", "real".into())));
    }

    #[test]
    fn items_must_live_on_the_devices_server() {
        let m = |s: &str, p: bool| UpMember {
            source: s.into(),
            session: format!("sess-{s}"),
            playing: p,
        };
        let group = [m("A", false), m("B", true)];
        let item = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(s, i)| Member {
                    source: (*s).into(),
                    id: (*i).into(),
                })
                .collect::<Vec<_>>()
        };
        // an item on both: the first member wins for play, the playing one for transport
        let both = [item(&[("A", "a1"), ("B", "b1")])];
        let (mm, ids) = pick_member(&group, &both, false).unwrap();
        assert_eq!((mm.source.as_str(), ids), ("A", vec!["a1".to_string()]));
        let (mm, _) = pick_member(&group, &[], true).unwrap();
        assert_eq!(
            mm.source, "B",
            "transport goes to the member that is playing"
        );
        // only on B
        let only_b = [item(&[("B", "b9")])];
        assert_eq!(
            pick_member(&group, &only_b, false).unwrap().1,
            vec!["b9".to_string()]
        );
        // a title on another server entirely, or a mix no member holds in full: a clear 400
        let other = [item(&[("C", "c1")])];
        assert_eq!(
            pick_member(&group, &other, false).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        let mix = [item(&[("A", "a1")]), item(&[("B", "b1")])];
        assert_eq!(
            pick_member(&group, &mix, false).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        assert!(
            safe_segment("e1ebf-AB_9")
                && !safe_segment("a/b")
                && !safe_segment("")
                && !safe_segment("a?b")
        );
    }

    #[test]
    fn capabilities_are_reduced_to_safe_lists() {
        let c = Caps::from_json(&json!({
            "PlayableMediaTypes": ["Video", "Audio", "bad name", "x".repeat(41)],
            "SupportedCommands": ["SetVolume", "Mute", 5, null],
            "SupportsMediaControl": true,
            "DeviceProfile": {"huge": "ignored"},
        }));
        assert_eq!(c.playable, ["Video", "Audio"]);
        assert_eq!(c.commands, ["SetVolume", "Mute"]);
        assert!(c.media_control);
        let q = qmap(vec![
            ("SupportedCommands".into(), "SetVolume,,Mute".into()),
            ("SupportsMediaControl".into(), "True".into()),
        ]);
        let c = Caps::from_query(&q);
        assert_eq!(c.commands, ["SetVolume", "Mute"]);
        assert!(c.media_control);
        assert_eq!(Caps::from_json(&Value::Null), Caps::default());
    }

    #[test]
    fn snapshot_shows_sockets_and_the_caller_and_forgets_the_idle() {
        let hub = Hub::default();
        let who = |t: &str, d: &str| AuthCtx {
            token_id: t.into(),
            client: "C".into(),
            device_name: d.into(),
            device_id: d.into(),
            version: "1".into(),
        };
        let (a, b) = (who("ta", "da"), who("tb", "db"));
        let (conn, mut rx) = hub.attach(&a);
        hub.set_caps(&b, Caps::default());
        assert_eq!(hub.snapshot(None).len(), 1, "no socket, not listed");
        assert_eq!(
            hub.snapshot(Some(&b)).len(),
            2,
            "but a caller always sees itself"
        );
        assert_eq!(hub.deliver("ta", "hello"), 1);
        assert_eq!(rx.try_recv().unwrap(), "hello");
        assert_eq!(hub.deliver("tb", "x"), 0);
        hub.detach("ta", conn);
        assert!(hub.snapshot(None).is_empty());
        hub.natives
            .lock()
            .unwrap()
            .get_mut("tb")
            .unwrap()
            .last_activity = 0;
        hub.snapshot(None);
        assert!(
            hub.natives.lock().unwrap().get("tb").is_none(),
            "idle without a socket is forgotten"
        );
    }
}
