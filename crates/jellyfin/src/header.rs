//! Optional extra HTTP header per server (for a gating reverse proxy). The value may be a secret:
//! it is kept by the caller in the keyring and only lives here in memory.
//!
//! ponytail: a process-wide map keyed by server URL, read by every client builder (this crate,
//! `jellyfin-user`, `jellyfin-admin`), instead of threading a parameter through ~15 call sites.
//! Replace with an explicit client option if a second consumer with different needs appears.
use reqwest::header::{HeaderName, HeaderValue};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Headers that would break or hijack the request; the token header is never overridable.
const FORBIDDEN: [&str; 8] = [
    "authorization",
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "connection",
    "upgrade",
    "cookie",
];

static REGISTRY: LazyLock<RwLock<HashMap<String, (HeaderName, HeaderValue)>>> =
    LazyLock::new(Default::default);

fn key(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_ascii_lowercase()
}

/// Parse and validate a `Name: value` line.
pub fn parse(line: &str) -> Result<(HeaderName, HeaderValue), &'static str> {
    let (name, value) = line.split_once(':').ok_or("Use the form Name: value")?;
    let (name, value) = (name.trim(), value.trim());
    if value.is_empty() {
        return Err("The header needs a value");
    }
    let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| "Not a valid header name")?;
    if FORBIDDEN.contains(&name.as_str()) {
        return Err("That header is set by NOCTURNE itself");
    }
    let mut value = HeaderValue::from_str(value).map_err(|_| "Not a valid header value")?;
    value.set_sensitive(true);
    Ok((name, value))
}

/// Use `line` (`Name: value`) for every client of `base_url`, or none with `None`.
/// An invalid line clears the entry and is reported.
pub fn set(base_url: &str, line: Option<&str>) -> Result<(), &'static str> {
    let mut map = REGISTRY.write().unwrap();
    map.remove(&key(base_url));
    if let Some(l) = line {
        map.insert(key(base_url), parse(l)?);
    }
    Ok(())
}

/// The header registered for `base_url`, for a client builder.
pub fn for_url(base_url: &str) -> Option<(HeaderName, HeaderValue)> {
    REGISTRY.read().unwrap().get(&key(base_url)).cloned()
}

/// `Name: value` for mpv's `http-header-fields` (a secret: never log it).
pub fn line_for(base_url: &str) -> Option<String> {
    let (n, v) = for_url(base_url)?;
    Some(format!("{}: {}", n, v.to_str().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_validates() {
        let (n, v) = parse("X-Gate:  s3cret ").unwrap();
        assert_eq!((n.as_str(), v.to_str().unwrap()), ("x-gate", "s3cret"));
        assert!(v.is_sensitive());
        for bad in [
            "nocolon",
            "X-A:",
            ": v",
            "bad name: v",
            "Authorization: x",
            "Host: x",
            "X-A: a\nb",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn registry_is_per_server_and_clearable() {
        set("http://gate.test:8096/", Some("X-Gate: 1")).unwrap();
        assert!(for_url("http://GATE.test:8096").is_some());
        assert!(for_url("http://other.test:8096").is_none());
        assert_eq!(
            line_for("http://gate.test:8096").as_deref(),
            Some("x-gate: 1")
        );
        assert!(set("http://gate.test:8096", Some("bad")).is_err());
        assert!(for_url("http://gate.test:8096").is_none(), "invalid clears");
        set("http://gate.test:8096", None).unwrap();
    }
}
