//! `nocturne-server agent`: a small storage server for another machine. It exports the folders of `[storage.agent]` (list, stat, ranged reads, create-only writes and moves, free space) behind one API key and nothing else: no library, no modules, no database, no delete or overwrite. The main server adds it as an agent root (`POST /api/v1/storage/roots`, kind `agent`).
use super::storage::{AgentConfig, StorageConfig};
use crate::api::ApiErr;
use crate::state::Paths;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path as UrlPath, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use nocturne_core::gateway::{Binding, peer_allowed};
use nocturne_core::server::ServerConfig;
use nocturne_storage::agent::{AgentRoot, Move, PREFIX, valid_name};
use nocturne_storage::{Entry, fs};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

struct Agent {
    key_sha256: String,
    binding: Binding,
    roots: HashMap<String, PathBuf>,
}

type A = State<Arc<Agent>>;

pub fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn check(cfg: &AgentConfig) -> Result<(), String> {
    if cfg.key_sha256.is_empty() {
        return Err("no agent key yet: run `nocturne-server agent --new-key`".into());
    }
    if cfg.binding == Binding::Remote {
        return Err("the agent has no TLS: use binding \"local\" or \"lan\"".into());
    }
    if cfg.roots.is_empty() {
        return Err(
            "no roots: add [[storage.agent.roots]] with name and path to server.toml".into(),
        );
    }
    for r in &cfg.roots {
        if !valid_name(&r.name) || !r.path.is_absolute() {
            return Err(format!(
                "root {:?}: names use letters, digits, - and _; paths are absolute",
                r.name
            ));
        }
    }
    Ok(())
}

/// The agent's routes (also used in process by tests).
pub fn router(cfg: &AgentConfig) -> Result<Router, String> {
    check(cfg)?;
    let st = Arc::new(Agent {
        key_sha256: cfg.key_sha256.clone(),
        binding: cfg.binding,
        roots: cfg
            .roots
            .iter()
            .map(|r| (r.name.clone(), r.path.clone()))
            .collect(),
    });
    let p = |s: &str| format!("{PREFIX}{s}");
    Ok(Router::new()
        .route(&p("/roots"), get(roots))
        .route(&p("/roots/{root}/list"), get(list))
        .route(&p("/roots/{root}/stat"), get(stat))
        .route(&p("/roots/{root}/file"), get(read).put(write))
        .route(&p("/roots/{root}/move"), post(rename))
        .layer(from_fn_with_state(st.clone(), guard))
        .with_state(st))
}

/// Serve on one listener until it fails.
pub async fn serve(cfg: &AgentConfig, l: tokio::net::TcpListener) -> std::io::Result<()> {
    let r = router(cfg).map_err(std::io::Error::other)?;
    axum::serve(l, r.into_make_service_with_connect_info::<SocketAddr>()).await
}

/// Peer allowed by the binding, then the key.
async fn guard(
    State(a): A,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if !peer_allowed(a.binding, peer.ip()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let h = req.headers();
    let key = h
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            h.get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });
    match key {
        Some(k) if sha256_hex(k.trim()) == a.key_sha256 => next.run(req).await,
        _ => ApiErr(
            StatusCode::UNAUTHORIZED,
            "wrong or missing agent key".into(),
        )
        .into_response(),
    }
}

#[derive(Deserialize)]
struct P {
    #[serde(default)]
    path: String,
}

fn root(a: &Agent, name: &str) -> Result<PathBuf, ApiErr> {
    a.roots.get(name).cloned().ok_or_else(ApiErr::not_found)
}

async fn run<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiErr> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiErr::bad(e.to_string()))?
        .map_err(ApiErr::bad)
}

async fn roots(State(a): A) -> Json<Vec<AgentRoot>> {
    let list: Vec<(String, PathBuf)> = a
        .roots
        .iter()
        .map(|(n, p)| (n.clone(), p.clone()))
        .collect();
    let mut out = run(move || {
        Ok(list
            .into_iter()
            .map(|(name, p)| AgentRoot {
                name,
                writable: fs::writable(&p),
                space: fs::space(&p).ok(),
            })
            .collect::<Vec<_>>())
    })
    .await
    .unwrap_or_default();
    out.sort_by(|x, y| x.name.cmp(&y.name));
    Json(out)
}

async fn list(
    State(a): A,
    UrlPath(r): UrlPath<String>,
    Query(q): Query<P>,
) -> Result<Json<Vec<Entry>>, ApiErr> {
    let base = root(&a, &r)?;
    run(move || fs::list(&fs::resolve(&base, &q.path)?).map_err(|e| e.to_string()))
        .await
        .map(Json)
}

async fn stat(
    State(a): A,
    UrlPath(r): UrlPath<String>,
    Query(q): Query<P>,
) -> Result<Json<Entry>, ApiErr> {
    let base = root(&a, &r)?;
    match run(move || fs::resolve(&base, &q.path)).await {
        Ok(p) => tokio::task::spawn_blocking(move || fs::stat(&p))
            .await
            .map_err(|e| ApiErr::bad(e.to_string()))?
            .map(Json)
            .map_err(|_| ApiErr::not_found()),
        Err(e) => Err(e),
    }
}

/// A file, whole or one byte range (`Range: bytes=`), streamed.
async fn read(
    State(a): A,
    UrlPath(r): UrlPath<String>,
    Query(q): Query<P>,
    headers: HeaderMap,
) -> Result<Response, ApiErr> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let base = root(&a, &r)?;
    let path = run(move || fs::resolve(&base, &q.path)).await?;
    let mut f = tokio::fs::File::open(&path)
        .await
        .map_err(|_| ApiErr::not_found())?;
    let meta = f.metadata().await.map_err(|e| ApiErr::bad(e.to_string()))?;
    if !meta.is_file() {
        return Err(ApiErr::bad("not a file"));
    }
    let len = meta.len();
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (status, first, count) = match range.map(|h| fs::parse_range(h, len)) {
        Some(Err(fs::Unsatisfiable)) => {
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{len}"))],
            )
                .into_response());
        }
        Some(Ok(Some((a, b)))) => (StatusCode::PARTIAL_CONTENT, a, b - a + 1),
        _ => (StatusCode::OK, 0, len),
    };
    f.seek(std::io::SeekFrom::Start(first))
        .await
        .map_err(|e| ApiErr::bad(e.to_string()))?;
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(f.take(count)));
    let mut resp = Response::builder()
        .status(status)
        .header(header::CONTENT_LENGTH, count)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if status == StatusCode::PARTIAL_CONTENT {
        resp = resp.header(
            header::CONTENT_RANGE,
            format!("bytes {first}-{}/{len}", first + count - 1),
        );
    }
    resp.body(body).map_err(|e| ApiErr::bad(e.to_string()))
}

/// Create a file from the request body (refused if it exists; missing folders are made). Written to a hidden part file first, renamed when complete.
async fn write(
    State(a): A,
    UrlPath(r): UrlPath<String>,
    Query(q): Query<P>,
    body: Body,
) -> Result<Json<serde_json::Value>, ApiErr> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let base = root(&a, &r)?;
    let path = run(move || fs::resolve_new(&base, &q.path)).await?;
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(ApiErr::bad("a file name is needed"));
    };
    if path.symlink_metadata().is_ok() {
        return Err(ApiErr::conflict("the file exists"));
    }
    let part = dir.join(format!(".{}.nocturne-part", name.to_string_lossy()));
    let mut f = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .await
        .map_err(|e| ApiErr::conflict(format!("cannot create the file: {e}")))?;
    let mut stream = body.into_data_stream();
    let result: Result<(), String> = async {
        while let Some(chunk) = stream.next().await {
            f.write_all(&chunk.map_err(|e| e.to_string())?)
                .await
                .map_err(|e| e.to_string())?;
        }
        f.sync_all().await.map_err(|e| e.to_string())?;
        if path.symlink_metadata().is_ok() {
            return Err("the file exists".into());
        }
        tokio::fs::rename(&part, &path)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(ApiErr::bad(e));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Rename inside one root (refused if the target exists; missing folders are made).
async fn rename(
    State(a): A,
    UrlPath(r): UrlPath<String>,
    Json(m): Json<Move>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let base = root(&a, &r)?;
    run(move || {
        let from = fs::resolve(&base, &m.from)?;
        if from.symlink_metadata().is_err() || from == base {
            return Err("no such file".into());
        }
        let to = fs::resolve_new(&base, &m.to)?;
        if to.symlink_metadata().is_ok() {
            return Err("the target exists".into());
        }
        std::fs::rename(&from, &to).map_err(|e| e.to_string())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// A new agent key; only its hash is stored. A running server holds the configuration in memory and writes all of it back on every change, so while one answers on the socket the hash goes through it; editing the file behind its back was undone by the next save.
pub fn new_key(paths: &Paths) -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    let key: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let key = format!("nsa_{key}");
    let hash = sha256_hex(&key);
    let running = paths
        .socket()
        .filter(|s| std::os::unix::net::UnixStream::connect(s).is_ok());
    if let Some(sock) = running {
        let ok = |(code, v): (u16, serde_json::Value)| {
            if (200..300).contains(&code) {
                Ok(v)
            } else {
                Err(v["error"]
                    .as_str()
                    .unwrap_or("the server refused")
                    .to_string())
            }
        };
        let mut v = ok(crate::cli::call(&sock, "GET", "/api/v1/config", None)?)?;
        v["storage"]["agent"]["key_sha256"] = hash.into();
        ok(crate::cli::call(&sock, "PUT", "/api/v1/config", Some(&v))?)?;
    } else {
        let mut cfg = ServerConfig::load_from(&paths.config)?;
        let mut s: StorageConfig = cfg.section("storage");
        s.agent.key_sha256 = hash;
        cfg.set_section("storage", &s)?;
        cfg.save_to(&paths.config)?;
    }
    Ok(key)
}

/// `nocturne-server agent [--new-key]`. Returns the exit code.
pub fn agent_main(args: &[String]) -> i32 {
    let result = (|| {
        let paths = Paths::system()?;
        if args.first().map(String::as_str) == Some("--new-key") {
            let key = new_key(&paths)?;
            eprintln!(
                "New agent key below (shown once; only its hash is kept in {}). Give it to the main server when adding this agent as a storage root:",
                paths.config.display()
            );
            println!("{key}");
            return Ok(());
        }
        if let Some(a) = args.first() {
            return Err(format!(
                "unknown agent option {a:?} (use --new-key or nothing)"
            ));
        }
        let cfg: StorageConfig = ServerConfig::load_from(&paths.config)?.section("storage");
        check(&cfg.agent)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let ls = crate::http::bind(cfg.agent.binding, cfg.agent.port)
                .await
                .map_err(|e| format!("cannot listen on port {}: {e}", cfg.agent.port))?;
            for l in &ls {
                if let Ok(a) = l.local_addr() {
                    eprintln!("nocturne-server agent: listening on {a}");
                }
            }
            let tasks: Vec<_> = ls
                .into_iter()
                .map(|l| {
                    let c = cfg.agent.clone();
                    tokio::spawn(async move { serve(&c, l).await })
                })
                .collect();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = async {
                    if let Ok(mut t) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                        t.recv().await;
                    }
                } => {}
                _ = async { for t in tasks { let _ = t.await; } } => {}
            }
            Ok::<_, String>(())
        })
    })();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("nocturne-server agent: {e}");
            1
        }
    }
}
