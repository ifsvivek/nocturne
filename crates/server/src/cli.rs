//! The command line: `nocturne-server <command>`. Service commands drive `systemctl --user`; the rest talk to the running server over its unix socket as the local admin (same uid, no password). Blocking, std only.
use nocturne_core::server::ServerConfig;
use nocturne_core::user_unit::{self, SERVER};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "usage: nocturne-server [command]

  serve                     run the server (default; what the service runs)
  install [--linger]        install and start the systemd user service
  uninstall                 stop and remove the service (data and settings stay)
  start | stop | restart    control the service
  status                    modules, counters and jobs of the running server
  logs [-f]                 the service's log
  update                    check for a newer version
  open                      print and open the dashboard address (with the setup code until setup is done)
  config get [key]          show the configuration, or one dotted key (http.port)
  config set <key> <value>  change one key (JSON value or plain text) and apply it
  apikey create <name> | list | revoke <id>
  user add <name>           create the admin (first-run setup); asks for the password
  user password <name>      set a user's password; asks for it";

/// What went wrong, for the user.
pub type Res<T> = Result<T, String>;

/// One HTTP/1.1 request over the unix socket. Returns status and JSON body.
pub fn call(sock: &Path, method: &str, path: &str, body: Option<&Value>) -> Res<(u16, Value)> {
    let mut s = std::os::unix::net::UnixStream::connect(sock).map_err(|e| {
        format!(
            "the server is not running ({}: {e}); start it with `nocturne-server start`",
            sock.display()
        )
    })?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(120)))
        .ok();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut r = std::io::BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let code: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("bad answer from the server")?;
    let (mut chunked, mut len) = (false, None);
    loop {
        line.clear();
        r.read_line(&mut line).map_err(|e| e.to_string())?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let lower = l.to_ascii_lowercase();
        if lower == "transfer-encoding: chunked" {
            chunked = true;
        } else if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut out = Vec::new();
    if chunked {
        loop {
            line.clear();
            r.read_line(&mut line).map_err(|e| e.to_string())?;
            let n = usize::from_str_radix(line.trim(), 16).map_err(|_| "bad chunk")?;
            if n == 0 {
                break;
            }
            let mut buf = vec![0; n + 2];
            r.read_exact(&mut buf).map_err(|e| e.to_string())?;
            out.extend_from_slice(&buf[..n]);
        }
    } else if let Some(n) = len {
        out.resize(n, 0);
        r.read_exact(&mut out).map_err(|e| e.to_string())?;
    } else {
        r.read_to_end(&mut out).map_err(|e| e.to_string())?;
    }
    Ok((code, serde_json::from_slice(&out).unwrap_or(Value::Null)))
}

fn api(sock: &Path, method: &str, path: &str, body: Option<Value>) -> Res<Value> {
    let (code, v) = call(sock, method, &format!("/api/v1{path}"), body.as_ref())?;
    if (200..300).contains(&code) {
        Ok(v)
    } else {
        Err(v["error"]
            .as_str()
            .map_or_else(|| format!("HTTP {code}"), String::from))
    }
}

/// The dotted key's value (`http.port`).
pub fn get_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(v, |v, k| v.get(k))
}

/// Set a dotted key; the value is JSON if it parses, else a string.
pub fn set_key(v: &mut Value, key: &str, raw: &str) -> Res<()> {
    let new = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into()));
    let mut cur = v;
    let parts: Vec<&str> = key.split('.').collect();
    for (i, k) in parts.iter().enumerate() {
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| format!("{key}: not a section"))?;
        if i + 1 == parts.len() {
            obj.insert(k.to_string(), new);
            return Ok(());
        }
        cur = obj.entry(k.to_string()).or_insert_with(|| json!({}));
    }
    Err("empty key".into())
}

fn dashboard_url(cfg: &ServerConfig) -> String {
    let host = if cfg.http.binding == nocturne_core::gateway::Binding::Local {
        "127.0.0.1"
    } else {
        "localhost"
    };
    nocturne_core::gateway::address_url(cfg.http.binding, host, cfg.http.port) + "/"
}

fn read_password(prompt: &str) -> Res<String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    #[cfg(unix)]
    // SAFETY: termios calls on stdin with a struct we own; echo is restored below.
    let saved = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &mut t) == 0 {
            let old = t;
            t.c_lflag &= !libc::ECHO;
            libc::tcsetattr(0, libc::TCSANOW, &t);
            Some(old)
        } else {
            None
        }
    };
    let mut pw = String::new();
    let r = std::io::stdin().read_line(&mut pw);
    #[cfg(unix)]
    if let Some(old) = saved {
        // SAFETY: restoring the settings read above.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &old) };
    }
    eprintln!();
    r.map_err(|e| e.to_string())?;
    Ok(pw.trim_end_matches(['\n', '\r']).to_string())
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// The old 0.7.0 gateway service is removed once the gateway module has imported its data (it leaves this marker).
pub fn gateway_marker(data: &Path) -> PathBuf {
    data.join("gateway-imported")
}

fn install(linger: bool, paths: &crate::state::Paths) -> Res<()> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    user_unit::enable(&SERVER, &exe, &[]).map_err(|e| {
        format!(
            "installing the service: {e}. Without it, run `nocturne-server serve` in a terminal or from your init system."
        )
    })?;
    println!("Installed and started {}.", SERVER.unit);
    if linger {
        let user = std::env::var("USER").unwrap_or_default();
        match std::process::Command::new("loginctl")
            .args(["enable-linger", &user])
            .status()
        {
            Ok(s) if s.success() => println!(
                "Lingering is on: the server starts at boot. Until you log in, the keyring is locked and stored keys (indexers, servers) cannot be read."
            ),
            _ => eprintln!("Could not turn on lingering (loginctl enable-linger)."),
        }
    }
    let old = user_unit::unit_dir().map(|d| d.join(user_unit::GATEWAY.unit));
    if old.is_some_and(|p| p.exists()) {
        if gateway_marker(&paths.data).exists() {
            user_unit::disable(&user_unit::GATEWAY).map_err(|e| e.to_string())?;
            println!(
                "Removed the old {} (its data now lives in the server).",
                user_unit::GATEWAY.unit
            );
        } else {
            println!(
                "The 0.7.0 {} stays until the server's gateway module has imported its data; enable that module, then run `nocturne-server install` again.",
                user_unit::GATEWAY.unit
            );
        }
    }
    let cfg = ServerConfig::load_from(&paths.config).unwrap_or_default();
    let url = dashboard_url(&cfg);
    println!("Dashboard: {url}");
    // The server writes the code when it starts; give it a moment.
    for _ in 0..20 {
        if crate::auth::setup_code(paths).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    match setup_link(&url, paths) {
        Some(l) => println!("First-run setup: {l} (or `nocturne-server user add <name>`)"),
        None => println!(
            "First-run setup: `nocturne-server open` once the server runs, or `nocturne-server user add <name>`"
        ),
    }
    Ok(())
}

/// The dashboard's setup page with the one-time code, while setup is waiting.
fn setup_link(url: &str, paths: &crate::state::Paths) -> Option<String> {
    crate::auth::setup_code(paths).map(|c| format!("{url}setup?code={c}"))
}

// ponytail: reports a newer version only; installing stays with the package manager or the app's updater until the server ships its own archives.
fn update() -> Res<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let m = rt.block_on(nocturne_core::update::fetch(
        nocturne_core::update::MANIFEST_URL,
    ))?;
    let cur = nocturne_core::update::CURRENT;
    if m.version == cur {
        println!("nocturne-server {cur} is the latest version.");
    } else {
        println!(
            "Version {} is available (this is {cur}). Update NOCTURNE through your package manager or the app, then run `nocturne-server restart`.",
            m.version
        );
    }
    Ok(())
}

/// Run a CLI command. `None`: not a CLI command (serve).
pub fn run(args: &[String]) -> Option<Res<()>> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let paths = match crate::state::Paths::system() {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    let sock = paths.socket().unwrap_or_default();
    let ctl = |what: &str| -> Res<()> {
        user_unit::systemctl(&[what, SERVER.unit]).map_err(|e| format!("systemctl {what}: {e}"))
    };
    Some(match a.as_slice() {
        [] | ["serve"] => return None,
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        ["--version" | "version"] => {
            println!("nocturne-server {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["install", rest @ ..] => install(rest.contains(&"--linger"), &paths),
        ["uninstall"] => user_unit::disable(&SERVER)
            .map_err(|e| e.to_string())
            .map(|()| println!("Removed {}. Data and settings stay.", SERVER.unit)),
        ["start"] => ctl("start"),
        ["stop"] => ctl("stop"),
        ["restart"] => ctl("restart"),
        ["logs", rest @ ..] => {
            let mut c = std::process::Command::new("journalctl");
            c.args(["--user", "-u", SERVER.unit, "-n", "200", "--no-pager"]);
            if rest.contains(&"-f") {
                c.arg("-f");
            }
            c.status()
                .map(|_| ())
                .map_err(|e| format!("journalctl: {e}"))
        }
        ["status"] => api(&sock, "GET", "/status", None).map(|v| print(&v)),
        ["open"] => {
            let url = dashboard_url(&ServerConfig::load_from(&paths.config).unwrap_or_default());
            let url = setup_link(&url, &paths).unwrap_or(url);
            println!("{url}");
            let _ = std::process::Command::new("xdg-open").arg(&url).status();
            Ok(())
        }
        ["update"] => update(),
        ["config", "get"] => api(&sock, "GET", "/config", None).map(|v| print(&v)),
        ["config", "get", key] => api(&sock, "GET", "/config", None).and_then(|v| {
            get_key(&v, key)
                .map(print)
                .ok_or_else(|| format!("no key {key}"))
        }),
        ["config", "set", key, value] => api(&sock, "GET", "/config", None).and_then(|mut v| {
            set_key(&mut v, key, value)?;
            api(&sock, "PUT", "/config", Some(v))
                .map(|v| print(get_key(&v, key).unwrap_or(&Value::Null)))
        }),
        ["apikey", "create", name] => api(&sock, "POST", "/apikeys", Some(json!({"name": name})))
            .map(|v| {
                println!("{}", v["key"].as_str().unwrap_or_default());
                eprintln!(
                    "Key {} created; it is shown only now.",
                    v["id"].as_str().unwrap_or_default()
                );
            }),
        ["apikey", "list"] => api(&sock, "GET", "/apikeys", None).map(|v| print(&v)),
        ["apikey", "revoke", id] => {
            api(&sock, "DELETE", &format!("/apikeys/{id}"), None).map(|_| println!("Revoked."))
        }
        ["user", "add", name] => read_password("Password (8 or more characters): ")
            .and_then(|pw| {
                api(
                    &sock,
                    "POST",
                    "/setup",
                    Some(json!({"username": name, "password": pw})),
                )
            })
            .map(|_| println!("Admin {name} created.")),
        ["user", "password", name] => read_password("New password: ")
            .and_then(|pw| {
                api(
                    &sock,
                    "PUT",
                    "/auth/password",
                    Some(json!({"user": name, "new": pw})),
                )
            })
            .map(|_| println!("Password changed; every session was signed out.")),
        _ => Err(format!("unknown command\n\n{USAGE}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_keys() {
        let mut v = json!({"http": {"port": 8100}, "library": {"enabled": false}});
        assert_eq!(get_key(&v, "http.port"), Some(&json!(8100)));
        set_key(&mut v, "library.enabled", "true").unwrap();
        set_key(&mut v, "http.cert_path", "/etc/c.pem").unwrap();
        set_key(&mut v, "gateway.enabled", "true").unwrap();
        assert_eq!(v["library"]["enabled"], true);
        assert_eq!(v["http"]["cert_path"], "/etc/c.pem");
        assert_eq!(v["gateway"], json!({"enabled": true}));
        assert!(set_key(&mut v, "http.port.x", "1").is_err());
    }
}
