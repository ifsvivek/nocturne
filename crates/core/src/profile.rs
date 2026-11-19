//! Source profiles: which servers and local folders the user has set up, in priority order.
//! `$XDG_CONFIG_HOME/nocturne/sources.toml`, versioned, atomic write, never a secret: a Jellyfin
//! profile's token stays in the keyring under the existing (server url, user id) key.
use crate::source::SourceId;
use nocturne_session::{Accounts, Servers};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileKind {
    Jellyfin {
        url: String,
        user_id: String,
        user_name: String,
    },
    Local {
        roots: Vec<PathBuf>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: SourceId,
    pub label: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Online details and subtitle switches (local sources only; off by default). No secrets: keys are in the keyring.
    #[serde(default)]
    pub online: crate::online::Flags,
    /// Jellyfin sources: follow the server's own home layout (section order, My media tiles,
    /// branding CSS) when this server alone is selected. Off for new servers.
    #[serde(default)]
    pub server_ui: bool,
    #[serde(flatten)]
    pub kind: ProfileKind,
}

fn yes() -> bool {
    true
}

/// The profile list. List order is priority: the first source wins as the preferred copy of a duplicate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sources {
    pub version: u32,
    /// Next id to hand out; only grows, so a removed source's id is never reused for another one.
    pub next_id: u32,
    #[serde(rename = "profile")]
    pub profiles: Vec<Profile>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            version: VERSION,
            next_id: 1,
            profiles: vec![],
        }
    }
}

/// The single login the 0.2.0 app knew: its server, user and display name (no token).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Legacy {
    pub url: String,
    pub user_id: String,
    pub user_name: String,
}

impl Sources {
    /// `$XDG_CONFIG_HOME/nocturne/sources.toml`, next to `servers.toml`.
    pub fn default_path() -> Option<PathBuf> {
        Servers::default_path()
            .ok()
            .map(|p| p.with_file_name("sources.toml"))
    }

    /// Load the real file; if it does not exist yet, build it once from the old single-server files and
    /// keyring login. The old files are only read, never changed.
    pub fn load() -> io::Result<Self> {
        let path = Self::default_path().ok_or_else(|| io::Error::other("no config directory"))?;
        load_or_migrate(&path, || {
            legacy_logins(
                &Servers::load().unwrap_or_default(),
                &Accounts::load().unwrap_or_default(),
                &|url| {
                    nocturne_session::load(url)
                        .ok()
                        .flatten()
                        .map(|s| (s.user_id, s.user_name))
                },
            )
        })
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::default_path().ok_or_else(|| io::Error::other("no config directory"))?;
        self.save_to(&path)
    }

    /// `Ok(None)` if the file is absent; a corrupt or newer file is an error (it is never overwritten here).
    pub fn load_from(path: &Path) -> io::Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let s: Self = toml::from_str(&text).map_err(io::Error::other)?;
        if s.version > VERSION {
            return Err(io::Error::other("sources.toml is from a newer NOCTURNE"));
        }
        Ok(Some(s))
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("path has a parent"))?;
        let tmp = path.with_extension("toml.tmp");
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    /// Append a profile (lowest priority) and return its new id.
    pub fn add(&mut self, label: impl Into<String>, kind: ProfileKind) -> SourceId {
        let id = SourceId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        self.profiles.push(Profile {
            id,
            label: label.into(),
            enabled: true,
            online: Default::default(),
            server_ui: false,
            kind,
        });
        id
    }

    pub fn remove(&mut self, id: SourceId) -> bool {
        let n = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        self.profiles.len() != n
    }

    pub fn get(&self, id: SourceId) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Move a profile to list position `to` (0 = highest priority).
    pub fn move_to(&mut self, id: SourceId, to: usize) {
        if let Some(i) = self.profiles.iter().position(|p| p.id == id) {
            let p = self.profiles.remove(i);
            self.profiles.insert(to.min(self.profiles.len()), p);
        }
    }

    /// Ids in priority order.
    pub fn ids(&self) -> Vec<SourceId> {
        self.profiles.iter().map(|p| p.id).collect()
    }

    /// Label a server by the name it announces, unless the user (or an earlier call) already chose
    /// one: only a label that is still the default host label is replaced. True if it changed.
    pub fn name_server(&mut self, id: SourceId, name: &str) -> bool {
        let name = name.trim();
        let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        let ProfileKind::Jellyfin { url, .. } = &p.kind else {
            return false;
        };
        if name.is_empty() || p.label != label_for(url) || p.label == name {
            return false;
        }
        p.label = name.to_string();
        true
    }

    /// Change a source's online switches; false if there is no such source.
    pub fn set_online(&mut self, id: SourceId, f: impl FnOnce(&mut crate::online::Flags)) -> bool {
        match self.profiles.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                f(&mut p.online);
                true
            }
            None => false,
        }
    }

    pub fn set_server_ui(&mut self, id: SourceId, on: bool) -> bool {
        let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        p.server_ui = on;
        true
    }

    pub fn set_enabled(&mut self, id: SourceId, on: bool) {
        if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
            p.enabled = on;
        }
    }

    /// Move one place up (`-1`) or down (`1`) in priority; stops at the ends. True if it moved.
    pub fn shift(&mut self, id: SourceId, by: i32) -> bool {
        let Some(i) = self.profiles.iter().position(|p| p.id == id) else {
            return false;
        };
        let j =
            (i as i64 + i64::from(by.signum())).clamp(0, self.profiles.len() as i64 - 1) as usize;
        self.profiles.swap(i, j);
        i != j
    }

    /// The profile of the server at `url` (one profile per server address, whatever the user).
    pub fn server_for(&self, url: &str) -> Option<&Profile> {
        let u = url.trim_end_matches('/');
        self.profiles.iter().find(|p| {
            matches!(&p.kind, ProfileKind::Jellyfin { url, .. } if url.trim_end_matches('/') == u)
        })
    }

    /// A login on `url`: a known server only changes its user (the account switch), a new one is
    /// appended. Returns the profile id.
    pub fn upsert_server(&mut self, url: &str, user_id: &str, user_name: &str) -> SourceId {
        let url = url.trim_end_matches('/');
        let kind = ProfileKind::Jellyfin {
            url: url.into(),
            user_id: user_id.into(),
            user_name: user_name.into(),
        };
        match self.server_for(url).map(|p| p.id) {
            Some(id) => {
                if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
                    p.kind = kind;
                }
                id
            }
            None => self.add(label_for(url), kind),
        }
    }

    /// The local library profile (all folders live in one), if any.
    pub fn local(&self) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| matches!(p.kind, ProfileKind::Local { .. }))
    }

    /// The local profile, created (lowest priority, no folders yet) if there is none.
    pub fn ensure_local(&mut self) -> SourceId {
        match self.local().map(|p| p.id) {
            Some(id) => id,
            None => self.add("Local library", ProfileKind::Local { roots: vec![] }),
        }
    }

    /// Add a folder to the local library; false if it is already there.
    pub fn add_root(&mut self, root: PathBuf) -> bool {
        let id = self.ensure_local();
        match self
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .map(|p| &mut p.kind)
        {
            Some(ProfileKind::Local { roots }) if !roots.contains(&root) => {
                roots.push(root);
                true
            }
            _ => false,
        }
    }

    pub fn remove_root(&mut self, root: &Path) -> bool {
        match self.profiles.iter_mut().find_map(|p| match &mut p.kind {
            ProfileKind::Local { roots } => Some(roots),
            _ => None,
        }) {
            Some(roots) => {
                let n = roots.len();
                roots.retain(|r| r != root);
                roots.len() != n
            }
            None => false,
        }
    }

    /// First run: nothing is set up at all (removing every source brings the welcome back).
    pub fn needs_welcome(&self) -> bool {
        self.profiles.is_empty()
    }
}

/// Forget the token of `user_id` on `url` (keyring and `accounts.toml`), plus the active-login
/// entry when it is that user. Other users of the server are untouched.
pub fn forget_login(url: &str, user_id: &str) {
    nocturne_session::forget_account(url, user_id);
    if nocturne_session::load(url)
        .ok()
        .flatten()
        .is_some_and(|s| s.user_id == user_id)
    {
        let _ = nocturne_session::delete(url);
    }
}

/// An existing `sources.toml` wins. Without one, every old login becomes a profile (the last used
/// server first, so it keeps the highest priority) and is saved; with none, the list is empty and
/// nothing is written (first run).
pub fn load_or_migrate(path: &Path, legacy: impl FnOnce() -> Vec<Legacy>) -> io::Result<Sources> {
    if let Some(s) = Sources::load_from(path)? {
        return Ok(s);
    }
    let mut s = Sources::default();
    let found = legacy();
    for l in found {
        s.add(
            label_for(&l.url),
            ProfileKind::Jellyfin {
                url: l.url,
                user_id: l.user_id,
                user_name: l.user_name,
            },
        );
    }
    if !s.profiles.is_empty() {
        s.save_to(path)?;
    }
    Ok(s)
}

/// The logins the old app knew: for every known server (last used first, then the others, then
/// servers only known through an account) its keyring user, else its default or first remembered
/// account. A server with neither is skipped. `active(url)` is the keyring lookup, `(user id, user name)`.
pub fn legacy_logins(
    servers: &Servers,
    accounts: &Accounts,
    active: &dyn Fn(&str) -> Option<(String, String)>,
) -> Vec<Legacy> {
    let norm = |u: &str| u.trim_end_matches('/').to_string();
    let mut urls: Vec<String> = vec![];
    for u in servers
        .last_used
        .iter()
        .chain(&servers.servers)
        .chain(accounts.accounts.iter().map(|a| &a.server))
    {
        if !urls.contains(&norm(u)) {
            urls.push(norm(u));
        }
    }
    urls.into_iter()
        .filter_map(|url| {
            let (user_id, user_name) = active(&url).or_else(|| {
                let a = accounts
                    .default_for(&url)
                    .or_else(|| accounts.for_server(&url).into_iter().next())?;
                Some((a.user_id.clone(), a.user_name.clone()))
            })?;
            Some(Legacy {
                url,
                user_id,
                user_name,
            })
        })
        .collect()
}

/// Default label: the server's host.
pub fn label_for(url: &str) -> String {
    url.split("://")
        .last()
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// The token of one (server, user) from the system keyring: the remembered account entry, else the
/// active login if it is that user.
pub fn keyring_token(url: &str, user_id: &str) -> Option<String> {
    if let Ok(Some(s)) = nocturne_session::load_account(url, user_id) {
        return Some(s.access_token);
    }
    nocturne_session::load(url)
        .ok()
        .flatten()
        .filter(|s| s.user_id == user_id)
        .map(|s| s.access_token)
}

/// Read each server's extra header from the keyring into the clients' registry. Run (off the UI
/// thread) before a slot's clients are built.
pub fn load_extra_headers(profiles: &[Profile]) {
    for p in profiles {
        if let ProfileKind::Jellyfin { url, .. } = &p.kind {
            let line = nocturne_session::load_header(url).ok().flatten();
            let _ = nocturne_jellyfin::header::set(url, line.as_deref());
        }
    }
}

/// The saved extra header of `url` (`Name: value`), for the edit dialog. Blocking keyring read.
pub fn extra_header(url: &str) -> Option<String> {
    nocturne_session::load_header(url).ok().flatten()
}

/// Validate, store (keyring only) and activate the extra header of `url`; `None` or blank removes it.
/// Clients built afterwards send it. Blocking keyring call.
pub fn set_extra_header(url: &str, line: Option<&str>) -> Result<(), String> {
    let line = line.map(str::trim).filter(|l| !l.is_empty());
    match line {
        Some(l) => {
            nocturne_jellyfin::header::parse(l).map_err(str::to_string)?;
            nocturne_session::save_header(url, l)
                .map_err(|_| "No system keyring is available to keep the header".to_string())?;
        }
        None => nocturne_session::delete_header(url)
            .map_err(|_| "Couldn't remove the saved header".to_string())?,
    }
    nocturne_jellyfin::header::set(url, line).map_err(str::to_string)
}

#[cfg(test)]
mod online_flag_tests {
    use super::*;

    #[test]
    fn online_flags_default_off_persist_and_old_files_load() {
        let mut s = Sources::default();
        let id = s.ensure_local();
        assert!(!s.get(id).unwrap().online.details, "off by default");
        s.profiles[0].online = crate::online::Flags {
            details: true,
            tmdb: true,
            ..Default::default()
        };
        let text = toml::to_string_pretty(&s).unwrap();
        assert!(
            !text.to_lowercase().contains("key"),
            "switches only, never a key: {text}"
        );
        let back: Sources = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        // A sources.toml written before this existed has no `online` table.
        let old = "version = 1\nnext_id = 2\n[[profile]]\nid = 1\nlabel = \"Local\"\nkind = \"local\"\nroots = []\n";
        let o: Sources = toml::from_str(old).unwrap();
        assert_eq!(o.profiles[0].online, crate::online::Flags::default());
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn a_server_is_named_once_and_user_edits_stay() {
        let mut s = Sources::default();
        let id = s.upsert_server("http://home.test:8096", "u", "n");
        assert!(s.name_server(id, "  Basement  "));
        assert_eq!(s.get(id).unwrap().label, "Basement");
        // Already named (or edited by the user): not replaced again.
        assert!(!s.name_server(id, "Other"));
        assert!(!s.name_server(id, ""));
        let local = s.ensure_local();
        assert!(!s.name_server(local, "x"), "only servers are named");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_jellyfin_user::Session;

    fn jf(url: &str) -> ProfileKind {
        ProfileKind::Jellyfin {
            url: url.into(),
            user_id: "u1".into(),
            user_name: "Ann".into(),
        }
    }

    #[test]
    fn save_load_round_trip_has_no_secret_and_keeps_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nocturne/sources.toml");
        let mut s = Sources::default();
        let a = s.add("Home", jf("http://a.test"));
        let b = s.add(
            "Disk",
            ProfileKind::Local {
                roots: vec!["/mnt/films".into()],
            },
        );
        s.profiles[1].enabled = false;
        s.save_to(&p).unwrap();
        assert!(!p.with_extension("toml.tmp").exists(), "atomic rename");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("token"));
        let back = Sources::load_from(&p).unwrap().unwrap();
        assert_eq!(back, s);
        assert_eq!((a, b), (SourceId(1), SourceId(2)));
        assert!(!back.profiles[1].enabled && back.profiles[0].enabled);
    }

    #[test]
    fn ids_are_never_reused_and_order_is_priority() {
        let mut s = Sources::default();
        let a = s.add("A", jf("http://a.test"));
        let b = s.add("B", jf("http://b.test"));
        assert!(s.remove(b));
        let c = s.add("C", jf("http://c.test"));
        assert_eq!(c, SourceId(3), "removed id stays retired");
        s.move_to(c, 0);
        assert_eq!(s.ids(), [c, a]);
        assert!(!s.remove(b) && s.get(a).is_some());
    }

    #[test]
    fn missing_file_is_none_and_corrupt_or_newer_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sources.toml");
        assert!(Sources::load_from(&p).unwrap().is_none());
        std::fs::write(&p, "this is = [not toml").unwrap();
        assert!(Sources::load_from(&p).is_err());
        std::fs::write(&p, "version = 99\n").unwrap();
        assert!(Sources::load_from(&p).is_err());
        assert!(load_or_migrate(&p, Vec::new).is_err(), "never overwritten");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "version = 99\n");
    }

    #[test]
    fn migrates_the_old_login_once_and_leaves_old_files_alone() {
        let d = tempfile::tempdir().unwrap();
        let cfg = d.path().join("nocturne");
        let (sp, ap, np) = (
            cfg.join("servers.toml"),
            cfg.join("accounts.toml"),
            cfg.join("sources.toml"),
        );
        let mut servers = Servers::default();
        servers.remember("http://old.test/");
        servers.save_to(&sp).unwrap();
        let mut accounts = Accounts::default();
        let session = Session {
            access_token: "SECRET".into(),
            user_id: "u9".into(),
            server_id: "s".into(),
            user_name: "Zed".into(),
            is_admin: false,
        };
        accounts.remember("http://old.test", &session);
        accounts.save_to(&ap).unwrap();
        let before = (std::fs::read(&sp).unwrap(), std::fs::read(&ap).unwrap());
        // keyring says nothing: the remembered account is used.
        let legacy = || legacy_logins(&servers, &accounts, &|_| None);
        let s = load_or_migrate(&np, legacy).unwrap();
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(
            s.profiles[0].kind,
            ProfileKind::Jellyfin {
                url: "http://old.test".into(),
                user_id: "u9".into(),
                user_name: "Zed".into()
            }
        );
        assert_eq!(s.profiles[0].label, "old.test");
        assert!(!std::fs::read_to_string(&np).unwrap().contains("SECRET"));
        // second run: the saved file wins, legacy is not consulted again.
        let again =
            load_or_migrate(&np, || -> Vec<Legacy> { panic!("must not migrate twice") }).unwrap();
        assert_eq!(again, s);
        assert_eq!(
            before,
            (std::fs::read(&sp).unwrap(), std::fs::read(&ap).unwrap())
        );
    }

    #[test]
    fn keyring_login_beats_remembered_account_and_nothing_means_empty() {
        let servers = Servers {
            servers: vec!["http://a.test".into()],
            last_used: None,
        };
        let l = legacy_logins(&servers, &Accounts::default(), &|u| {
            (u == "http://a.test").then(|| ("k1".to_string(), "Kay".to_string()))
        });
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].user_id.as_str(), l[0].user_name.as_str()),
            ("k1", "Kay")
        );
        assert!(legacy_logins(&Servers::default(), &Accounts::default(), &|_| None).is_empty());
        assert!(legacy_logins(&servers, &Accounts::default(), &|_| None).is_empty());
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sources.toml");
        let s = load_or_migrate(&p, Vec::new).unwrap();
        assert!(
            s.profiles.is_empty() && !p.exists(),
            "first run writes nothing"
        );
    }

    #[test]
    fn every_server_with_a_login_migrates_last_used_first() {
        let session = |id: &str, name: &str| Session {
            access_token: "T".into(),
            user_id: id.into(),
            server_id: "s".into(),
            user_name: name.into(),
            is_admin: false,
        };
        let servers = Servers {
            servers: vec![
                "http://a.test".into(),
                "http://b.test/".into(),
                "http://c.test".into(),
                "http://dead.test".into(),
            ],
            last_used: Some("http://b.test".into()),
        };
        let mut accounts = Accounts::default();
        accounts.remember("http://a.test", &session("a1", "Ann"));
        accounts.remember("http://a.test", &session("a2", "Al"));
        accounts.remember("http://c.test", &session("c1", "Cy"));
        // b has only a keyring login; dead has nothing; e is known only through an account.
        accounts.remember("http://e.test", &session("e1", "Eve"));
        let l = legacy_logins(&servers, &accounts, &|u| {
            (u == "http://b.test").then(|| ("b9".to_string(), "Bo".to_string()))
        });
        let got: Vec<_> = l
            .iter()
            .map(|l| (l.url.as_str(), l.user_id.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("http://b.test", "b9"),
                ("http://a.test", "a1"),
                ("http://c.test", "c1"),
                ("http://e.test", "e1")
            ]
        );
        let d = tempfile::tempdir().unwrap();
        let np = d.path().join("sources.toml");
        let s = load_or_migrate(&np, || l.clone()).unwrap();
        assert_eq!(s.profiles.len(), 4);
        assert_eq!(s.profiles[0].label, "b.test");
    }

    #[test]
    fn list_ops_follow_the_sources_screen() {
        let mut s = Sources::default();
        let a = s.upsert_server("http://a.test/", "u1", "Ann");
        let b = s.upsert_server("http://b.test", "u2", "Bo");
        // Same server again: the profile keeps its id and priority, only the user changes.
        assert_eq!(s.upsert_server("http://a.test", "u7", "Al"), a);
        assert_eq!(s.profiles.len(), 2);
        assert!(matches!(&s.get(a).unwrap().kind,
            ProfileKind::Jellyfin { user_id, .. } if user_id == "u7"));
        assert!(!s.needs_welcome());
        assert!(s.add_root("/m/films".into()));
        assert!(!s.add_root("/m/films".into()), "no duplicate folder");
        assert!(s.add_root("/m/shows".into()));
        let local = s.local().unwrap().id;
        assert_eq!(s.ids(), [a, b, local]);
        assert!(s.shift(local, -1) && s.shift(local, -1));
        assert_eq!(s.ids(), [local, a, b]);
        assert!(!s.shift(local, -1), "stops at the top");
        assert!(!s.shift(b, 1), "stops at the bottom");
        s.set_enabled(a, false);
        assert!(!s.get(a).unwrap().enabled);
        assert!(s.remove_root(Path::new("/m/films")) && !s.remove_root(Path::new("/m/films")));
        assert!(matches!(&s.local().unwrap().kind,
            ProfileKind::Local { roots } if roots == &[PathBuf::from("/m/shows")]));
        for id in s.ids() {
            s.remove(id);
        }
        assert!(s.needs_welcome(), "all sources removed: welcome again");
    }
}
