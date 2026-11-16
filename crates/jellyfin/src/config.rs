use crate::{Error, Result};
use std::path::Path;

/// Connection settings. `Debug` redacts the token.
#[derive(Clone)]
pub struct Config {
    pub base_url: String,
    pub token: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl Config {
    /// Reads `NOCTURNE_JF_URL` (default `http://127.0.0.1:8096`) and `NOCTURNE_JF_TOKEN`.
    /// Debug builds first load a `.env` file from the current directory or the nearest ancestor
    /// (developer convenience); variables already set in the process environment win. Release
    /// builds never read `.env`, so a stray file in some parent directory cannot point an
    /// installed app at a server.
    pub fn load() -> Result<Self> {
        if cfg!(debug_assertions) {
            Self::load_dotenv();
        }
        Self::from_vars(
            std::env::var("NOCTURNE_JF_URL").ok(),
            std::env::var("NOCTURNE_JF_TOKEN").ok(),
        )
    }

    fn load_dotenv() {
        let found = std::env::current_dir().ok().and_then(|cwd| {
            cwd.ancestors()
                .map(|d| d.join(".env"))
                .find(|p| p.is_file())
        });
        if let Some(p) = found {
            load_env_file(&p);
        }
    }

    pub(crate) fn from_vars(url: Option<String>, token: Option<String>) -> Result<Self> {
        let token = token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| Error::Config("NOCTURNE_JF_TOKEN is not set".into()))?;
        let base_url = url
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:8096".into())
            .trim_end_matches('/')
            .to_string();
        Ok(Self { base_url, token })
    }
}

// ponytail: tiny parser (KEY=VALUE, `#` comments, optional quotes, no expansion); dotenvy if more is needed.
fn load_env_file(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for (k, v) in parse_env(&text) {
        if std::env::var_os(&k).is_none() {
            // SAFETY: called once at startup before other threads read the environment.
            unsafe { std::env::set_var(k, v) };
        }
    }
}

fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("export ").unwrap_or(l);
            if l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            let v = v.trim();
            let v = ['"', '\'']
                .iter()
                .find_map(|q| v.strip_prefix(*q)?.strip_suffix(*q))
                .unwrap_or(v);
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_token() {
        let c = Config::from_vars(None, Some("s3cret-token".into())).unwrap();
        let d = format!("{c:?}");
        assert!(!d.contains("s3cret-token") && d.contains("redacted"));
    }

    #[test]
    fn defaults_and_trim() {
        let c = Config::from_vars(None, Some("t".into())).unwrap();
        assert_eq!(c.base_url, "http://127.0.0.1:8096");
        let c = Config::from_vars(Some("http://h:1/".into()), Some("t".into())).unwrap();
        assert_eq!(c.base_url, "http://h:1");
        assert!(Config::from_vars(None, None).is_err());
        assert!(Config::from_vars(None, Some(String::new())).is_err());
    }

    #[test]
    fn env_parser() {
        let v = parse_env("# c\nA=1\nexport B=\"two\"\n C = 'x y' \nbad\n");
        assert_eq!(
            v,
            [("A", "1"), ("B", "two"), ("C", "x y")].map(|(a, b)| (a.to_string(), b.to_string()))
        );
    }
}
