//! "Update now" for pacman, apt and dnf installs (docs/dev/UPDATER_PLAN.md, "What shipped in 0.5.2"): ask PackageKit over the system D-Bus to refresh and update only the `nocturne` package.
//! The desktop shows its own polkit prompt; NOCTURNE never runs sudo or a package manager and never sees a password.
//! The transaction logic (signals to outcome, error codes to messages, picking the package) is pure and tested; the D-Bus part is Linux only.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

/// PackageKit's `PkInfoEnum` value for an installed package in a `Package` signal.
const INFO_INSTALLED: u32 = 1;
/// `PkExitEnum`: success and the two cancellations.
const EXIT_SUCCESS: u32 = 1;
const EXIT_CANCELLED: u32 = 3;
const EXIT_CANCELLED_PRIORITY: u32 = 9;

/// What a transaction tells us, already parsed from the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// `Percentage` property change; 101 means unknown.
    Percentage(u32),
    Package {
        info: u32,
        id: String,
    },
    ErrorCode {
        code: u32,
        details: String,
    },
    Finished {
        exit: u32,
    },
}

/// One transaction: folds signals until `Finished`, then yields the packages it listed or the failure message.
#[derive(Debug, Default)]
pub struct Tx {
    pub percentage: Option<u32>,
    packages: Vec<(u32, String)>,
    error: Option<(u32, String)>,
}

impl Tx {
    pub fn feed(&mut self, s: Signal) -> Option<Result<Vec<(u32, String)>, String>> {
        match s {
            Signal::Percentage(p) => self.percentage = (p <= 100).then_some(p),
            Signal::Package { info, id } => self.packages.push((info, id)),
            Signal::ErrorCode { code, details } => self.error = Some((code, details)),
            Signal::Finished { exit } => {
                return Some(if let Some((code, details)) = self.error.take() {
                    Err(error_message(code, &details))
                } else if exit == EXIT_SUCCESS {
                    Ok(std::mem::take(&mut self.packages))
                } else if matches!(exit, EXIT_CANCELLED | EXIT_CANCELLED_PRIORITY) {
                    Err("The update was cancelled.".into())
                } else {
                    Err("The package manager could not finish the update.".into())
                });
            }
        }
        None
    }
}

/// PackageKit `PkErrorEnum` to a sentence a person can act on. Unknown codes fall back to PackageKit's own text.
pub fn error_message(code: u32, details: &str) -> String {
    let m = match code {
        2 => "There is no network connection.",
        3 => "PackageKit does not support this on your system.",
        5 | 30 | 31 | 50 | 51 => {
            "The package signature could not be verified, so nothing was installed."
        }
        8 | 49 => "The nocturne package was not found in your repositories.",
        10 | 43 => "The package could not be downloaded.",
        17 | 65 => "The update was cancelled.",
        18 => "The package cache is not ready: run the command below once, then try again.",
        19 | 33 | 37 => {
            "The NOCTURNE repository is not configured or not reachable on this system."
        }
        26 | 67 => "Another package manager is running. Try again when it has finished.",
        27 | 41 => {
            "There is nothing to update: the repository does not offer a newer NOCTURNE yet."
        }
        35 | 36 => "The update conflicts with other installed packages.",
        46 => "There is not enough disk space for the update.",
        48 => "Authentication was cancelled or refused.",
        60 => {
            "The update could not finish because the program is running. Restart NOCTURNE and use the command below."
        }
        _ => "",
    };
    if m.is_empty() {
        let d = details.trim();
        if d.is_empty() {
            format!("PackageKit reported error {code}.")
        } else {
            format!("PackageKit: {d}")
        }
    } else {
        m.into()
    }
}

/// A D-Bus level failure of a method call (a polkit refusal comes back this way) to a message.
pub fn bus_error_message(name: &str, detail: Option<&str>) -> String {
    if ["Refused", "NotAuthorized", "AuthFailed", "AccessDenied"]
        .iter()
        .any(|k| name.contains(k))
    {
        "Authentication was cancelled or refused.".into()
    } else if name.ends_with("ServiceUnknown") || name.ends_with("NameHasNoOwner") {
        "PackageKit is not available on this system.".into()
    } else {
        format!("PackageKit: {}", detail.unwrap_or(name))
    }
}

/// PackageKit is reachable when its name has an owner or the bus can start it on demand.
pub fn is_available(has_owner: bool, activatable: &[String]) -> bool {
    has_owner
        || activatable
            .iter()
            .any(|n| n == "org.freedesktop.PackageKit")
}

/// The package id of the not-yet-installed `nocturne` build at `version` from the `Resolve` result.
/// Ids look like `name;version;arch;data`; the version may carry an epoch (`1:`) and a release (`-1`).
pub fn pick_target(packages: &[(u32, String)], version: &str) -> Result<String, String> {
    let mut other_version = false;
    for (info, id) in packages {
        let mut f = id.split(';');
        let (Some("nocturne"), Some(v)) = (f.next(), f.next()) else {
            continue;
        };
        if *info == INFO_INSTALLED {
            other_version = true;
            continue;
        }
        let v = v.split_once(':').map_or(v, |(_, r)| r);
        if v.strip_prefix(version)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit() || c == '.'))
        {
            return Ok(id.clone());
        }
        other_version = true;
    }
    Err(if other_version {
        format!(
            "The repository does not offer NOCTURNE {version} yet. Try again later, or use the command below."
        )
    } else {
        error_message(8, "")
    })
}

/// After the package manager replaced the binary, `/proc` reports the running exe as `path (deleted)`; restart into the path itself.
pub fn restart_path(exe: &Path) -> PathBuf {
    let s = exe.to_string_lossy();
    s.strip_suffix(" (deleted)")
        .map_or_else(|| exe.to_path_buf(), PathBuf::from)
}

/// Debug builds only: `NOCTURNE_PK_MOCK=ok|refuse` fakes PackageKit for layout work and headless captures; nothing touches the bus.
fn mock() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("NOCTURNE_PK_MOCK").ok()
    } else {
        None
    }
}

/// Whether the Update now button can be offered. Asked once per launch, only for system-package installs.
pub async fn available() -> bool {
    if mock().is_some() {
        return true;
    }
    tokio::task::spawn_blocking(imp::available)
        .await
        .unwrap_or(false)
}

/// Refresh, resolve and update only `nocturne` to `version`. `progress` carries 0..=100 for the banner.
pub async fn update(version: String, progress: Arc<AtomicU64>) -> Result<(), String> {
    if let Some(m) = mock() {
        for p in (0..=100).step_by(10) {
            progress.store(p, std::sync::atomic::Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        return if m == "refuse" {
            Err(error_message(48, ""))
        } else {
            Ok(())
        };
    }
    tokio::task::spawn_blocking(move || imp::update(&version, &progress))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;
    use zbus::blocking::{Connection, MessageIterator, Proxy};
    use zbus::zvariant::{OwnedObjectPath, OwnedValue};
    use zbus::{MatchRule, message::Type};

    const DEST: &str = "org.freedesktop.PackageKit";
    /// `TRANSACTION_FLAG_ONLY_TRUSTED`: refuse unsigned packages.
    const ONLY_TRUSTED: u64 = 1 << 1;

    fn bus_err(e: zbus::Error) -> String {
        match &e {
            zbus::Error::MethodError(name, detail, _) => {
                bus_error_message(name.as_str(), detail.as_deref())
            }
            zbus::Error::InputOutput(_) => "PackageKit is not available on this system.".into(),
            _ => format!("PackageKit: {e}"),
        }
    }

    pub fn available() -> bool {
        let probe = || -> zbus::Result<bool> {
            let c = Connection::system()?;
            let d = zbus::blocking::fdo::DBusProxy::new(&c)?;
            let owner = d.name_has_owner(DEST.try_into()?)?;
            let act: Vec<String> = d
                .list_activatable_names()?
                .iter()
                .map(|n| n.to_string())
                .collect();
            Ok(is_available(owner, &act))
        };
        probe().unwrap_or(false)
    }

    fn parse(msg: &zbus::Message) -> Option<Signal> {
        let h = msg.header();
        let body = msg.body();
        match h.member()?.as_str() {
            "Package" => {
                let (info, id, _): (u32, String, String) = body.deserialize().ok()?;
                Some(Signal::Package { info, id })
            }
            "ErrorCode" => {
                let (code, details): (u32, String) = body.deserialize().ok()?;
                Some(Signal::ErrorCode { code, details })
            }
            "Finished" => {
                let (exit, _): (u32, u32) = body.deserialize().ok()?;
                Some(Signal::Finished { exit })
            }
            "PropertiesChanged" => {
                let (_, changed, _): (String, HashMap<String, OwnedValue>, Vec<String>) =
                    body.deserialize().ok()?;
                let p = u32::try_from(changed.get("Percentage")?).ok()?;
                Some(Signal::Percentage(p))
            }
            _ => None,
        }
    }

    /// One PackageKit transaction runs one method: create it, listen first, call, then follow the signals to `Finished`.
    /// The progress bar moves inside `lo..=hi` while this step runs.
    fn run_tx(
        c: &Connection,
        progress: &AtomicU64,
        (lo, hi): (u64, u64),
        call: impl FnOnce(&Proxy) -> zbus::Result<()>,
    ) -> Result<Vec<(u32, String)>, String> {
        let pk = Proxy::new(
            c,
            DEST,
            "/org/freedesktop/PackageKit",
            "org.freedesktop.PackageKit",
        )
        .map_err(bus_err)?;
        let path: OwnedObjectPath = pk.call("CreateTransaction", &()).map_err(bus_err)?;
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .path(path.as_ref())
            .map_err(bus_err)?
            .build();
        let signals = MessageIterator::for_match_rule(rule, c, None).map_err(bus_err)?;
        let tx = Proxy::new(
            c,
            DEST,
            path.as_ref(),
            "org.freedesktop.PackageKit.Transaction",
        )
        .map_err(bus_err)?;
        // `interactive` lets PackageKit show the polkit prompt through the desktop's agent.
        tx.call::<_, _, ()>("SetHints", &(vec!["interactive=true"],))
            .map_err(bus_err)?;
        call(&tx).map_err(bus_err)?;
        let mut t = Tx::default();
        for msg in signals {
            let Some(sig) = msg.ok().as_ref().and_then(parse) else {
                continue;
            };
            if let Some(done) = t.feed(sig) {
                progress.store(hi, Ordering::Relaxed);
                return done;
            }
            if let Some(p) = t.percentage {
                progress.store(lo + (hi - lo) * u64::from(p) / 100, Ordering::Relaxed);
            }
        }
        Err("PackageKit stopped answering.".into())
    }

    pub fn update(version: &str, progress: &AtomicU64) -> Result<(), String> {
        let c = Connection::system()
            .map_err(|_| "PackageKit is not available on this system.".to_string())?;
        run_tx(&c, progress, (0, 20), |t| {
            t.call::<_, _, ()>("RefreshCache", &(false,))
        })?;
        let found = run_tx(&c, progress, (20, 30), |t| {
            t.call::<_, _, ()>("Resolve", &(0u64, vec!["nocturne"]))
        })?;
        let id = pick_target(&found, version)?;
        run_tx(&c, progress, (30, 100), |t| {
            t.call::<_, _, ()>("UpdatePackages", &(ONLY_TRUSTED, vec![id.as_str()]))
        })?;
        Ok(())
    }

    /// Read-only: daemon state and a `Resolve` of `nocturne`. Used by the ignored live test.
    #[cfg(test)]
    pub fn query() -> Result<(String, usize), String> {
        let c = Connection::system().map_err(bus_err)?;
        let pk = Proxy::new(
            &c,
            DEST,
            "/org/freedesktop/PackageKit",
            "org.freedesktop.PackageKit",
        )
        .map_err(bus_err)?;
        let state: String = pk.call("GetDaemonState", &()).map_err(bus_err)?;
        let found = run_tx(&c, &AtomicU64::new(0), (0, 100), |t| {
            t.call::<_, _, ()>("Resolve", &(0u64, vec!["nocturne"]))
        })?;
        Ok((state, found.len()))
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    pub fn available() -> bool {
        false
    }
    pub fn update(_: &str, _: &AtomicU64) -> Result<(), String> {
        Err("PackageKit is not available on this system.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(signals: Vec<Signal>) -> Option<Result<Vec<(u32, String)>, String>> {
        let mut t = Tx::default();
        signals.into_iter().find_map(|s| t.feed(s))
    }

    #[test]
    fn success_collects_packages_and_ignores_unknown_percentage() {
        let mut t = Tx::default();
        assert!(t.feed(Signal::Percentage(101)).is_none());
        assert_eq!(t.percentage, None);
        t.feed(Signal::Percentage(40));
        assert_eq!(t.percentage, Some(40));
        t.feed(Signal::Package {
            info: 2,
            id: "nocturne;0.5.2-1;x86_64;repo".into(),
        });
        let r = t.feed(Signal::Finished { exit: 1 }).unwrap().unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn error_code_wins_over_exit_status() {
        let r = run(vec![
            Signal::ErrorCode {
                code: 48,
                details: "x".into(),
            },
            Signal::Finished { exit: 2 },
        ])
        .unwrap();
        assert_eq!(r.unwrap_err(), "Authentication was cancelled or refused.");
    }

    #[test]
    fn cancelled_and_failed_exits() {
        assert_eq!(
            run(vec![Signal::Finished { exit: 3 }])
                .unwrap()
                .unwrap_err(),
            "The update was cancelled."
        );
        assert!(
            run(vec![Signal::Finished { exit: 2 }])
                .unwrap()
                .unwrap_err()
                .contains("could not finish")
        );
        assert!(run(vec![Signal::Percentage(5)]).is_none());
    }

    #[test]
    fn error_messages_map_and_fall_back() {
        assert!(error_message(2, "").contains("network"));
        assert!(error_message(27, "").contains("nothing to update"));
        assert!(error_message(19, "").contains("repository"));
        assert_eq!(error_message(999, "boom"), "PackageKit: boom");
        assert_eq!(error_message(999, " "), "PackageKit reported error 999.");
    }

    #[test]
    fn bus_errors_map() {
        assert!(
            bus_error_message(
                "org.freedesktop.PackageKit.Transaction.RefusedByPolicy",
                None
            )
            .contains("refused")
        );
        assert!(
            bus_error_message("org.freedesktop.DBus.Error.ServiceUnknown", None)
                .contains("not available")
        );
        assert_eq!(bus_error_message("x.Other", Some("why")), "PackageKit: why");
    }

    #[test]
    fn availability_is_owner_or_activatable() {
        assert!(is_available(true, &[]));
        assert!(is_available(false, &["org.freedesktop.PackageKit".into()]));
        assert!(!is_available(false, &["org.other".into()]));
    }

    #[test]
    fn pick_target_takes_the_available_new_version() {
        let p = |i: u32, id: &str| (i, id.to_string());
        let list = [
            p(1, "nocturne;0.5.1-1;x86_64;installed"),
            p(2, "nocturne;0.5.2-1;x86_64;nocturne"),
            p(2, "nocturne-extra;0.5.2-1;x86_64;r"),
        ];
        assert_eq!(
            pick_target(&list, "0.5.2").unwrap(),
            "nocturne;0.5.2-1;x86_64;nocturne"
        );
        assert_eq!(
            pick_target(&[p(2, "nocturne;1:0.5.2-1;amd64;r")], "0.5.2").unwrap(),
            "nocturne;1:0.5.2-1;amd64;r"
        );
        // Only the installed version, or a different one: the repository is behind.
        assert!(
            pick_target(&list[..1], "0.5.2")
                .unwrap_err()
                .contains("does not offer")
        );
        assert!(
            pick_target(&[p(2, "nocturne;0.5.20-1;x;r")], "0.5.2")
                .unwrap_err()
                .contains("does not offer")
        );
        assert!(pick_target(&[], "0.5.2").unwrap_err().contains("not found"));
    }

    #[test]
    fn restart_path_drops_the_deleted_marker() {
        assert_eq!(
            restart_path(Path::new("/usr/bin/nocturne (deleted)")),
            Path::new("/usr/bin/nocturne")
        );
        assert_eq!(
            restart_path(Path::new("/usr/bin/nocturne")),
            Path::new("/usr/bin/nocturne")
        );
    }

    /// Read-only: needs a PackageKit on the system bus. `cargo test -p nocturne-core update_pk -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn live_queries_packagekit_without_updating() {
        assert!(imp::available(), "PackageKit is not on the system bus");
        let (state, n) = imp::query().unwrap();
        println!("daemon state ok ({} bytes), resolve found {n}", state.len());
    }
}
