//! "Update available" for every install kind (docs/dev/UPDATER_PLAN.md). This module checks and tells; `update_install` downloads and swaps (per-user installs only).
//! One small JSON manifest, at most one GET per 24 h, nothing sent but the version in the User-Agent (and, unavoidably, the IP).
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub const MANIFEST_URL: &str = "https://nocturne.ifsvivek.in/update/stable.json";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const EVERY_SECS: u64 = 24 * 3600;
const MAX_BYTES: usize = 64 * 1024;

/// `update/stable.json`. Unknown fields are ignored so the format can grow; `platforms` is read by phase 2 only.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub date: Option<String>,
    pub notes_url: String,
    #[serde(default)]
    pub platforms: BTreeMap<String, Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub signature: String,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    if bytes.len() > MAX_BYTES {
        return Err("manifest too large".into());
    }
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    // A manifest we cannot compare or show is as good as none; the link is opened in a browser, so https only.
    Version::parse(&m.version).ok_or("bad version")?;
    if !m.notes_url.starts_with("https://") {
        return Err("notes_url must be https".into());
    }
    Ok(m)
}

/// Semver precedence: numeric core, then a pre-release sorts below its release (`0.5.0-rc.1 < 0.5.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('v');
        let s = s.split('+').next()?; // build metadata never counts
        let (core, pre) = s.split_once('-').unwrap_or((s, ""));
        let mut it = core.split('.');
        let mut n = [0u64; 3];
        for slot in &mut n {
            *slot = it.next()?.parse().ok()?;
        }
        if it.next().is_some() {
            return None;
        }
        Some(Self {
            core: n,
            pre: pre
                .split('.')
                .filter(|p| !p.is_empty())
                .map(Into::into)
                .collect(),
        })
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        self.core
            .cmp(&o.core)
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), _) => Ordering::Less,
                            (_, Ok(_)) => Ordering::Greater,
                            _ => a.cmp(b),
                        };
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            })
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Pacman,
    Apt,
    Dnf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// The binary lives in `/usr` and the distro's package database exists: the package manager owns it.
    SystemPackage(Manager),
    /// Anything else a user installed (tarball, Windows installer, macOS app). Self-updates when its directory is writable.
    PerUser,
    /// A cargo build from the source tree: never checks.
    Dev,
}

/// Cheap and reliable enough: a path test plus the presence of one database file, no subprocess (`pacman -Qo` and
/// friends cost a fork and a lock, and can hang on a busy database). The tarball installs to `/usr/local` by default,
/// which this does not match; `PREFIX=/usr` over a package manager's files is unsupported anyway.
pub fn detect(exe: &Path, has: &dyn Fn(&str) -> bool) -> InstallKind {
    if exe.components().any(|c| c.as_os_str() == "target") {
        return InstallKind::Dev;
    }
    if exe.starts_with("/usr") && !exe.starts_with("/usr/local") {
        let m = if has("/var/lib/pacman/local") {
            Manager::Pacman
        } else if has("/var/lib/dpkg/status") {
            Manager::Apt
        } else if has("/var/lib/rpm") || has("/usr/lib/sysimage/rpm") {
            Manager::Dnf
        } else {
            return InstallKind::PerUser;
        };
        return InstallKind::SystemPackage(m);
    }
    InstallKind::PerUser
}

pub fn detect_here() -> InstallKind {
    if cfg!(debug_assertions) {
        return InstallKind::Dev;
    }
    std::env::current_exe().map_or(InstallKind::Dev, |e| detect(&e, &|p| Path::new(p).exists()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run in a terminal; shown copyable.
    Command(&'static str),
    /// Open the release page (per-user installs that cannot update themselves, or no artifact for this platform).
    Download(String),
    /// Per-user install that can replace itself: download, verify, swap, restart.
    Install(Artifact),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    UpToDate,
    Available {
        version: String,
        notes_url: String,
        action: Action,
    },
}

/// `self_update`: the install directory is writable by the user (`update_install::can_self_update`).
pub fn decide(current: &str, m: &Manifest, kind: InstallKind, self_update: bool) -> UpdateState {
    let (Some(cur), Some(new)) = (Version::parse(current), Version::parse(&m.version)) else {
        return UpdateState::UpToDate;
    };
    if kind == InstallKind::Dev || new <= cur {
        return UpdateState::UpToDate;
    }
    let action = match kind {
        InstallKind::SystemPackage(Manager::Pacman) => Action::Command("sudo pacman -Syu"),
        InstallKind::SystemPackage(Manager::Apt) => {
            Action::Command("sudo apt update && sudo apt upgrade")
        }
        InstallKind::SystemPackage(Manager::Dnf) => Action::Command("sudo dnf upgrade"),
        InstallKind::PerUser if self_update => m
            .platforms
            .get(&crate::update_install::platform_key())
            .map_or_else(
                || Action::Download(m.notes_url.clone()),
                |a| Action::Install(a.clone()),
            ),
        _ => Action::Download(m.notes_url.clone()),
    };
    UpdateState::Available {
        version: m.version.clone(),
        notes_url: m.notes_url.clone(),
        action,
    }
}

/// True when the last check (unix seconds) is missing, unreadable, in the future, or a day old.
pub fn due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|l| l > now || now - l >= EVERY_SECS)
}

fn stamp_path() -> Option<std::path::PathBuf> {
    crate::paths::data_dir().map(|d| d.join("update-check"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

async fn fetch(url: &str) -> Result<Manifest, String> {
    let mut r = reqwest::Client::builder()
        .user_agent(format!("NOCTURNE/{CURRENT}"))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    while let Some(c) = r.chunk().await.map_err(|e| e.to_string())? {
        body.extend_from_slice(&c);
        if body.len() > MAX_BYTES {
            return Err("manifest too large".into());
        }
    }
    parse_manifest(&body)
}

/// The startup check: `None` when it is not due, not applicable (dev build) or failed (calm: the next launch tries
/// again, a failure does not stamp). Run it through `bridge::spawn`.
pub async fn check() -> Option<UpdateState> {
    let kind = detect_here();
    if kind == InstallKind::Dev {
        return None;
    }
    let path = stamp_path()?;
    let last = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.trim().parse().ok());
    if !due(last, now_secs()) {
        return None;
    }
    let m = fetch(MANIFEST_URL).await.ok()?;
    let _ = std::fs::create_dir_all(path.parent()?);
    let _ = std::fs::write(&path, now_secs().to_string());
    let can =
        std::env::current_exe().is_ok_and(|e| crate::update_install::can_self_update(kind, &e));
    Some(decide(CURRENT, &m, kind, can))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r#"{"version":"0.6.0","date":"2026-12-01","notes_url":"https://example.org/r/v0.6.0",
        "platforms":{"linux-x86_64":{"url":"https://example.org/a.tar.gz","size":10,"sha256":"ab","signature":"cd"}},"extra":1}"#;

    fn m(v: &str) -> Manifest {
        Manifest {
            version: v.into(),
            date: None,
            notes_url: "https://example.org/r".into(),
            platforms: BTreeMap::new(),
        }
    }

    #[test]
    fn manifest_parsing() {
        let p = parse_manifest(OK.as_bytes()).unwrap();
        assert_eq!(p.version, "0.6.0");
        assert_eq!(p.platforms["linux-x86_64"].size, 10);
        assert!(parse_manifest(br#"{"notes_url":"https://x"}"#).is_err());
        assert!(parse_manifest(br#"{"version":"0.6.0"}"#).is_err());
        assert!(parse_manifest(br#"{"version":"x","notes_url":"https://x"}"#).is_err());
        assert!(parse_manifest(br#"{"version":"1.0.0","notes_url":"file:///x"}"#).is_err());
        assert!(parse_manifest(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(parse_manifest(b"not json").is_err());
    }

    #[test]
    fn versions_order_like_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("0.5.0") > v("0.5.0-rc.1"));
        assert!(v("0.5.0-rc.2") > v("0.5.0-rc.1"));
        assert!(v("0.5.0-rc.10") > v("0.5.0-rc.9"));
        assert!(v("0.5.0-rc.1") > v("0.5.0-beta.7"));
        assert!(v("0.5.0-1") < v("0.5.0-alpha"));
        assert!(v("0.5.0-rc") < v("0.5.0-rc.1"));
        assert_eq!(v("v0.5.0+build5"), v("0.5.0"));
        assert!(Version::parse("1.2").is_none() && Version::parse("1.2.3.4").is_none());
    }

    #[test]
    fn decision_per_install_kind() {
        let up = |k| decide("0.5.0", &m("0.6.0"), k, false);
        assert_eq!(
            up(InstallKind::SystemPackage(Manager::Pacman)),
            UpdateState::Available {
                version: "0.6.0".into(),
                notes_url: "https://example.org/r".into(),
                action: Action::Command("sudo pacman -Syu")
            }
        );
        for (k, cmd) in [
            (
                InstallKind::SystemPackage(Manager::Apt),
                "sudo apt update && sudo apt upgrade",
            ),
            (InstallKind::SystemPackage(Manager::Dnf), "sudo dnf upgrade"),
        ] {
            assert!(
                matches!(up(k), UpdateState::Available { action: Action::Command(c), .. } if c == cmd)
            );
        }
        // not writable: Download
        assert!(matches!(
            up(InstallKind::PerUser),
            UpdateState::Available {
                action: Action::Download(_),
                ..
            }
        ));
        assert_eq!(up(InstallKind::Dev), UpdateState::UpToDate);
        let same = decide("0.6.0", &m("0.6.0"), InstallKind::PerUser, true);
        assert_eq!(same, UpdateState::UpToDate);
        let older = decide("0.7.0", &m("0.6.0"), InstallKind::PerUser, true);
        assert_eq!(older, UpdateState::UpToDate);
        let rc = decide("0.6.0-rc.1", &m("0.6.0"), InstallKind::PerUser, true);
        assert!(matches!(rc, UpdateState::Available { .. }));
    }

    #[test]
    fn only_per_user_gets_the_in_app_install() {
        let art = Artifact {
            url: "https://example.org/a".into(),
            size: 1,
            sha256: "00".into(),
            signature: "x".into(),
        };
        let mut mf = m("0.6.0");
        mf.platforms
            .insert(crate::update_install::platform_key(), art.clone());
        let act = |k| match decide("0.5.0", &mf, k, true) {
            UpdateState::Available { action, .. } => Some(action),
            UpdateState::UpToDate => None,
        };
        assert_eq!(act(InstallKind::PerUser), Some(Action::Install(art)));
        for k in [
            InstallKind::SystemPackage(Manager::Pacman),
            InstallKind::SystemPackage(Manager::Apt),
            InstallKind::SystemPackage(Manager::Dnf),
        ] {
            assert!(matches!(act(k), Some(Action::Command(_))));
        }
        assert_eq!(act(InstallKind::Dev), None);
        // no entry for this platform: Download
        mf.platforms.clear();
        assert!(matches!(
            decide("0.5.0", &mf, InstallKind::PerUser, true),
            UpdateState::Available {
                action: Action::Download(_),
                ..
            }
        ));
    }

    #[test]
    fn detection() {
        let only = |p: &'static str| move |q: &str| q == p;
        let d = |p, f: &dyn Fn(&str) -> bool| detect(Path::new(p), f);
        let pac = only("/var/lib/pacman/local");
        assert_eq!(
            d("/usr/bin/nocturne", &pac),
            InstallKind::SystemPackage(Manager::Pacman)
        );
        assert_eq!(
            d("/usr/bin/nocturne", &only("/var/lib/dpkg/status")),
            InstallKind::SystemPackage(Manager::Apt)
        );
        assert_eq!(
            d("/usr/bin/nocturne", &only("/var/lib/rpm")),
            InstallKind::SystemPackage(Manager::Dnf)
        );
        assert_eq!(d("/usr/local/bin/nocturne", &pac), InstallKind::PerUser);
        assert_eq!(d("/usr/bin/nocturne", &|_| false), InstallKind::PerUser);
        assert_eq!(
            d("/home/u/.local/bin/nocturne", &|_| false),
            InstallKind::PerUser
        );
        let dev = d("/home/u/src/target/release/nocturne", &|_| false);
        assert_eq!(dev, InstallKind::Dev);
    }

    #[test]
    fn throttle_is_24_hours() {
        assert!(due(None, 1000));
        assert!(!due(Some(1000), 1000 + EVERY_SECS - 1));
        assert!(due(Some(1000), 1000 + EVERY_SECS));
        assert!(due(Some(5000), 1000), "clock went back");
    }
}
