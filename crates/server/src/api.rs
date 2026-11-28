//! The HTTP API (docs/api/openapi.yaml): the router of the core routes and every module, the shared auth middleware, the per-module gate (503 while off), errors as `{"error": ...}`, and the events WebSocket.
use crate::auth::{self, Caller, TokenKind};
use crate::module::ApiRouter;
use crate::state::{Shared, now};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, Router};
use nocturne_core::server::{JobInfo, ServerConfig, Status};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How a request reached the server (set per connection by `http.rs`).
#[derive(Clone, Copy, Debug)]
pub enum Peer {
    /// The unix socket, peer uid checked: the local admin.
    Unix,
    Tcp(std::net::SocketAddr),
}

/// An error answer: `{"error": msg}` with a status.
#[derive(Debug)]
pub struct ApiErr(pub StatusCode, pub String);

impl ApiErr {
    pub fn bad(m: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, m.into())
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not found".into())
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, m.into())
    }
    pub fn forbidden(m: impl Into<String>) -> Self {
        Self(StatusCode::FORBIDDEN, m.into())
    }
}

impl From<String> for ApiErr {
    fn from(m: String) -> Self {
        Self::bad(m)
    }
}

impl From<&str> for ApiErr {
    fn from(m: &str) -> Self {
        Self::bad(m)
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiErr>;

pub fn ok() -> ApiResult<Value> {
    Ok(Json(json!({ "ok": true })))
}

pub fn saved(id: u64) -> ApiResult<Value> {
    Ok(Json(json!({ "id": id })))
}

/// Routes that need no credential.
fn public(m: &Method, path: &str) -> bool {
    matches!(
        (m.as_str(), path),
        ("GET", "/api/v1/health")
            | ("GET" | "POST", "/api/v1/setup")
            | ("POST", "/api/v1/auth/login")
    )
}

fn presented(req: &Request) -> Option<String> {
    let h = req.headers();
    if let Some(v) = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(t) = v.strip_prefix("Bearer ")
    {
        return Some(t.trim().to_string());
    }
    if let Some(v) = h.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    // Browsers cannot set headers on a WebSocket.
    if req.uri().path() == "/api/v1/events" {
        return req
            .uri()
            .query()?
            .split('&')
            .find_map(|kv| kv.strip_prefix("token=").map(String::from));
    }
    None
}

async fn auth_mw(State(sh): State<Shared>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/api/") || public(req.method(), path) {
        return next.run(req).await;
    }
    let caller = match req.extensions().get::<Peer>() {
        Some(Peer::Unix) => Some(Caller::Local),
        _ => presented(&req).and_then(|t| auth::lookup(&sh, &t)),
    };
    match caller {
        Some(c) => {
            req.extensions_mut().insert(c);
            next.run(req).await
        }
        None => ApiErr(
            StatusCode::UNAUTHORIZED,
            "sign in or send an API key".into(),
        )
        .into_response(),
    }
}

async fn gate(
    State((sh, name)): State<(Shared, &'static str)>,
    req: Request,
    next: Next,
) -> Response {
    if sh.modules().running(name) {
        next.run(req).await
    } else {
        ApiErr(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("the {name} module is off"),
        )
        .into_response()
    }
}

/// The whole app and the list of registered `(method, path)`.
pub fn build(sh: &Shared) -> (Router, Vec<(&'static str, String)>) {
    let core = routes(ApiRouter::new("/api/v1"));
    let mut router = core.router;
    let mut list = core.routes;
    for m in sh.modules().all() {
        let name = m.name();
        let r = m.routes(ApiRouter::new(&format!("/api/v1/{name}")));
        router = router.merge(r.router.layer(from_fn_with_state((sh.clone(), name), gate)));
        list.extend(r.routes);
    }
    let router = router
        .fallback(crate::web::serve)
        .layer(from_fn_with_state(sh.clone(), auth_mw))
        .with_state(sh.clone());
    (router, list)
}

fn routes(r: ApiRouter) -> ApiRouter {
    r.get("/health", health)
        .get("/status", status)
        .get("/config", get_config)
        .put("/config", put_config)
        .get("/setup", setup_needed)
        .post("/setup", setup)
        .post("/auth/login", login)
        .post("/auth/logout", logout)
        .get("/auth/me", me)
        .put("/auth/password", password)
        .get("/auth/sessions", sessions)
        .delete("/auth/sessions/{id}", revoke_session)
        .get("/apikeys", apikeys)
        .post("/apikeys", create_apikey)
        .delete("/apikeys/{id}", revoke_apikey)
        .get("/events", events)
        .get("/logs", logs)
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// Job state is kept by the library module; it fills this slot.
#[derive(Default)]
pub struct JobSlot(pub std::sync::Mutex<Vec<JobInfo>>);

/// Counters a module may fill for `/status` (the library module does).
#[derive(Default)]
pub struct Counters(pub std::sync::Mutex<(usize, usize, usize, u64, u64)>);

async fn status(State(sh): State<Shared>) -> Json<Status> {
    let (titles, missing, downloading, down_rate, up_rate) =
        *sh.ext::<Counters>().0.lock().unwrap();
    Json(Status {
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_secs: now().saturating_sub(sh.started),
        setup_needed: auth::setup_needed(&sh),
        modules: sh.modules().status(&sh),
        titles,
        missing,
        downloading,
        down_rate,
        up_rate,
        jobs: sh.ext::<JobSlot>().0.lock().unwrap().clone(),
        disk: disks(&disk_paths(&sh)),
    })
}

#[derive(Deserialize)]
struct LogsQ {
    lines: Option<usize>,
}

/// The newest server log lines (redacted when written), oldest first. Admin only.
async fn logs(
    Extension(c): Extension<Caller>,
    axum::extract::Query(q): axum::extract::Query<LogsQ>,
) -> ApiResult<Vec<Value>> {
    if !c.admin() {
        return Err(ApiErr::forbidden("only an admin may read the log"));
    }
    Ok(Json(
        crate::log::recent(q.lines.unwrap_or(200).min(crate::log::KEEP))
            .into_iter()
            .map(|(at, line)| json!({"at": at, "line": line}))
            .collect(),
    ))
}

/// More folders whose file systems `/status` reports (the storage module adds its mounts here).
#[derive(Default)]
pub struct DiskPaths(pub std::sync::Mutex<Vec<std::path::PathBuf>>);

/// Total and free bytes per file system of `paths` (one entry per file system, the first path naming it).
pub fn disks(paths: &[std::path::PathBuf]) -> Vec<nocturne_core::server::Disk> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for p in paths {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
                continue;
            };
            // SAFETY: statvfs fills the struct we own from a NUL-terminated path.
            let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 || !seen.insert(s.f_fsid) {
                continue;
            }
            let unit = s.f_frsize as u64;
            out.push(nocturne_core::server::Disk {
                path: p.clone(),
                total: s.f_blocks as u64 * unit,
                free: s.f_bavail as u64 * unit,
            });
        }
    }
    out
}

fn disk_paths(sh: &Shared) -> Vec<std::path::PathBuf> {
    let mut v = vec![sh.paths.data.clone()];
    if let Some(d) = sh.cfg().downloads.dir_or_default() {
        v.push(d);
    }
    v.extend(
        sh.db
            .all::<nocturne_core::server::Library>()
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.root),
    );
    v.extend(sh.ext::<DiskPaths>().0.lock().unwrap().iter().cloned());
    v
}

async fn get_config(State(sh): State<Shared>) -> Json<ServerConfig> {
    Json(sh.cfg())
}

async fn put_config(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(new): Json<ServerConfig>,
) -> ApiResult<ServerConfig> {
    if !c.admin() {
        return Err(ApiErr::forbidden(
            "only an admin may change the configuration",
        ));
    }
    sh.save_cfg(new)?;
    sh.modules().apply(&sh).await;
    Ok(Json(sh.cfg()))
}

async fn setup_needed(State(sh): State<Shared>) -> Json<Value> {
    Json(json!({ "needed": auth::setup_needed(&sh) }))
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct UserDto {
    id: u64,
    name: String,
    admin: bool,
    created: u64,
    local: bool,
}

fn login_result(sh: &Shared, u: &auth::UserRec, client: &str) -> ApiResult<Value> {
    let (token, _) = auth::mint(sh, u.id, TokenKind::Session, client)?;
    Ok(Json(json!({
        "token": token,
        "user": UserDto { id: u.id, name: u.name.clone(), admin: u.admin, created: u.created, local: false },
    })))
}

fn client_name(h: &HeaderMap) -> String {
    h.get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

/// One setup at a time, so two racing first-run requests cannot both create an admin.
static SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn setup(
    State(sh): State<Shared>,
    h: HeaderMap,
    Json(c): Json<Credentials>,
) -> ApiResult<Value> {
    let _one = SETUP.lock().await;
    if !auth::setup_needed(&sh) {
        return Err(ApiErr::conflict("setup is already done"));
    }
    let u = auth::create_user(&sh, &c.username, &c.password, true).await?;
    login_result(&sh, &u, &client_name(&h))
}

/// Failed sign-ins are answered one at a time, each after a second.
// ponytail: one global delay for failures; per-address limits if the server is exposed widely.
static FAILED: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn login(
    State(sh): State<Shared>,
    h: HeaderMap,
    Json(c): Json<Credentials>,
) -> ApiResult<Value> {
    match auth::verify(&sh, &c.username, &c.password).await {
        Some(u) => login_result(&sh, &u, &client_name(&h)),
        None => {
            let _one = FAILED.lock().await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            Err(ApiErr(
                StatusCode::UNAUTHORIZED,
                "wrong user name or password".into(),
            ))
        }
    }
}

async fn logout(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    if let Caller::User {
        token,
        kind: TokenKind::Session,
        ..
    } = c
    {
        auth::revoke(&sh, TokenKind::Session, token)?;
    }
    ok()
}

async fn me(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<UserDto> {
    Ok(Json(match c {
        Caller::Local => UserDto {
            id: 0,
            name: "local".into(),
            admin: true,
            created: sh.started,
            local: true,
        },
        Caller::User { user, .. } => {
            let u = sh
                .db
                .get::<auth::UserRec>(user)?
                .ok_or_else(ApiErr::not_found)?;
            UserDto {
                id: u.id,
                name: u.name,
                admin: u.admin,
                created: u.created,
                local: false,
            }
        }
    }))
}

#[derive(Deserialize)]
struct PasswordChange {
    #[serde(default)]
    current: String,
    new: String,
    /// The local admin resets a user's password by name (CLI `user password`).
    #[serde(default)]
    user: Option<String>,
}

async fn password(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(p): Json<PasswordChange>,
) -> ApiResult<Value> {
    let id = match (&c, &p.user) {
        (Caller::Local, Some(name)) => {
            sh.db
                .all::<auth::UserRec>()?
                .into_iter()
                .find(|u| u.name.eq_ignore_ascii_case(name))
                .ok_or_else(ApiErr::not_found)?
                .id
        }
        (Caller::User { user, name, .. }, _) => {
            if auth::verify(&sh, name, &p.current).await.is_none() {
                return Err(ApiErr::forbidden("the current password is wrong"));
            }
            *user
        }
        (Caller::Local, None) => return Err(ApiErr::bad("name the user")),
    };
    auth::set_password(&sh, id, &p.new).await?;
    auth::revoke_sessions(&sh)?;
    ok()
}

#[derive(Serialize)]
struct SessionDto {
    id: String,
    created: u64,
    last_used: u64,
    client: String,
    current: bool,
}

async fn sessions(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
) -> ApiResult<Vec<SessionDto>> {
    let cur = match c {
        Caller::User { token, .. } => token,
        Caller::Local => 0,
    };
    Ok(Json(
        auth::list(&sh, TokenKind::Session)?
            .into_iter()
            .map(|t| SessionDto {
                id: t.id.to_string(),
                created: t.created,
                last_used: t.last_used,
                client: t.name,
                current: t.id == cur,
            })
            .collect(),
    ))
}

fn revoke_kind(sh: &Shared, kind: TokenKind, id: &str) -> ApiResult<Value> {
    let id: u64 = id.parse().map_err(|_| ApiErr::not_found())?;
    if auth::revoke(sh, kind, id)? {
        ok()
    } else {
        Err(ApiErr::not_found())
    }
}

async fn revoke_session(State(sh): State<Shared>, Path(id): Path<String>) -> ApiResult<Value> {
    revoke_kind(&sh, TokenKind::Session, &id)
}

async fn revoke_apikey(State(sh): State<Shared>, Path(id): Path<String>) -> ApiResult<Value> {
    revoke_kind(&sh, TokenKind::ApiKey, &id)
}

async fn apikeys(State(sh): State<Shared>) -> ApiResult<Vec<Value>> {
    Ok(Json(
        auth::list(&sh, TokenKind::ApiKey)?
            .into_iter()
            .map(|t| json!({"id": t.id.to_string(), "name": t.name, "created": t.created, "last_used": t.last_used}))
            .collect(),
    ))
}

#[derive(Deserialize)]
struct KeyName {
    name: String,
}

async fn create_apikey(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(k): Json<KeyName>,
) -> ApiResult<Value> {
    if k.name.trim().is_empty() {
        return Err(ApiErr::bad("give the key a name"));
    }
    // The local admin's keys belong to the first admin user.
    let user = match c {
        Caller::User { user, .. } => user,
        Caller::Local => {
            sh.db
                .all::<auth::UserRec>()?
                .into_iter()
                .find(|u| u.admin)
                .ok_or_else(|| ApiErr::conflict("run the first-run setup first"))?
                .id
        }
    };
    let (key, r) = auth::mint(&sh, user, TokenKind::ApiKey, k.name.trim())?;
    Ok(Json(
        json!({"id": r.id.to_string(), "name": r.name, "key": key}),
    ))
}

async fn events(State(sh): State<Shared>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |s| pump(sh, s))
}

async fn pump(sh: Shared, mut s: WebSocket) {
    let mut rx = sh.subscribe();
    loop {
        tokio::select! {
            () = sh.stopped() => break,
            e = rx.recv() => match e {
                Ok(e) => {
                    let Ok(t) = serde_json::to_string(&e) else { continue };
                    if s.send(Message::Text(t.into())).await.is_err() {
                        break;
                    }
                }
                // A slow client skips what it missed; the next queue event carries the full state.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            m = s.recv() => match m {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
}
