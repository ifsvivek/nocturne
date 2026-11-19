//! Where NOCTURNE keeps its files, per OS. Linux follows XDG (unchanged); macOS uses
//! `~/Library/...`; Windows `%APPDATA%` (config) and `%LOCALAPPDATA%` (data, cache).
//! Plain std env lookups, no `dirs` crate. The OS is a parameter of the pure [`resolve`] so every
//! OS's rules are unit-tested on any host.
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub const CURRENT: Os = if cfg!(target_os = "windows") {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Config,
    Data,
    Cache,
}

fn home_var(os: Os) -> &'static str {
    if os == Os::Windows {
        "USERPROFILE"
    } else {
        "HOME"
    }
}

/// `<base>/nocturne` for `kind`, looking variables up with `env`.
pub fn resolve(os: Os, kind: Kind, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = || var(home_var(os));
    let base = match (os, kind) {
        (Os::Linux, k) => {
            let (xdg, fallback) = match k {
                Kind::Config => ("XDG_CONFIG_HOME", ".config"),
                Kind::Data => ("XDG_DATA_HOME", ".local/share"),
                Kind::Cache => ("XDG_CACHE_HOME", ".cache"),
            };
            var(xdg)
                .filter(|p| p.is_absolute()) // the XDG spec says to ignore relative values
                .or_else(|| home().map(|h| h.join(fallback)))?
        }
        (Os::MacOs, Kind::Cache) => home()?.join("Library/Caches"),
        (Os::MacOs, _) => home()?.join("Library/Application Support"),
        (Os::Windows, Kind::Config) => var("APPDATA")?,
        (Os::Windows, _) => var("LOCALAPPDATA")?,
    };
    Some(base.join("nocturne"))
}

fn here(kind: Kind) -> Option<PathBuf> {
    resolve(Os::CURRENT, kind, &|k| std::env::var_os(k))
}

pub fn config_dir() -> Option<PathBuf> {
    here(Kind::Config)
}

pub fn data_dir() -> Option<PathBuf> {
    here(Kind::Data)
}

pub fn cache_dir() -> Option<PathBuf> {
    here(Kind::Cache)
}

/// The user's home directory.
pub fn home() -> Option<PathBuf> {
    std::env::var_os(home_var(Os::CURRENT))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Per-user runtime directory for sockets and the like: `$XDG_RUNTIME_DIR` (Linux), `$TMPDIR`
/// (macOS). `None` on Windows, which uses named pipes instead.
pub fn runtime_dir() -> Option<PathBuf> {
    let k = match Os::CURRENT {
        Os::Linux => "XDG_RUNTIME_DIR",
        Os::MacOs => "TMPDIR",
        Os::Windows => return None,
    };
    std::env::var_os(k)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(*v))
        }
    }

    // Unix paths: on Windows "/x" is not absolute, so the XDG rules this test checks do not apply there.
    #[cfg(unix)]
    #[test]
    fn linux_follows_xdg_with_home_fallbacks() {
        let r = |kind, p: &[(&str, &str)]| resolve(Os::Linux, kind, &env(p));
        let home = [("HOME", "/h")];
        assert_eq!(
            r(Kind::Config, &home).unwrap(),
            Path::new("/h/.config/nocturne")
        );
        assert_eq!(
            r(Kind::Data, &home).unwrap(),
            Path::new("/h/.local/share/nocturne")
        );
        assert_eq!(
            r(Kind::Cache, &home).unwrap(),
            Path::new("/h/.cache/nocturne")
        );
        let xdg = [
            ("HOME", "/h"),
            ("XDG_DATA_HOME", "/x"),
            ("XDG_CONFIG_HOME", "rel"),
        ];
        assert_eq!(r(Kind::Data, &xdg).unwrap(), Path::new("/x/nocturne"));
        // Relative and empty values are ignored.
        assert_eq!(
            r(Kind::Config, &xdg).unwrap(),
            Path::new("/h/.config/nocturne")
        );
        assert!(r(Kind::Config, &[("XDG_CONFIG_HOME", "")]).is_none());
    }

    #[test]
    fn macos_uses_library() {
        let e = env(&[("HOME", "/Users/u")]);
        let r = |k| resolve(Os::MacOs, k, &e).unwrap();
        assert_eq!(
            r(Kind::Config),
            Path::new("/Users/u/Library/Application Support/nocturne")
        );
        assert_eq!(
            r(Kind::Data),
            Path::new("/Users/u/Library/Application Support/nocturne")
        );
        assert_eq!(
            r(Kind::Cache),
            Path::new("/Users/u/Library/Caches/nocturne")
        );
    }

    #[test]
    fn windows_uses_appdata() {
        let e = env(&[("APPDATA", r"C:\R"), ("LOCALAPPDATA", r"C:\L")]);
        let r = |k| resolve(Os::Windows, k, &e).unwrap();
        assert_eq!(r(Kind::Config), Path::new(r"C:\R").join("nocturne"));
        assert_eq!(r(Kind::Data), Path::new(r"C:\L").join("nocturne"));
        assert_eq!(r(Kind::Cache), Path::new(r"C:\L").join("nocturne"));
        assert!(resolve(Os::Windows, Kind::Data, &env(&[])).is_none());
    }
}
