//! Logging with token redaction. Use [`glog!`] for every line the gateway prints: it removes `ApiKey=`, `api_key=`, `X-Emby-Token=` and `Token="..."` values before anything reaches stderr. Never log a request URI, header map or `Config` through any other path.

const KEYS: [&str; 5] = [
    "apikey=",
    "api_key=",
    "x-emby-token=",
    "access_token=",
    "token=",
];

/// Replace the value after each secret key with `<redacted>`. Values end at `&`, whitespace, a comma or a quote.
pub fn redact(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < s.len() {
        for k in KEYS {
            if lower[i..].starts_with(k) {
                out.push_str(&s[i..i + k.len()]);
                let mut j = i + k.len();
                if s[j..].starts_with('"') {
                    out.push('"');
                    j += 1;
                }
                out.push_str("<redacted>");
                while j < s.len()
                    && !matches!(s.as_bytes()[j], b'&' | b'"' | b',' | b' ' | b'\n' | b'\t')
                {
                    j += 1;
                }
                i = j;
                continue 'outer;
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

pub fn line(msg: &str) {
    eprintln!("nocturne-gateway: {}", redact(msg));
}

#[macro_export]
macro_rules! glog {
    ($($a:tt)*) => { $crate::log::line(&format!($($a)*)) };
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn removes_every_token_spelling() {
        for (i, o) in [
            (
                "GET /Items?ApiKey=abc123&x=1",
                "GET /Items?ApiKey=<redacted>&x=1",
            ),
            ("/x?api_key=deadbeef", "/x?api_key=<redacted>"),
            ("/x?X-Emby-Token=zz&a=b", "/x?X-Emby-Token=<redacted>&a=b"),
            (
                r#"MediaBrowser Client="a", Token="secret", Version="1""#,
                r#"MediaBrowser Client="a", Token="<redacted>", Version="1""#,
            ),
            ("/x?APIKEY=UP", "/x?APIKEY=<redacted>"),
            ("plain text ünïcode", "plain text ünïcode"),
        ] {
            assert_eq!(redact(i), o);
        }
        assert!(!redact("a?ApiKey=SECRET&b=1&api_key=SECRET2").contains("SECRET"));
    }
}
