//! `tunnel`: optional remote access through the user's own installed `cloudflared` (never required, never downloaded by NOCTURNE). Two modes:
//! * `token`: a named tunnel made in the Cloudflare dashboard; its token is kept in the keyring (`server:tunnel:token`) and handed to `cloudflared tunnel run` through the `TUNNEL_TOKEN` environment variable, never on the command line (where `ps` shows it). Its public hostnames and their targets are set in the dashboard; point them at the addresses `GET /tunnel/status` lists.
//! * `quick`: a throwaway `trycloudflare.com` address for testing, no account; it exposes one target (`expose`: the dashboard or the gateway) and its address changes on every start.
//!
//! The process is supervised: restarted with a growing pause when it exits, stopped by its own handle (never by name).
use crate::api::{ApiErr, ApiResult, ok};
use crate::auth::Caller;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::state::Shared;
use axum::extract::State;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;

const NAME: &str = "tunnel";
const SECRET: &str = "tunnel:token";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Token,
    Quick,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expose {
    #[default]
    Dashboard,
    Gateway,
}

/// `[tunnel]` in `server.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TunnelCfg {
    pub enabled: bool,
    pub mode: Mode,
    /// What a quick tunnel points at.
    pub expose: Expose,
    /// `cloudflared` to run; `None` = found on `PATH`.
    pub cloudflared: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TunnelStatus {
    /// `starting`, `connected`, `restarting`, `stopped`.
    pub state: String,
    pub mode: Mode,
    /// The public address of a quick tunnel, once `cloudflared` printed it.
    pub url: Option<String>,
    /// Local addresses a token tunnel's hostnames should point at.
    pub targets: Vec<String>,
    pub token_set: bool,
    pub restarts: u32,
    pub error: Option<String>,
}

#[derive(Default)]
struct Run {
    status: Mutex<TunnelStatus>,
    stop: Mutex<Option<watch::Sender<bool>>>,
}

/// `cloudflared` on `PATH` (or the configured file), if it is there and executable.
pub fn find(cfg: &TunnelCfg) -> Option<PathBuf> {
    fn exe(p: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    if let Some(p) = &cfg.cloudflared {
        return exe(p).then(|| p.clone());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join("cloudflared"))
        .find(|p| exe(p))
}

fn targets(sh: &Shared) -> (String, String) {
    let gw: nocturne_core::gateway::GatewaySettings = sh.cfg().section("gateway");
    (
        format!("http://127.0.0.1:{}", sh.cfg().http.port),
        format!("http://127.0.0.1:{}", gw.port),
    )
}

/// The public address in a line of `cloudflared` output.
fn quick_url(line: &str) -> Option<String> {
    let i = line.find("https://")?;
    let u: String = line[i..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-'))
        .collect();
    u.ends_with(".trycloudflare.com").then_some(u)
}

fn update(sh: &Shared, f: impl FnOnce(&mut TunnelStatus)) {
    let run = sh.ext::<Run>();
    let s = {
        let mut s = run.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    sh.module_event(NAME, "status", &s);
}

/// Run `cloudflared` until stopped, restarting it after a pause that doubles up to a minute.
async fn supervise(
    sh: Shared,
    bin: PathBuf,
    cfg: TunnelCfg,
    token: Option<String>,
    mut stop: watch::Receiver<bool>,
) {
    let (dash, gw) = targets(&sh);
    let mut pause = Duration::from_secs(2);
    loop {
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg("tunnel").arg("--no-autoupdate");
        match cfg.mode {
            Mode::Token => {
                cmd.arg("run");
                cmd.env("TUNNEL_TOKEN", token.as_deref().unwrap_or(""));
            }
            Mode::Quick => {
                cmd.arg("--url").arg(if cfg.expose == Expose::Gateway {
                    &gw
                } else {
                    &dash
                });
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        update(&sh, |s| {
            s.state = "starting".into();
            s.url = None;
        });
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                update(&sh, |s| {
                    s.state = "restarting".into();
                    s.error = Some(format!("cannot run cloudflared: {e}"));
                });
                tokio::select! { _ = stop.changed() => return, () = tokio::time::sleep(pause) => {} }
                pause = (pause * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        if let Some(err) = child.stderr.take() {
            let sh2 = sh.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    if let Some(u) = quick_url(&l) {
                        update(&sh2, |s| s.url = Some(u));
                    }
                    if l.contains("Registered tunnel connection") {
                        update(&sh2, |s| {
                            s.state = "connected".into();
                            s.error = None;
                        });
                    }
                }
            });
        }
        tokio::select! {
            _ = stop.changed() => {
                let _ = child.kill().await;
                return;
            }
            r = child.wait() => {
                let why = match r {
                    Ok(st) => format!("cloudflared ended ({st})"),
                    Err(e) => format!("cloudflared ended: {e}"),
                };
                update(&sh, |s| {
                    s.state = "restarting".into();
                    s.restarts += 1;
                    s.error = Some(why);
                });
            }
        }
        tokio::select! { _ = stop.changed() => return, () = tokio::time::sleep(pause) => {} }
        pause = (pause * 2).min(Duration::from_secs(60));
    }
}

async fn up(sh: &Shared) -> Result<(), String> {
    let cfg: TunnelCfg = sh.cfg().section(NAME);
    let bin = find(&cfg).ok_or("cloudflared is not installed: install it from your distribution or from Cloudflare, or set [tunnel].cloudflared")?;
    let sh2 = sh.clone();
    let token = tokio::task::spawn_blocking(move || sh2.secrets.get(SECRET))
        .await
        .map_err(|e| e.to_string())?;
    let (dash, gw) = targets(sh);
    let run = sh.ext::<Run>();
    *run.status.lock().unwrap() = TunnelStatus {
        state: "starting".into(),
        mode: cfg.mode,
        targets: vec![dash, gw],
        token_set: token.is_some(),
        ..Default::default()
    };
    // The module runs (so the token route answers) but waits for a token; storing one starts the tunnel.
    if cfg.mode == Mode::Token && token.is_none() {
        update(sh, |s| {
            s.state = "stopped".into();
            s.error = Some(
                "no tunnel token: set one with PUT /api/v1/tunnel/token, or use mode = \"quick\""
                    .into(),
            );
        });
        return Ok(());
    }
    let (tx, rx) = watch::channel(false);
    *run.stop.lock().unwrap() = Some(tx);
    tokio::spawn(supervise(sh.clone(), bin, cfg, token, rx));
    Ok(())
}

async fn down(sh: &Shared) {
    let run = sh.ext::<Run>();
    let tx = run.stop.lock().unwrap().take();
    if let Some(tx) = tx {
        let _ = tx.send(true);
        // Let the supervisor kill and reap the child.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    update(sh, |s| {
        s.state = "stopped".into();
        s.url = None;
    });
}

#[derive(Default)]
pub struct Tunnel;

impl Module for Tunnel {
    fn name(&self) -> &'static str {
        NAME
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("/status", status)
            .post("/restart", restart)
            .put("/token", set_token)
            .delete("/token", clear_token)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(up(sh))
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(down(sh))
    }
}

fn admin(c: &Caller) -> Result<(), ApiErr> {
    if c.admin() {
        Ok(())
    } else {
        Err(ApiErr::forbidden(
            "only an administrator manages remote access",
        ))
    }
}

async fn status(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
) -> ApiResult<TunnelStatus> {
    admin(&c)?;
    Ok(Json(sh.ext::<Run>().status.lock().unwrap().clone()))
}

async fn restart(State(sh): State<Shared>, Extension(c): Extension<Caller>) -> ApiResult<Value> {
    admin(&c)?;
    down(&sh).await;
    up(&sh).await?;
    ok()
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

/// Store the token of a dashboard-made tunnel (the keyring only); takes effect on the next start or restart.
async fn set_token(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(b): Json<TokenBody>,
) -> ApiResult<Value> {
    admin(&c)?;
    let t = b.token.trim().to_string();
    // A tunnel token is base64 JSON; anything else (a pasted command line, a URL) is refused before it is stored.
    if t.len() < 32
        || t.len() > 4096
        || !t
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+/=-_".contains(&c))
    {
        return Err(ApiErr::bad(
            "that is not a tunnel token (copy only the token from the Cloudflare dashboard)",
        ));
    }
    let sh2 = sh.clone();
    tokio::task::spawn_blocking(move || sh2.secrets.set(SECRET, Some(&t)))
        .await
        .map_err(|e| e.to_string())??;
    // Start (or restart) with the new token.
    down(&sh).await;
    up(&sh).await?;
    ok()
}

async fn clear_token(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
) -> ApiResult<Value> {
    admin(&c)?;
    let sh2 = sh.clone();
    tokio::task::spawn_blocking(move || sh2.secrets.set(SECRET, None))
        .await
        .map_err(|e| e.to_string())??;
    sh.ext::<Run>().status.lock().unwrap().token_set = false;
    ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quick_address_is_found_in_cloudflared_output() {
        assert_eq!(
            quick_url("2026-11-28T10:00:00Z INF |  https://calm-river-1234.trycloudflare.com                    |").as_deref(),
            Some("https://calm-river-1234.trycloudflare.com")
        );
        assert_eq!(
            quick_url("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
        assert_eq!(
            quick_url("see https://developers.cloudflare.com/docs"),
            None
        );
    }
}
