//! The router, its allow-list and middleware, the public routes, and the accept loop.
//!
//! Every route is registered through [`Routes::add`] (or [`Routes::add_stream`]) with an explicit [`Access`]; there is no other way to put a route into the router, so there is no unannotated route. Everything not registered answers 404.
use crate::auth::{AuthCtx, ClientInfo, client_info, status_json};
use crate::config::Config;
use crate::limits::limit_key;
use crate::state::State;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get};
use hyper_util::rt::{TokioIo, TokioTimer};
use nocturne_core::gateway::{self as core_gw, Binding};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower::ServiceExt;

/// Who may call a route. Part of every registration, listed in [`RouteInfo`] and checked by a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// No token: only the routes clients call before they can sign in.
    Public,
    /// A valid gateway token.
    Auth,
    /// A valid token AND `gateway_remote_control` on (sessions and commands); 403 otherwise.
    Remote,
}

#[derive(Clone, Debug)]
pub struct RouteInfo {
    pub path: &'static str,
    pub access: Access,
    /// No per-request timeout (media, WebSocket).
    pub streaming: bool,
}

/// The effective client address (the proxy's `X-Forwarded-For` only when the peer is the configured trusted proxy). Extension of every request.
#[derive(Clone, Copy, Debug)]
pub struct ClientIp(pub IpAddr);

/// The TCP peer. Extension of every request.
#[derive(Clone, Copy, Debug)]
pub struct Peer(pub SocketAddr);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_LIMIT: usize = 1 << 20;
const HEADER_LIMIT: usize = 16 * 1024;

/// The route registry. A module adds its routes in `pub fn routes(r: Routes) -> Routes`.
pub struct Routes {
    state: State,
    router: Router<State>,
    table: Vec<RouteInfo>,
}

impl Routes {
    pub fn new(state: State) -> Self {
        Self {
            state,
            router: Router::new(),
            table: vec![],
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Register `path` (axum 0.8 syntax: `/Items/{itemId}`; use the parameter names in the `lib.rs` table) with its access class.
    pub fn add(self, path: &'static str, access: Access, mr: MethodRouter<State>) -> Self {
        self.add_inner(path, access, false, mr)
    }

    /// Like [`Routes::add`] for media and WebSocket routes: no 60 s request timeout.
    pub fn add_stream(self, path: &'static str, access: Access, mr: MethodRouter<State>) -> Self {
        self.add_inner(path, access, true, mr)
    }

    fn add_inner(
        mut self,
        path: &'static str,
        access: Access,
        streaming: bool,
        mr: MethodRouter<State>,
    ) -> Self {
        let st = self.state.clone();
        let mr = mr.layer(middleware::from_fn(move |req: Request, next: Next| {
            let st = st.clone();
            async move { gate(st, access, streaming, req, next).await }
        }));
        self.router = self.router.route(path, mr);
        self.table.push(RouteInfo {
            path,
            access,
            streaming,
        });
        self
    }

    pub fn finish(self) -> (Router, Vec<RouteInfo>) {
        let tls = self.state.cfg().gw.tls();
        let router = self
            .router
            .fallback(|req: Request| async move {
                let path = req.uri().path();
                // Path only (never the query, which can carry a token): shows which routes real clients still miss.
                crate::glog!("404 {} {}", req.method(), path);
                StatusCode::NOT_FOUND.into_response()
            })
            .layer(DefaultBodyLimit::max(BODY_LIMIT))
            .layer(middleware::from_fn(
                move |req: Request, next: Next| async move {
                    let mut r = next.run(req).await;
                    let h = r.headers_mut();
                    h.insert(
                        header::REFERRER_POLICY,
                        HeaderValue::from_static("no-referrer"),
                    );
                    h.insert(
                        header::X_CONTENT_TYPE_OPTIONS,
                        HeaderValue::from_static("nosniff"),
                    );
                    if tls {
                        h.insert(
                            header::STRICT_TRANSPORT_SECURITY,
                            HeaderValue::from_static("max-age=31536000"),
                        );
                    }
                    r
                },
            ))
            .with_state(self.state);
        (router, self.table)
    }
}

/// Every route of the gateway, public and authenticated. Modules of workers B and C are registered here and nowhere else.
pub fn build(state: &State) -> (Router, Vec<RouteInfo>) {
    let r = Routes::new(state.clone());
    let r = public(r);
    let r = crate::auth::routes(r);
    let r = crate::misc::routes(r);
    let r = crate::items::routes(r);
    let r = crate::images::routes(r);
    let r = crate::stream::routes(r);
    let r = crate::playback::routes(r);
    let r = crate::userdata::routes(r);
    let r = crate::sessions::routes(r);
    let r = crate::ws::routes(r);
    let r = crate::web::routes(r);
    let (router, table) = r.finish();
    // Jellyfin routes ignore case (`/system/info/public` works against a real server and clients use it): the path is put into the registered spelling before routing.
    let paths: std::sync::Arc<[&'static str]> = table.iter().map(|t| t.path).collect();
    let outer = Router::new().fallback_service(tower::ServiceExt::map_request(
        router,
        move |mut req: Request| {
            // Jellyfin still accepts the old `/emby` and `/mediabrowser` prefixes that some clients send.
            let path = req.uri().path();
            let lower = path.to_ascii_lowercase();
            let stripped = ["/emby", "/mediabrowser"]
                .iter()
                .find(|p| lower.starts_with(*p) && lower[p.len()..].starts_with('/'))
                .map(|p| path[p.len()..].to_string());
            let base = stripped.clone().unwrap_or_else(|| path.to_string());
            if let Some(p) = canonical_path(&paths, &base).or(stripped) {
                let pq = match req.uri().query() {
                    Some(q) => format!("{p}?{q}"),
                    None => p,
                };
                let mut parts = req.uri().clone().into_parts();
                if let Ok(v) = pq.parse() {
                    parts.path_and_query = Some(v);
                    if let Ok(u) = axum::http::Uri::from_parts(parts) {
                        *req.uri_mut() = u;
                    }
                }
            }
            req
        },
    ));
    (outer, table)
}

/// The registered spelling of `path` when it differs only in the case of fixed segments (`{param}` segments are kept as sent). `None`: no change needed or no route fits.
pub fn canonical_path(templates: &[&str], path: &str) -> Option<String> {
    let segs: Vec<&str> = path.split('/').collect();
    let mut best: Option<(usize, String)> = None;
    for t in templates {
        let ts: Vec<&str> = t.split('/').collect();
        if ts.len() != segs.len() {
            continue;
        }
        let mut fixed = 0;
        let mut out = Vec::with_capacity(ts.len());
        let mut ok = true;
        for (a, b) in ts.iter().zip(&segs) {
            if a.starts_with('{') {
                out.push(*b);
            } else if a.eq_ignore_ascii_case(b) {
                fixed += 1;
                out.push(*a);
            } else {
                ok = false;
                break;
            }
        }
        if ok && best.as_ref().is_none_or(|(f, _)| fixed > *f) {
            best = Some((fixed, out.join("/")));
        }
    }
    best.map(|(_, p)| p).filter(|p| p != path)
}

fn effective_ip(cfg: &Config, peer: IpAddr, headers: &axum::http::HeaderMap) -> IpAddr {
    let peer = core_gw::normalize(peer);
    if cfg.gw.trusted_proxy.map(core_gw::normalize) == Some(peer)
        && let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(ip) = v
            .rsplit(',')
            .next()
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
    {
        return core_gw::normalize(ip);
    }
    peer
}

/// The per-route middleware: client address, per-IP limit, token check, per-token limit, remote-control switch, timeout.
async fn gate(
    st: State,
    access: Access,
    streaming: bool,
    mut req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<Peer>()
        .map_or(IpAddr::from([127, 0, 0, 1]), |p| p.0.ip());
    let cfg = st.cfg();
    let ip = effective_ip(&cfg, peer, req.headers());
    req.extensions_mut().insert(ClientIp(ip));
    let now = Instant::now();
    if !st.limits.ip.allow(&limit_key(ip), now) {
        return status_json(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }
    let mut ci: ClientInfo = client_info(req.headers(), req.uri().query());
    if access != Access::Public {
        let Some(rec) = ci.token.as_deref().and_then(|t| st.tokens.lookup(t)) else {
            return status_json(StatusCode::UNAUTHORIZED, "Unauthorized");
        };
        if !st.limits.token.allow(&rec.id, now) {
            return status_json(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
        }
        if access == Access::Remote && !cfg.gw.remote_control {
            return status_json(StatusCode::FORBIDDEN, "Remote control is off");
        }
        req.extensions_mut().insert(AuthCtx::from(rec));
    }
    // Handlers never see the client's token: nothing downstream can forward or log it by accident.
    ci.token = None;
    req.extensions_mut().insert(ci);
    if streaming {
        next.run(req).await
    } else {
        match tokio::time::timeout(REQUEST_TIMEOUT, next.run(req)).await {
            Ok(r) => r,
            Err(_) => status_json(StatusCode::GATEWAY_TIMEOUT, "Timed out"),
        }
    }
}

// ---------------------------------------------------------------- public routes

fn public_info(st: &State) -> serde_json::Value {
    json!({
        "ServerName": "NOCTURNE",
        "Version": core_gw::ADVERTISED_VERSION,
        "ProductName": "Jellyfin Server",
        "OperatingSystem": "",
        "Id": st.users.server_id(),
        "StartupWizardCompleted": true,
    })
}

fn public(r: Routes) -> Routes {
    use axum::extract::State as St;
    let css = || async { ([(header::CONTENT_TYPE, "text/css")], "") };
    r.add(
        "/System/Info/Public",
        Access::Public,
        get(|St(st): St<State>| async move { Json(public_info(&st)) }),
    )
    .add(
        "/System/Ping",
        Access::Public,
        get(|| async { Json("NOCTURNE") }).post(|| async { Json("NOCTURNE") }),
    )
    .add(
        "/System/Info",
        Access::Auth,
        get(|St(st): St<State>| async move {
            let mut v = public_info(&st);
            v["LocalAddress"] = json!("");
            Json(v)
        }),
    )
    .add(
        "/Branding/Configuration",
        Access::Public,
        get(|| async {
            Json(json!({"LoginDisclaimer": null, "CustomCss": null, "SplashscreenEnabled": false}))
        }),
    )
    .add("/Branding/Css", Access::Public, get(css))
    .add("/Branding/Css.css", Access::Public, get(css))
    // No user enumeration: the login picker is always empty.
    .add(
        "/Users/Public",
        Access::Public,
        get(|| async { Json(json!([])) }),
    )
}

// ---------------------------------------------------------------- listeners

/// This machine's address towards the LAN (no packet is sent: a UDP "connect" only picks the route).
pub fn lan_host(binding: Binding) -> String {
    if binding == Binding::Local {
        return "127.0.0.1".into();
    }
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|()| s.local_addr()))
        .map_or_else(|_| "127.0.0.1".into(), |a| a.ip().to_string())
}

/// Whether a peer is served at all in this mode (checked at accept time, before any bytes are read).
pub fn admit(cfg: &Config, peer: IpAddr) -> bool {
    core_gw::peer_allowed(cfg.gw.binding, peer)
}

pub async fn bind(cfg: &Config) -> std::io::Result<Vec<TcpListener>> {
    let [v4, v6] = core_gw::bind_addrs(cfg.gw.binding);
    // Wildcard: the IPv6 socket first (dual-stack takes IPv4 too); loopback: IPv4 first.
    let order = if cfg.gw.binding == Binding::Local {
        [v4, v6]
    } else {
        [v6, v4]
    };
    let mut out: Vec<TcpListener> = vec![];
    let mut port = cfg.gw.port;
    let mut last = None;
    for ip in order {
        match TcpListener::bind((ip, port)).await {
            Ok(l) => {
                port = l.local_addr()?.port();
                out.push(l);
            }
            Err(e) => last = Some(e),
        }
    }
    match (out.is_empty(), last) {
        (true, Some(e)) => Err(e),
        _ => Ok(out),
    }
}

/// The router and the case table every connection needs.
#[derive(Clone)]
struct Front {
    router: Router,
    cases: std::sync::Arc<CaseMap>,
}

#[derive(Clone, Debug)]
enum Seg {
    Lit(&'static str),
    Param,
    /// A parameter inside a segment (`stream.{container}`): literal prefix and suffix.
    Mixed(&'static str, &'static str),
}

/// Registered paths by segment count. ASP.NET-style clients send `/users/me` or `/ITEMS/...` in any case; the router is case-sensitive, so literal segments are put into the registered case before routing. Parameter segments (ids, names) are kept as sent.
pub struct CaseMap(std::collections::HashMap<usize, Vec<Vec<Seg>>>);

impl CaseMap {
    pub fn new(table: &[RouteInfo]) -> Self {
        let mut m: std::collections::HashMap<usize, Vec<Vec<Seg>>> = Default::default();
        for r in table {
            let segs: Vec<Seg> = r
                .path
                .split('/')
                .map(|s| match (s.find('{'), s.find('}')) {
                    (Some(0), Some(e)) if e + 1 == s.len() => Seg::Param,
                    (Some(a), Some(e)) => Seg::Mixed(&s[..a], &s[e + 1..]),
                    _ => Seg::Lit(s),
                })
                .collect();
            m.entry(segs.len()).or_default().push(segs);
        }
        Self(m)
    }

    /// The path in registered case, or `None` when it needs no change (or matches no route: the router answers 404).
    pub fn fix(&self, path: &str) -> Option<String> {
        let req: Vec<&str> = path.split('/').collect();
        let mut best: Option<(usize, &Vec<Seg>)> = None;
        for t in self.0.get(&req.len())? {
            let mut score = 0;
            let ok = t.iter().zip(&req).all(|(s, r)| match s {
                Seg::Lit(l) if r.eq_ignore_ascii_case(l) => {
                    score += 2;
                    true
                }
                Seg::Lit(_) => false,
                Seg::Param => true,
                Seg::Mixed(p, x) => {
                    let (rl, ok) = (r.to_ascii_lowercase(), r.len() >= p.len() + x.len());
                    score += 1;
                    ok && rl.starts_with(&p.to_ascii_lowercase())
                        && rl.ends_with(&x.to_ascii_lowercase())
                }
            });
            if ok && best.is_none_or(|(b, _)| score > b) {
                best = Some((score, t));
            }
        }
        let (_, t) = best?;
        let fixed: Vec<String> = t
            .iter()
            .zip(&req)
            .map(|(s, r)| match s {
                Seg::Lit(l) => (*l).to_string(),
                Seg::Param => (*r).to_string(),
                Seg::Mixed(p, x) => format!("{p}{}{x}", &r[p.len()..r.len() - x.len()]),
            })
            .collect();
        let out = fixed.join("/");
        (out != path).then_some(out)
    }

    fn apply(&self, req: &mut hyper::Request<Body>) {
        let Some(path) = self.fix(req.uri().path()) else {
            return;
        };
        let pq = match req.uri().query() {
            Some(q) => format!("{path}?{q}"),
            None => path,
        };
        let mut parts = req.uri().clone().into_parts();
        if let Ok(p) = pq.parse() {
            parts.path_and_query = Some(p);
            if let Ok(u) = axum::http::Uri::from_parts(parts) {
                *req.uri_mut() = u;
            }
        }
    }
}

async fn accept_loop(
    st: State,
    l: TcpListener,
    router: Front,
    tls: Option<tokio_rustls::TlsAcceptor>,
) {
    loop {
        let Ok((stream, peer)) = l.accept().await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let cfg = st.cfg();
        if !admit(&cfg, peer.ip()) {
            continue;
        }
        let Some(guard) = st.limits.conns.try_acquire(peer.ip()) else {
            continue;
        };
        let _ = stream.set_nodelay(true);
        let (router, tls) = (router.clone(), tls.clone());
        tokio::spawn(async move {
            let _guard = guard;
            match tls {
                None => serve_conn(stream, peer, router).await,
                Some(a) => {
                    if let Ok(Ok(s)) =
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, a.accept(stream)).await
                    {
                        serve_conn(s, peer, router).await
                    }
                }
            }
        });
    }
}

async fn serve_conn<S>(io: S, peer: SocketAddr, router: Front)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
        let mut req = req.map(Body::new);
        req.extensions_mut().insert(Peer(peer));
        router.cases.apply(&mut req);
        let r = router.router.clone();
        async move { r.oneshot(req).await }
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .max_buf_size(HEADER_LIMIT)
        .serve_connection(TokioIo::new(io), svc)
        .with_upgrades()
        .await;
}

async fn start_listeners(st: &State) -> Result<Vec<JoinHandle<()>>, String> {
    let cfg = st.cfg();
    cfg.gw.validate()?;
    let tls = match (cfg.gw.tls(), &cfg.gw.cert_path, &cfg.gw.key_path) {
        (true, Some(c), Some(k)) => Some(crate::tls::load(c, k)?),
        _ => None,
    };
    let listeners = bind(&cfg)
        .await
        .map_err(|e| format!("cannot listen on port {}: {e}", cfg.gw.port))?;
    let addrs: Vec<SocketAddr> = listeners
        .iter()
        .filter_map(|l| l.local_addr().ok())
        .collect();
    st.set_bound(addrs.clone());
    let (router, table) = build(st);
    let router = Front {
        router,
        cases: std::sync::Arc::new(CaseMap::new(&table)),
    };
    let mut tasks: Vec<_> = listeners
        .into_iter()
        .map(|l| tokio::spawn(accept_loop(st.clone(), l, router.clone(), tls.clone())))
        .collect();
    if cfg.gw.binding == Binding::Lan && cfg.gw.announce {
        let port = addrs.first().map_or(cfg.gw.port, |a| a.port());
        tasks.push(tokio::spawn(crate::discovery::run(st.clone(), port)));
    }
    crate::glog!(
        "listening on {}",
        addrs
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(tasks)
}

/// Serve until stopped: bind, accept, rebind when a reload changes the listener settings, run the control socket and housekeeping.
pub async fn run(st: State) -> std::io::Result<()> {
    crate::control::spawn(&st)?;
    tokio::spawn(housekeeping(st.clone()));
    let mut changed = st.relisten_rx();
    let mut first = true;
    loop {
        match start_listeners(&st).await {
            Ok(tasks) => {
                st.set_error(None);
                tokio::select! {
                    () = st.stopped() => {
                        tasks.iter().for_each(JoinHandle::abort);
                        break;
                    }
                    _ = changed.changed() => {
                        tasks.iter().for_each(JoinHandle::abort);
                        st.set_bound(vec![]);
                    }
                }
            }
            Err(e) => {
                if first {
                    return Err(std::io::Error::other(e));
                }
                crate::glog!("{e}");
                st.set_error(Some(e));
                tokio::select! {
                    () = st.stopped() => break,
                    _ = changed.changed() => {}
                    () = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
            }
        }
        first = false;
    }
    let _ = st.tokens.persist();
    let _ = st.ids.persist();
    crate::control::cleanup(&st);
    Ok(())
}

async fn housekeeping(st: State) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.tick().await;
    loop {
        tokio::select! { () = st.stopped() => return, _ = tick.tick() => {} }
        let _ = st.tokens.persist();
        st.ids.prune();
        let _ = st.ids.persist();
        st.qc.prune_expired(Instant::now());
        st.limits.prune(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_for_only_from_the_trusted_proxy() {
        let mut cfg = Config::default();
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-forwarded-for", "9.9.9.9, 203.0.113.7".parse().unwrap());
        let proxy: IpAddr = "10.0.0.2".parse().unwrap();
        let other: IpAddr = "10.0.0.3".parse().unwrap();
        assert_eq!(
            effective_ip(&cfg, proxy, &h),
            proxy,
            "no proxy configured: header ignored"
        );
        cfg.gw.trusted_proxy = Some(proxy);
        assert_eq!(
            effective_ip(&cfg, proxy, &h),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            effective_ip(&cfg, other, &h),
            other,
            "a spoofer is not the proxy"
        );
        h.insert("x-forwarded-for", "garbage".parse().unwrap());
        assert_eq!(effective_ip(&cfg, proxy, &h), proxy);
    }

    #[test]
    fn literal_segments_take_the_registered_case_and_parameters_stay() {
        let route = |path| RouteInfo {
            path,
            access: Access::Auth,
            streaming: false,
        };
        let m = CaseMap::new(&[
            route("/Users/Me"),
            route("/Users/{userId}"),
            route("/Items/{itemId}/Images/{imageType}"),
            route("/Items/Latest"),
            route("/Items/{itemId}"),
            route("/Videos/{itemId}/stream.{container}"),
        ]);
        let fix = |p: &str| m.fix(p).unwrap_or_else(|| p.to_string());
        assert_eq!(fix("/users/me"), "/Users/Me");
        assert_eq!(fix("/USERS/Me"), "/Users/Me");
        assert_eq!(
            fix("/Users/AbC123"),
            "/Users/AbC123",
            "a parameter is kept as sent"
        );
        assert_eq!(
            fix("/items/AbC/images/primary"),
            "/Items/AbC/Images/primary",
            "the image type is a parameter"
        );
        assert_eq!(
            fix("/items/latest"),
            "/Items/Latest",
            "a literal wins over a parameter"
        );
        assert_eq!(fix("/items/Latestx"), "/Items/Latestx");
        assert_eq!(
            fix("/videos/ABC/STREAM.MKV"),
            "/Videos/ABC/stream.MKV",
            "suffix parameter keeps the extension as sent"
        );
        assert_eq!(fix("/nothing/here"), "/nothing/here");
        assert_eq!(fix("/Items/Latest"), "/Items/Latest");
    }

    #[test]
    fn admit_follows_the_binding_mode() {
        let mut cfg = Config::default();
        let lan: IpAddr = "192.168.1.20".parse().unwrap();
        let wan: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(admit(&cfg, "127.0.0.1".parse().unwrap()) && !admit(&cfg, lan));
        cfg.gw.binding = Binding::Lan;
        assert!(admit(&cfg, lan) && !admit(&cfg, wan));
        cfg.gw.binding = Binding::Remote;
        assert!(admit(&cfg, wan));
    }
}

#[cfg(test)]
mod case_tests {
    use super::canonical_path;

    #[test]
    fn paths_are_matched_without_regard_to_case() {
        let t = [
            "/System/Info/Public",
            "/Users/{userId}",
            "/Users/Me",
            "/Sessions/{sessionId}/Playing/{command}",
        ];
        let c = |p| canonical_path(&t, p);
        assert_eq!(
            c("/system/info/public").as_deref(),
            Some("/System/Info/Public")
        );
        assert_eq!(
            c("/SYSTEM/INFO/PUBLIC").as_deref(),
            Some("/System/Info/Public")
        );
        assert_eq!(
            c("/users/me").as_deref(),
            Some("/Users/Me"),
            "the fixed segment wins over the parameter"
        );
        assert_eq!(
            c("/users/AbC").as_deref(),
            Some("/Users/AbC"),
            "parameters keep the sender's case"
        );
        assert_eq!(
            c("/sessions/AbC/playing/Pause").as_deref(),
            Some("/Sessions/AbC/Playing/Pause")
        );
        assert_eq!(c("/System/Info/Public"), None, "already right");
        assert_eq!(c("/nothing/here"), None);
    }
}
