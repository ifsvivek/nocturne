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

fn account_key(server_url: &str, user_id: &str) -> String {
    format!("{}|{user_id}", key(server_url))
}

/// Remember a login for switching: its token goes into the keyring under the server and user, so
/// several accounts per server can be kept side by side. The matching `accounts.toml` entry
/// ([`Accounts`]) holds names only.
pub fn save_account(server_url: &str, session: &Session) -> Result<()> {
    save_account_in(&Keyring(service()), server_url, session)
}

fn save_account_in(s: &dyn Store, server_url: &str, session: &Session) -> Result<()> {
    let json = serde_json::to_string(session).expect("session serializes");
    s.set(&account_key(server_url, &session.user_id), &json)
}

/// The remembered login of `user_id` on `server_url`, if any.
pub fn load_account(server_url: &str, user_id: &str) -> Result<Option<Session>> {
    load_account_from(&Keyring(service()), server_url, user_id)
}

fn load_account_from(s: &dyn Store, server_url: &str, user_id: &str) -> Result<Option<Session>> {
    s.get(&account_key(server_url, user_id))?
        .map(|j| serde_json::from_str(&j).map_err(|e| Error::Corrupt(e.to_string())))
        .transpose()
}

/// Forget the remembered login of `user_id` on `server_url` (no error if there is none).
pub fn delete_account(server_url: &str, user_id: &str) -> Result<()> {
    Keyring(service()).delete(&account_key(server_url, user_id))
}

/// The optional extra HTTP header of `server_url` (`Name: value`; the value may be a secret, so it
/// lives in the keyring and never in a file).
pub fn save_header(server_url: &str, line: &str) -> Result<()> {
    Keyring(service()).set(&header_key(server_url), line)
}

pub fn load_header(server_url: &str) -> Result<Option<String>> {
    Keyring(service()).get(&header_key(server_url))
}

pub fn delete_header(server_url: &str) -> Result<()> {
    Keyring(service()).delete(&header_key(server_url))
}

fn header_key(server_url: &str) -> String {
    format!("{}|extra-header", key(server_url))
}

/// Make `session` the active login for `server_url` AND a remembered account (keyring writes),
/// and list it in `accounts.toml`. Called after every sign-in, restore and switch.
pub fn remember_login(server_url: &str, session: &Session) -> Result<()> {
    save(server_url, session)?;
    save_account(server_url, session)?;
    let mut a = Accounts::load().unwrap_or_default();
    a.remember(server_url, session);
    a.save()
}

/// One remembered account: names only, never a token. `image_tag` is the server's public cache
/// tag of the profile picture (not a secret), so the account switcher can show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub server: String,
    pub user_id: String,
    pub user_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_tag: Option<String>,
    /// The profile picked automatically at launch (a name/id marker, never a secret). At most one
    /// per server; it lives on the entry, so forgetting the account clears it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

/// Remembered accounts across servers (`$XDG_CONFIG_HOME/nocturne/accounts.toml`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
    pub accounts: Vec<Account>,
}

impl Accounts {
    pub fn default_path() -> Result<PathBuf> {
        Ok(Servers::default_path()?.with_file_name("accounts.toml"))
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

    /// Add or update the account of `session` on `server_url`.
    pub fn remember(&mut self, server_url: &str, session: &Session) {
        let server = key(server_url).to_string();
        match self
            .accounts
            .iter_mut()
            .find(|a| a.server == server && a.user_id == session.user_id)
        {
            Some(a) => a.user_name = session.user_name.clone(),
            None => self.accounts.push(Account {
                server,
                user_id: session.user_id.clone(),
                user_name: session.user_name.clone(),
                image_tag: None,
                default: false,
            }),
        }
    }

    /// Record the profile picture tag of a remembered account; true when something changed.
    pub fn set_image_tag(&mut self, server_url: &str, user_id: &str, tag: Option<String>) -> bool {
        let server = key(server_url);
        match self
            .accounts
            .iter_mut()
            .find(|a| a.server == server && a.user_id == user_id)
        {
            Some(a) if a.image_tag != tag => {
                a.image_tag = tag;
                true
            }
            _ => false,
        }
    }

    /// The profile chosen to open automatically on `server_url`, if any.
    pub fn default_for(&self, server_url: &str) -> Option<&Account> {
        let server = key(server_url);
        self.accounts
            .iter()
            .find(|a| a.server == server && a.default)
    }

    /// Make `user_id` the default profile of `server_url` (`None` clears it); true when something
    /// changed. An unknown user changes nothing.
    pub fn set_default(&mut self, server_url: &str, user_id: Option<&str>) -> bool {
        let server = key(server_url);
        if user_id.is_some_and(|u| {
            !self
                .accounts
                .iter()
                .any(|a| a.server == server && a.user_id == u)
        }) {
            return false;
        }
        let mut changed = false;
        for a in self.accounts.iter_mut().filter(|a| a.server == server) {
            let want = user_id == Some(a.user_id.as_str());
            changed |= a.default != want;
            a.default = want;
        }
        changed
    }

    pub fn forget(&mut self, server_url: &str, user_id: &str) {
        let server = key(server_url);
        self.accounts
            .retain(|a| !(a.server == server && a.user_id == user_id));
    }

    /// Accounts of one server, in the order they were first remembered.
    pub fn for_server(&self, server_url: &str) -> Vec<&Account> {
        let server = key(server_url);
        self.accounts
            .iter()
            .filter(|a| a.server == server)
            .collect()
    }
}

/// Store the picture tag of a remembered account (no-op when unchanged or not remembered).
pub fn remember_image_tag(server_url: &str, user_id: &str, tag: Option<String>) {
    let mut a = Accounts::load().unwrap_or_default();
    if a.set_image_tag(server_url, user_id, tag) {
        let _ = a.save();
    }
}

/// Store the default profile of a server (`None`: ask every time); no-op when unchanged.
pub fn remember_default(server_url: &str, user_id: Option<&str>) {
    let mut a = Accounts::load().unwrap_or_default();
    if a.set_default(server_url, user_id) {
        let _ = a.save();
    }
}

/// Forget one account completely: its keyring token and its `accounts.toml` entry.
pub fn forget_account(server_url: &str, user_id: &str) {
    let _ = delete_account(server_url, user_id);
    let mut a = Accounts::load().unwrap_or_default();
    a.forget(server_url, user_id);
    let _ = a.save();
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
    fn image_tag_is_stored_per_account_and_only_saved_when_changed() {
        let mut a = Accounts::default();
        a.remember("http://h:8096/", &session());
        assert!(
            !a.set_image_tag("http://other", "u", Some("t".into())),
            "unknown server"
        );
        assert!(a.set_image_tag("http://h:8096", "u", Some("t".into())));
        assert!(
            !a.set_image_tag("http://h:8096", "u", Some("t".into())),
            "unchanged"
        );
        assert_eq!(a.accounts[0].image_tag.as_deref(), Some("t"));
        a.remember("http://h:8096", &session()); // re-login keeps the tag
        assert_eq!(a.accounts[0].image_tag.as_deref(), Some("t"));
        let text = toml::to_string_pretty(&a).unwrap();
        assert_eq!(toml::from_str::<Accounts>(&text).unwrap(), a);
        assert!(a.set_image_tag("http://h:8096", "u", None));
        assert!(!toml::to_string_pretty(&a).unwrap().contains("image_tag"));
        // Files written before this field existed still load.
        let old = "[[accounts]]\nserver = \"h\"\nuser_id = \"u\"\nuser_name = \"n\"\n";
        assert_eq!(
            toml::from_str::<Accounts>(old).unwrap().accounts[0].image_tag,
            None
        );
    }

    #[test]
    fn default_profile_is_one_per_server_clears_on_forget_and_old_files_load() {
        let mut bob = session();
        bob.user_id = "u2".into();
        bob.user_name = "bob".into();
        let mut a = Accounts::default();
        a.remember("http://a:1", &session());
        a.remember("http://a:1", &bob);
        a.remember("http://b:1", &session());
        assert!(a.default_for("http://a:1").is_none());
        assert!(!a.set_default("http://a:1", Some("nobody")), "unknown user");
        assert!(a.set_default("http://a:1", Some("u")));
        assert!(!a.set_default("http://a:1", Some("u")), "unchanged");
        assert!(a.set_default("http://a:1/", Some("u2")), "moves, never two");
        assert_eq!(a.default_for("http://a:1").unwrap().user_id, "u2");
        assert!(
            a.default_for("http://b:1").is_none(),
            "other server untouched"
        );
        a.remember("http://a:1", &bob); // re-login keeps the marker
        assert!(a.default_for("http://a:1").is_some());
        let text = toml::to_string_pretty(&a).unwrap();
        assert_eq!(text.matches("default = true").count(), 1);
        assert!(!text.contains("tok"));
        assert_eq!(toml::from_str::<Accounts>(&text).unwrap(), a);
        a.forget("http://a:1", "u2"); // removing the default account clears the default
        assert!(a.default_for("http://a:1").is_none());
        assert!(!toml::to_string_pretty(&a).unwrap().contains("default"));
        assert!(a.set_default("http://a:1", Some("u")));
        assert!(a.set_default("http://a:1", None));
        assert!(a.default_for("http://a:1").is_none());
        // Files written before the field existed still load, with no default.
        let old = "[[accounts]]\nserver = \"h\"\nuser_id = \"u\"\nuser_name = \"n\"\n";
        let old = toml::from_str::<Accounts>(old).unwrap();
        assert!(!old.accounts[0].default && old.default_for("h").is_none());
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

    #[test]
    fn several_accounts_per_server_keep_their_own_tokens() {
        let m = Mem::default();
        let mut bob = session();
        bob.user_id = "u2".into();
        bob.user_name = "bob".into();
        bob.access_token = "tok2".into();
        save_account_in(&m, "http://a:1/", &session()).unwrap();
        save_account_in(&m, "http://a:1", &bob).unwrap();
        let tok = |id| {
            load_account_from(&m, "http://a:1", id)
                .unwrap()
                .map(|s| s.access_token)
        };
        assert_eq!(
            (tok("u"), tok("u2")),
            (Some("tok".into()), Some("tok2".into()))
        );
        assert_eq!(tok("nobody"), None);
        assert!(load_account_from(&m, "http://b:1", "u").unwrap().is_none());
    }

    #[test]
    fn accounts_file_lists_names_only_and_forgets() {
        let dir = std::env::temp_dir().join(format!("nocturne-accts-{}", std::process::id()));
        let path = dir.join("nocturne").join("accounts.toml");
        assert_eq!(Accounts::load_from(&path).unwrap(), Accounts::default());
        let mut bob = session();
        bob.user_id = "u2".into();
        bob.user_name = "bob".into();
        let mut a = Accounts::default();
        a.remember("http://a:1/", &session());
        a.remember("http://a:1", &bob);
        a.remember("http://a:1", &session()); // again: no duplicate
        a.remember("http://b:1", &session());
        a.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("tok"), "no token on disk");
        let mut back = Accounts::load_from(&path).unwrap();
        assert_eq!(back, a);
        let names: Vec<_> = back
            .for_server("http://a:1/")
            .iter()
            .map(|x| x.user_name.as_str())
            .collect();
        assert_eq!(names, ["alice", "bob"]);
        back.forget("http://a:1", "u");
        assert_eq!(back.for_server("http://a:1").len(), 1);
        assert_eq!(back.for_server("http://b:1").len(), 1);
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
