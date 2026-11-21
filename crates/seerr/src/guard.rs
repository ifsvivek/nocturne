//! Safety guard for the live tests: they may only talk to the throwaway Seerr from
//! `scripts/test-seerr.sh`. `SeerrTestGuard::connect()` refuses unless
//! 1. the URL comes from the file named by `NOCTURNE_SEERR_TEST_ENV` (never the repo `.env`),
//! 2. the URL is loopback on a non-default port (never Seerr's own 5055),
//! 3. it differs from any real Seerr named by `NOCTURNE_SEERR_URL` in the process environment,
//! 4. the env file says the container's data directory is under `/tmp/`,
//! 5. the Seerr is attached to the Jellyfin that `nocturne_jellyfin_admin::guard::TestServerGuard`
//!    accepts (checked by the live tests through that guard).

use crate::{Error, Result};
use reqwest::Url;

/// Seerr's own default port: a real server is likely there, so a test never uses it.
pub const REAL_PORT: u16 = 5055;

/// What the env file tells the live tests.
#[derive(Debug, Clone)]
pub struct TestEnv {
    pub url: String,
    pub data_dir: String,
    pub admin_user: String,
    pub admin_password: String,
    pub alice_password: String,
}

pub struct SeerrTestGuard;

impl SeerrTestGuard {
    pub fn connect() -> Result<TestEnv> {
        let path = std::env::var("NOCTURNE_SEERR_TEST_ENV").map_err(|_| {
            Error::Config(
                "NOCTURNE_SEERR_TEST_ENV is not set (run scripts/test-seerr.sh start)".into(),
            )
        })?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("cannot read {path}: {e}")))?;
        let get = |k: &str| {
            parse_var(&text, k).ok_or_else(|| Error::Config(format!("{k} missing in {path}")))
        };
        let env = TestEnv {
            url: get("NOCTURNE_SEERR_TEST_URL")?,
            data_dir: get("NOCTURNE_SEERR_TEST_DATA")?,
            admin_user: get("NOCTURNE_SEERR_TEST_ADMIN_USER")?,
            admin_password: get("NOCTURNE_SEERR_TEST_ADMIN_PW")?,
            alice_password: get("NOCTURNE_SEERR_TEST_ALICE_PW")?,
        };
        Self::check(&env.url, &env.data_dir, &real_urls())?;
        Ok(env)
    }

    /// The pure checks (rules 2 to 4).
    pub fn check(url: &str, data_dir: &str, real: &[String]) -> Result<()> {
        let hp = host_port(url).ok_or_else(|| Error::Config("unparsable test URL".into()))?;
        if hp.0 != "127.0.0.1" || hp.1 == REAL_PORT {
            return Err(Error::Config(format!(
                "test URL must be loopback and not port {REAL_PORT}"
            )));
        }
        if real.iter().filter_map(|u| host_port(u)).any(|r| r == hp) {
            return Err(Error::Config("test URL equals the real Seerr".into()));
        }
        if !data_dir.starts_with("/tmp/") || data_dir.contains("..") {
            return Err(Error::Config(
                "Seerr data directory is not under /tmp: not a throwaway instance".into(),
            ));
        }
        Ok(())
    }
}

/// Normalised (host, port); `localhost` is `127.0.0.1`.
fn host_port(url: &str) -> Option<(String, u16)> {
    let u = Url::parse(url).ok()?;
    let h = match u.host_str()? {
        "localhost" | "[::1]" => "127.0.0.1",
        h => h,
    };
    Some((h.to_string(), u.port_or_known_default()?))
}

fn parse_var(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l
            .trim()
            .strip_prefix("export ")
            .unwrap_or(l.trim())
            .split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches(['"', '\'']).to_string())
    })
}

fn real_urls() -> Vec<String> {
    let mut v = vec![format!("http://127.0.0.1:{REAL_PORT}")];
    v.extend(std::env::var("NOCTURNE_SEERR_URL"));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_throwaway_and_refuses_the_rest() {
        let real = vec!["http://127.0.0.1:5055".to_string()];
        assert!(SeerrTestGuard::check("http://127.0.0.1:5057", "/tmp/x", &real).is_ok());
        assert!(SeerrTestGuard::check("http://localhost:5057/", "/tmp/x", &real).is_ok());
        for (url, dir) in [
            ("http://127.0.0.1:5055", "/tmp/x"),
            ("http://localhost:5055", "/tmp/x"),
            ("http://10.0.0.5:5057", "/tmp/x"),
            ("http://seerr.local:5057", "/tmp/x"),
            ("http://127.0.0.1:5057", "/var/lib/seerr"),
            ("http://127.0.0.1:5057", "/tmp/../etc"),
            ("nope", "/tmp/x"),
        ] {
            assert!(
                SeerrTestGuard::check(url, dir, &real).is_err(),
                "{url} {dir}"
            );
        }
        // a real Seerr configured on the same host and port is refused whatever the port
        let other = vec!["http://127.0.0.1:5057".to_string()];
        assert!(SeerrTestGuard::check("http://127.0.0.1:5057", "/tmp/x", &other).is_err());
    }

    #[test]
    fn env_file_parser() {
        assert_eq!(
            parse_var("A=1\nexport B=\"x y\"\n", "B").as_deref(),
            Some("x y")
        );
        assert_eq!(parse_var("A=1\n", "C"), None);
    }
}
