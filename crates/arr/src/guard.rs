//! Safety guard for the live tests: they may only talk to the throwaway Radarr and Sonarr from
//! `scripts/test-arr.sh`. `ArrTestGuard::connect()` refuses unless
//! 1. the URLs come from the file named by `NOCTURNE_ARR_TEST_ENV` (never the repo `.env`),
//! 2. both are loopback on a non-default port (never Radarr's 7878 or Sonarr's 8989),
//! 3. neither equals a real service named by `NOCTURNE_RADARR_URL` / `NOCTURNE_SONARR_URL`,
//! 4. the env file says the containers' data directory is under `/tmp/`.

use crate::{Error, Result};
use reqwest::Url;

/// The services' own default ports: a real one is likely there, so a test never uses them.
pub const REAL_PORTS: [u16; 2] = [7878, 8989];

/// What the env file tells the live tests.
#[derive(Clone, Debug)]
pub struct TestEnv {
    pub radarr_url: String,
    pub radarr_key: String,
    pub sonarr_url: String,
    pub sonarr_key: String,
    pub data_dir: String,
}

pub struct ArrTestGuard;

impl ArrTestGuard {
    pub fn connect() -> Result<TestEnv> {
        let path = std::env::var("NOCTURNE_ARR_TEST_ENV").map_err(|_| {
            Error::Config("NOCTURNE_ARR_TEST_ENV is not set (run scripts/test-arr.sh start)".into())
        })?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("cannot read {path}: {e}")))?;
        let get = |k: &str| {
            parse_var(&text, k).ok_or_else(|| Error::Config(format!("{k} missing in {path}")))
        };
        let env = TestEnv {
            radarr_url: get("NOCTURNE_ARR_TEST_RADARR_URL")?,
            radarr_key: get("NOCTURNE_ARR_TEST_RADARR_KEY")?,
            sonarr_url: get("NOCTURNE_ARR_TEST_SONARR_URL")?,
            sonarr_key: get("NOCTURNE_ARR_TEST_SONARR_KEY")?,
            data_dir: get("NOCTURNE_ARR_TEST_DATA")?,
        };
        let real: Vec<String> = ["NOCTURNE_RADARR_URL", "NOCTURNE_SONARR_URL"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .collect();
        for url in [&env.radarr_url, &env.sonarr_url] {
            Self::check(url, &env.data_dir, &real)?;
        }
        Ok(env)
    }

    /// The pure checks (rules 2 to 4).
    pub fn check(url: &str, data_dir: &str, real: &[String]) -> Result<()> {
        let hp = host_port(url).ok_or_else(|| Error::Config("unparsable test URL".into()))?;
        if hp.0 != "127.0.0.1" || REAL_PORTS.contains(&hp.1) {
            return Err(Error::Config(format!(
                "test URL must be loopback and not port {} or {}",
                REAL_PORTS[0], REAL_PORTS[1]
            )));
        }
        if real.iter().filter_map(|u| host_port(u)).any(|r| r == hp) {
            return Err(Error::Config("test URL equals a real service".into()));
        }
        if !data_dir.starts_with("/tmp/") || data_dir.contains("..") {
            return Err(Error::Config(
                "data directory is not under /tmp: not a throwaway instance".into(),
            ));
        }
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_throwaway_and_refuses_the_rest() {
        assert!(ArrTestGuard::check("http://127.0.0.1:7879", "/tmp/x", &[]).is_ok());
        for (url, dir) in [
            ("http://127.0.0.1:7878", "/tmp/x"),
            ("http://localhost:8989", "/tmp/x"),
            ("http://10.0.0.5:7879", "/tmp/x"),
            ("http://radarr.local:7879", "/tmp/x"),
            ("http://127.0.0.1:7879", "/srv/radarr"),
            ("http://127.0.0.1:7879", "/tmp/../etc"),
            ("nope", "/tmp/x"),
        ] {
            assert!(ArrTestGuard::check(url, dir, &[]).is_err(), "{url} {dir}");
        }
        let real = vec!["http://127.0.0.1:7879".to_string()];
        assert!(ArrTestGuard::check("http://127.0.0.1:7879", "/tmp/x", &real).is_err());
    }
}
