//! Users (Argon2id, same parameters as the gateway), session tokens and named API keys. Tokens are 256 random bits; only their SHA-256 is stored; every one is revocable. The unix-socket peer with the server's uid is the local admin ([`Caller::Local`]) and needs none.
use crate::db::Res;
use crate::rec;
use crate::state::{Shared, now};
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRec {
    pub id: u64,
    pub name: String,
    /// Argon2id PHC string.
    pub hash: String,
    pub admin: bool,
    pub created: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Session,
    ApiKey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRec {
    pub id: u64,
    pub user: u64,
    pub kind: TokenKind,
    /// SHA-256 of the token, hex.
    pub hash: String,
    /// Client (sessions) or the key's name (API keys).
    pub name: String,
    pub created: u64,
    pub last_used: u64,
}

rec!(UserRec, "users");
rec!(TokenRec, "tokens");

/// Who is calling; handlers take `Extension<Caller>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// The unix socket, same uid as the server.
    Local,
    User {
        user: u64,
        name: String,
        admin: bool,
        token: u64,
        kind: TokenKind,
    },
}

impl Caller {
    pub fn name(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::User { name, .. } => name,
        }
    }
    pub fn admin(&self) -> bool {
        matches!(self, Self::Local | Self::User { admin: true, .. })
    }
}

fn random_hex() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("the system random source works");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn hash_password(pw: &str) -> String {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("the system random source works");
    let salt = SaltString::encode_b64(&salt).expect("16 bytes is a valid salt");
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .expect("argon2 hashes a short password")
        .to_string()
}

fn verify_hash(hash: &str, pw: &str) -> bool {
    PasswordHash::new(hash)
        .is_ok_and(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
}

pub fn check_password(pw: &str) -> Res<()> {
    if pw.chars().count() < 8 {
        return Err("the password needs at least 8 characters".into());
    }
    Ok(())
}

pub fn setup_needed(sh: &Shared) -> bool {
    sh.db.all::<UserRec>().map_or(true, |u| u.is_empty())
}

pub async fn create_user(sh: &Shared, name: &str, pw: &str, admin: bool) -> Res<UserRec> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a user name is needed".into());
    }
    check_password(pw)?;
    if sh
        .db
        .all::<UserRec>()?
        .iter()
        .any(|u| u.name.eq_ignore_ascii_case(name))
    {
        return Err("that user name is taken".into());
    }
    let pw = pw.to_string();
    let hash = tokio::task::spawn_blocking(move || hash_password(&pw))
        .await
        .map_err(|e| e.to_string())?;
    let mut u = UserRec {
        id: 0,
        name: name.into(),
        hash,
        admin,
        created: now(),
    };
    sh.db.put(&mut u)?;
    Ok(u)
}

/// The user with that name and password. Argon2 runs off the async threads.
pub async fn verify(sh: &Shared, name: &str, pw: &str) -> Option<UserRec> {
    let u = sh
        .db
        .all::<UserRec>()
        .ok()?
        .into_iter()
        .find(|u| u.name.eq_ignore_ascii_case(name.trim()));
    // Hash anyway when the user is unknown, so timing does not tell names apart.
    let (hash, pw) = (u.as_ref().map(|u| u.hash.clone()), pw.to_string());
    let ok = tokio::task::spawn_blocking(move || match hash {
        Some(h) => verify_hash(&h, &pw),
        None => {
            let _ = hash_password(&pw);
            false
        }
    })
    .await
    .unwrap_or(false);
    u.filter(|_| ok)
}

pub async fn set_password(sh: &Shared, user: u64, pw: &str) -> Res<()> {
    check_password(pw)?;
    let mut u = sh.db.get::<UserRec>(user)?.ok_or("no such user")?;
    let pw = pw.to_string();
    u.hash = tokio::task::spawn_blocking(move || hash_password(&pw))
        .await
        .map_err(|e| e.to_string())?;
    sh.db.put(&mut u)?;
    Ok(())
}

/// A new token; returns it (shown once) and its record.
pub fn mint(sh: &Shared, user: u64, kind: TokenKind, name: &str) -> Res<(String, TokenRec)> {
    let token = format!(
        "{}{}",
        if kind == TokenKind::Session {
            "nst_"
        } else {
            "nsk_"
        },
        random_hex()
    );
    let mut r = TokenRec {
        id: 0,
        user,
        kind,
        hash: sha256_hex(&token),
        name: name.chars().take(100).collect(),
        created: now(),
        last_used: now(),
    };
    sh.db.put(&mut r)?;
    Ok((token, r))
}

/// A session unused this long (seconds) is gone; API keys stay until revoked.
pub const SESSION_IDLE: u64 = 30 * 86_400;

/// The caller a presented token stands for.
pub fn lookup(sh: &Shared, token: &str) -> Option<Caller> {
    if token.len() < 20 {
        return None;
    }
    let h = sha256_hex(token);
    // ponytail: linear scan of the tokens table; an index by hash when there are thousands.
    let mut t = sh
        .db
        .all::<TokenRec>()
        .ok()?
        .into_iter()
        .find(|t| t.hash == h)?;
    if t.kind == TokenKind::Session && now() > t.last_used + SESSION_IDLE {
        let _ = sh.db.delete::<TokenRec>(&[t.id]);
        return None;
    }
    let u = sh.db.get::<UserRec>(t.user).ok()??;
    if now() > t.last_used + 60 {
        t.last_used = now();
        let _ = sh.db.put(&mut t);
    }
    Some(Caller::User {
        user: u.id,
        name: u.name,
        admin: u.admin,
        token: t.id,
        kind: t.kind,
    })
}

pub fn list(sh: &Shared, kind: TokenKind) -> Res<Vec<TokenRec>> {
    Ok(sh
        .db
        .all::<TokenRec>()?
        .into_iter()
        .filter(|t| t.kind == kind)
        .collect())
}

/// Revoke one token of a kind; false if there was none.
pub fn revoke(sh: &Shared, kind: TokenKind, id: u64) -> Res<bool> {
    match sh.db.get::<TokenRec>(id)? {
        Some(t) if t.kind == kind => {
            sh.db.delete::<TokenRec>(&[id])?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Revoke every session (password change); API keys stay.
pub fn revoke_sessions(sh: &Shared) -> Res<()> {
    let ids: Vec<u64> = list(sh, TokenKind::Session)?.iter().map(|t| t.id).collect();
    sh.db.delete::<TokenRec>(&ids)
}

/// `<data>/setup-code`: the one-time code first-run setup over TCP needs (0600, only the owner reads it; `nocturne-server open` puts it in the link). Gone once an admin exists.
pub fn setup_code_path(p: &crate::state::Paths) -> std::path::PathBuf {
    p.data.join("setup-code")
}

pub fn setup_code(p: &crate::state::Paths) -> Option<String> {
    std::fs::read_to_string(setup_code_path(p))
        .ok()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

/// While setup is needed: make the code once (kept across restarts so a printed link stays valid). Otherwise remove a stale one.
pub fn prepare_setup_code(sh: &Shared) -> Res<()> {
    let path = setup_code_path(&sh.paths);
    if !setup_needed(sh) {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    if setup_code(&sh.paths).is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(&sh.paths.data).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let code: String = random_hex().chars().take(24).collect();
    std::io::Write::write_all(
        &mut o.open(&path).map_err(|e| e.to_string())?,
        code.as_bytes(),
    )
    .map_err(|e| e.to_string())
}

/// The presented code is the waiting one (compared as hashes, so timing tells nothing).
pub fn setup_code_ok(p: &crate::state::Paths, given: Option<&str>) -> bool {
    match (setup_code(p), given) {
        (Some(c), Some(g)) => sha256_hex(&c) == sha256_hex(g.trim()),
        _ => false,
    }
}
