//! Safety guard for integration tests: they may only talk to the throwaway server.
//!
//! Every live test starts with `TestServerGuard::connect()`, which refuses unless
//! 1. the URL comes from the file named by `NOCTURNE_JF_TEST_ENV` (never the repo `.env`),
//! 2. the URL is loopback on the throwaway port [`TEST_PORT`],
//! 3. host:port differs from the real server (`127.0.0.1:8096`, `NOCTURNE_JF_URL` from the process
//!    environment, or from the repo `.env`, read only to find that one URL),
//! 4. the server itself reports a data path under `/tmp/` (so a mis-pointed port cannot slip through).

use crate::{AdminClient, Config, Error, Result};
use reqwest::Url;

/// Default port used by `scripts/test-jellyfin.sh`.
pub const TEST_PORT: u16 = 8097;

/// The second throwaway (`scripts/test-jellyfin.sh start 2`), for merge tests that need two servers.
pub const TEST_PORT_2: u16 = 8098;

/// Port the tests may talk to: `NOCTURNE_JF_TEST_PORT` (a second throwaway) or [`TEST_PORT`].
/// Never 8096, the usual port of a real server.
pub fn test_port() -> u16 {
    std::env::var("NOCTURNE_JF_TEST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .filter(|p| *p != 8096)
        .unwrap_or(TEST_PORT)
}

pub struct TestServerGuard;

impl TestServerGuard {
    /// Load the throwaway server's URL/token from `NOCTURNE_JF_TEST_ENV`, run all checks, return a client.
    pub async fn connect() -> Result<AdminClient> {
        Self::connect_env("NOCTURNE_JF_TEST_ENV")
            .await
            .map(|(c, _, _)| c)
    }

    /// [`TestServerGuard::connect`] for the env file named by `var` (`NOCTURNE_JF_TEST_ENV_2` for the second instance), also returning the checked URL and the throwaway key for tests that drive the server themselves.
    pub async fn connect_env(var: &str) -> Result<(AdminClient, String, String)> {
        let path = std::env::var(var).map_err(|_| {
            Error::Guard(format!(
                "{var} is not set (run scripts/test-jellyfin.sh start)"
            ))
        })?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::Guard(format!("cannot read {path}: {e}")))?;
        let get = |k: &str| {
            parse_var(&text, k).ok_or_else(|| Error::Guard(format!("{k} missing in {path}")))
        };
        let (url, token) = (get("NOCTURNE_JF_TEST_URL")?, get("NOCTURNE_JF_TEST_TOKEN")?);
        Self::check_url(&url, &real_urls())?;
        let client = AdminClient::new(Config {
            base_url: url.clone(),
            token: token.clone(),
        })?;
        let data = client.system_info().await?.program_data_path;
        if !data.starts_with("/tmp/") {
            return Err(Error::Guard(
                "server data path is not under /tmp: not a throwaway instance".into(),
            ));
        }
        Ok((client, url, token))
    }

    /// Pure URL check (see module docs, rules 2 and 3).
    pub fn check_url(url: &str, real: &[String]) -> Result<()> {
        let hp = host_port(url).ok_or_else(|| Error::Guard("unparsable test URL".into()))?;
        if !hp.0.is_loopback() || ![TEST_PORT, TEST_PORT_2, test_port()].contains(&hp.1) {
            return Err(Error::Guard(format!(
                "test URL must be loopback port {TEST_PORT}, {TEST_PORT_2} or {}",
                test_port()
            )));
        }
        if real.iter().filter_map(|u| host_port(u)).any(|r| r == hp) {
            return Err(Error::Guard("test URL equals the real server".into()));
        }
        Ok(())
    }
}

#[derive(PartialEq)]
struct Host(String);
impl Host {
    fn is_loopback(&self) -> bool {
        self.0 == "127.0.0.1"
    }
}

/// Normalised (host, port); `localhost` == `127.0.0.1`.
fn host_port(url: &str) -> Option<(Host, u16)> {
    let u = Url::parse(url).ok()?;
    let h = match u.host_str()? {
        "localhost" | "[::1]" => "127.0.0.1",
        h => h,
    };
    Some((Host(h.to_string()), u.port_or_known_default()?))
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
    let mut v = vec!["http://127.0.0.1:8096".to_string()];
    v.extend(std::env::var("NOCTURNE_JF_URL"));
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in manifest.ancestors() {
        if let Ok(text) = std::fs::read_to_string(dir.join(".env")) {
            v.extend(parse_var(&text, "NOCTURNE_JF_URL"));
            break;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real(u: &str) -> Vec<String> {
        vec![u.to_string()]
    }

    #[test]
    fn accepts_throwaway() {
        assert!(
            TestServerGuard::check_url("http://127.0.0.1:8097", &real("http://127.0.0.1:8096"))
                .is_ok()
        );
        assert!(
            TestServerGuard::check_url("http://localhost:8097/", &real("http://127.0.0.1:8096"))
                .is_ok()
        );
        // The second throwaway instance (merge tests).
        assert!(
            TestServerGuard::check_url("http://127.0.0.1:8098", &real("http://127.0.0.1:8096"))
                .is_ok()
        );
    }

    #[test]
    fn refuses_real_or_wrong_port_or_remote() {
        let r = real("http://127.0.0.1:8096");
        for bad in [
            "http://127.0.0.1:8096",
            "http://localhost:8096",
            "http://127.0.0.1:9000",
            "http://10.0.0.5:8097",
            "http://omen:8097",
            "nope",
        ] {
            assert!(TestServerGuard::check_url(bad, &r).is_err(), "{bad}");
        }
        // even on the throwaway port, a real server configured there is refused
        assert!(
            TestServerGuard::check_url("http://127.0.0.1:8097", &real("http://localhost:8097"))
                .is_err()
        );
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
