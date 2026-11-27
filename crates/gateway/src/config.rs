//! Where the gateway keeps things, what it reads from the app's settings, and the private-file helpers.
use nocturne_core::gateway::GatewaySettings;
use std::io;
use std::path::{Path, PathBuf};

/// Directories and files. `system()` for the real process; tests build their own under a temp dir.
#[derive(Clone, Debug)]
pub struct Paths {
    /// `$XDG_DATA_HOME/nocturne/gateway/`: ids.json, tokens.json, users.toml, displayprefs.json (all 0600 in a 0700 dir).
    pub data: PathBuf,
    /// `$XDG_RUNTIME_DIR/nocturne/`: gateway.sock and gateway.lock. None: no control socket, no instance lock.
    pub runtime: Option<PathBuf>,
    /// `settings.toml` (the `[gateway]` table plus the write switches).
    pub settings: Option<PathBuf>,
    /// `sources.toml`.
    pub sources: Option<PathBuf>,
}

impl Paths {
    pub fn system() -> Self {
        use nocturne_session::paths;
        Self {
            data: paths::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("gateway"),
            runtime: paths::runtime_dir().map(|d| d.join("nocturne")),
            settings: nocturne_core::settings::Settings::default_path(),
            sources: nocturne_core::profile::Sources::default_path(),
        }
    }

    pub fn socket(&self) -> Option<PathBuf> {
        self.runtime.as_ref().map(|d| d.join("gateway.sock"))
    }

    pub fn lock(&self) -> Option<PathBuf> {
        self.runtime.as_ref().map(|d| d.join("gateway.lock"))
    }
}

/// A snapshot of the settings this process acts on; `State::reload` swaps it.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub gw: GatewaySettings,
    /// Master switch for everything that changes a server (docs/WRITES.md). The gateway forwards no write without it.
    pub allow_server_writes: bool,
    pub sync_progress: bool,
    pub sync_watched: bool,
}

impl Config {
    pub fn load(paths: &Paths) -> Self {
        let Some(p) = &paths.settings else {
            return Self::default();
        };
        let s = nocturne_core::settings::Settings::load_from(p);
        Self {
            gw: GatewaySettings::load_from(p),
            allow_server_writes: s.allow_server_writes,
            sync_progress: s.sync_progress,
            sync_watched: s.sync_watched,
        }
    }

    /// Progress reports may be forwarded to a server (WRITES.md: needs both switches).
    pub fn forward_progress(&self) -> bool {
        self.allow_server_writes && self.sync_progress
    }
}

/// Create `dir` (and parents) and make it 0700.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Atomic write (temp file + rename) with mode 0600; the parent becomes a private dir.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    private_dir(dir)?;
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn private_files_are_0600_in_0700_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("gw").join("tokens.json");
        write_private(&p, b"one").unwrap();
        write_private(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(p.parent().unwrap()), 0o700);
    }

    #[test]
    fn progress_forwarding_needs_both_switches() {
        let mut c = Config::default();
        assert!(!c.forward_progress());
        c.sync_progress = true;
        assert!(!c.forward_progress());
        c.allow_server_writes = true;
        assert!(c.forward_progress());
    }
}
