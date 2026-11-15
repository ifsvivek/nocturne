//! Persist the login. The session (access token) goes ONLY into the system keyring (Secret
//! Service); there is no file fallback. If no keyring is available every call returns
//! [`Error::Unavailable`] and the app asks for a login each run.
//! The list of known server URLs and the last used one (no secrets) live in
//! `$XDG_CONFIG_HOME/nocturne/servers.toml`, written atomically.

use nocturne_jellyfin_user::Session;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Keyring service name. `NOCTURNE_KEYRING_SERVICE` overrides it so tests never touch real entries.
fn service() -> &'static str {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::env::var("NOCTURNE_KEYRING_SERVICE")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "nocturne".into())
    })
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No Secret Service (or it is locked/unreachable). Do not fall back to a file.
    #[error("no usable system keyring: {0}")]
    Unavailable(String),
    #[error("stored session is unreadable: {0}")]
    Corrupt(String),
    #[error("servers file: {0}")]
    File(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Secret storage seam: the system keyring in production, a map in unit tests.
trait Store {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

struct Keyring(&'static str);

impl Keyring {
    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(self.0, key).map_err(map_err)
    }
}

fn map_err(e: keyring::Error) -> Error {
    Error::Unavailable(e.to_string())
}

impl Store for Keyring {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?.set_password(value).map_err(map_err)
    }
    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

fn key(server_url: &str) -> &str {
    server_url.trim_end_matches('/')
}

fn save_in(s: &dyn Store, server_url: &str, session: &Session) -> Result<()> {
    let json = serde_json::to_string(session).expect("session serializes");
    s.set(key(server_url), &json)
}

fn load_from(s: &dyn Store, server_url: &str) -> Result<Option<Session>> {
    s.get(key(server_url))?
        .map(|j| serde_json::from_str(&j).map_err(|e| Error::Corrupt(e.to_string())))
        .transpose()
}

/// Store the login for `server_url` in the keyring.
pub fn save(server_url: &str, session: &Session) -> Result<()> {
    save_in(&Keyring(service()), server_url, session)
}

/// The stored login for `server_url`, if any.
pub fn load(server_url: &str) -> Result<Option<Session>> {
    load_from(&Keyring(service()), server_url)
}

/// Remove the stored login (no error if there is none).
pub fn delete(server_url: &str) -> Result<()> {
    Keyring(service()).delete(key(server_url))
}

/// Known server URLs and the last used one. URLs only, never tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Servers {
    pub servers: Vec<String>,
    pub last_used: Option<String>,
}

impl Servers {
    /// `$XDG_CONFIG_HOME/nocturne/servers.toml` (default `~/.config`).
    pub fn default_path() -> Result<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
            .ok_or_else(|| Error::File("neither XDG_CONFIG_HOME nor HOME is set".into()))?;
        Ok(base.join("nocturne").join("servers.toml"))
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::default_path()?)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::default_path()?)
    }

    /// Missing file means an empty list.
    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(t) => toml::from_str(&t).map_err(|e| Error::File(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::File(e.to_string())),
        }
    }

    /// Atomic: write a sibling temp file, then rename over the target.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let io = |e: std::io::Error| Error::File(e.to_string());
        std::fs::create_dir_all(path.parent().expect("path has a parent")).map_err(io)?;
        let tmp = path.with_extension("toml.tmp");
        let text = toml::to_string_pretty(self).map_err(|e| Error::File(e.to_string()))?;
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    /// Add `url` if new and make it the last used.
    pub fn remember(&mut self, url: &str) {
        let url = key(url).to_string();
        if !self.servers.contains(&url) {
            self.servers.push(url.clone());
        }
        self.last_used = Some(url);
    }

    /// Drop `url` from the list (and from last used).
    pub fn forget(&mut self, url: &str) {
        let url = key(url);
        self.servers.retain(|s| s != url);
        if self.last_used.as_deref() == Some(url) {
            self.last_used = self.servers.last().cloned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    #[derive(Default)]
    struct Mem(RefCell<HashMap<String, String>>);
    impl Store for Mem {
        fn get(&self, k: &str) -> Result<Option<String>> {
            Ok(self.0.borrow().get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> Result<()> {
            self.0.borrow_mut().insert(k.into(), v.into());
            Ok(())
        }
        fn delete(&self, k: &str) -> Result<()> {
            self.0.borrow_mut().remove(k);
            Ok(())
        }
    }

    fn session() -> Session {
        Session {
            access_token: "tok".into(),
            user_id: "u".into(),
            server_id: "s".into(),
            user_name: "alice".into(),
            is_admin: false,
        }
    }

    #[test]
    fn save_load_delete_per_server() {
        let m = Mem::default();
        assert!(load_from(&m, "http://a:1").unwrap().is_none());
        save_in(&m, "http://a:1/", &session()).unwrap();
        let s = load_from(&m, "http://a:1").unwrap().unwrap(); // trailing slash is normalised
        assert_eq!(
            (s.access_token.as_str(), s.user_name.as_str()),
            ("tok", "alice")
        );
        assert!(load_from(&m, "http://b:1").unwrap().is_none());
        m.delete("http://a:1").unwrap();
        assert!(load_from(&m, "http://a:1").unwrap().is_none());
    }

    #[test]
    fn corrupt_entry_is_typed() {
        let m = Mem::default();
        m.set("http://a:1", "not json").unwrap();
        assert!(matches!(
            load_from(&m, "http://a:1"),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn servers_toml_round_trip_has_no_tokens() {
        let dir = std::env::temp_dir().join(format!("nocturne-session-{}", std::process::id()));
        let path = dir.join("nocturne").join("servers.toml");
        assert_eq!(Servers::load_from(&path).unwrap(), Servers::default());
        let mut s = Servers::default();
        s.remember("http://a:1/");
        s.remember("http://b:2");
        s.remember("http://a:1");
        s.save_to(&path).unwrap();
        let back = Servers::load_from(&path).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.servers, ["http://a:1", "http://b:2"]);
        assert_eq!(back.last_used.as_deref(), Some("http://a:1"));
        assert!(!path.with_extension("toml.tmp").exists());
        let mut back = back;
        back.forget("http://a:1");
        assert_eq!(back.last_used.as_deref(), Some("http://b:2"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Real Secret Service under a unique throwaway service name; cleans up after itself.
    #[test]
    #[ignore = "needs a running Secret Service (gnome-keyring, kwallet, keepassxc)"]
    fn real_keyring_round_trip() {
        let k = Keyring(Box::leak(
            format!("nocturne-test-{}", std::process::id()).into_boxed_str(),
        ));
        let url = "http://127.0.0.1:8097";
        assert!(load_from(&k, url).unwrap().is_none());
        save_in(&k, url, &session()).unwrap();
        let back = load_from(&k, url).unwrap().unwrap();
        k.delete(url).unwrap();
        assert_eq!(back.user_name, "alice");
        assert!(load_from(&k, url).unwrap().is_none());
    }
}
