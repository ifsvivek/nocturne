//! Naming templates with Radarr / Sonarr-style tokens: `{Movie Title}`, `{season:00}`. A `/` in the template separates folders; a `/` inside a value never does.
use std::path::PathBuf;

/// One path component made safe: no separators, no characters Windows or Samba refuse, no control characters, no leading dots (so never `.` / `..` or a hidden file), `:` as ` -`.
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | '\\' => out.push('-'),
            ':' => out.push_str(" -"),
            '*' | '?' | '"' | '<' | '>' | '|' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.trim_start_matches(['.', ' ']).to_string()
}

/// Tidy one rendered component: drop brackets left empty by missing tokens, collapse spaces, trim separators at the ends.
fn tidy(s: &str) -> String {
    let mut s = s.to_string();
    for empty in ["()", "[]", "{}"] {
        while s.contains(empty) {
            s = s.replace(empty, "");
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.replace(" - - ", " - ");
    s.trim_matches(|c: char| c == ' ' || c == '-' || c == '.')
        .to_string()
}

/// Render `template` with `value(token)`. `{name:00}` pads a number to that many digits. Unknown tokens render empty.
pub fn render(template: &str, value: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let mut parts = Vec::new();
    for seg in template.split('/') {
        let mut out = String::new();
        let mut rest = seg;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            let tok = &rest[open + 1..open + close];
            let (name, pad) = tok.split_once(':').unwrap_or((tok, ""));
            if let Some(v) = value(name) {
                let v = match v.parse::<u64>() {
                    Ok(n) if !pad.is_empty() => format!("{n:0w$}", w = pad.len()),
                    _ => v,
                };
                out.push_str(&sanitize(&v));
            }
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        let c = tidy(&sanitize(&out));
        if !c.is_empty() {
            parts.push(c);
        }
    }
    parts.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals<'a>(kv: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| kv.iter().find(|(a, _)| *a == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn movie_and_episode_templates() {
        let v = vals(&[
            ("Movie Title", "Alien: Romulus"),
            ("Release Year", "2024"),
            ("Quality Full", "Bluray-1080p"),
        ]);
        assert_eq!(
            render(
                "{Movie Title} ({Release Year})/{Movie Title} ({Release Year}) {Quality Full}",
                &v
            ),
            PathBuf::from("Alien - Romulus (2024)/Alien - Romulus (2024) Bluray-1080p")
        );
        let v = vals(&[
            ("Series Title", "Show"),
            ("season", "1"),
            ("episode", "2"),
            ("Quality Full", "WEBDL-1080p"),
        ]);
        assert_eq!(
            render(
                "Season {season:00}/{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}",
                &v
            ),
            PathBuf::from("Season 01/Show - S01E02 - WEBDL-1080p")
        );
    }

    #[test]
    fn missing_tokens_and_hostile_values_cannot_escape() {
        let v = vals(&[("Artist Name", "AC/DC"), ("Album Title", "../../etc")]);
        assert_eq!(
            render("{Artist Name}/{Album Title} ({Release Year})", &v),
            PathBuf::from("AC-DC/etc")
        );
        let v = vals(&[("x", "..")]);
        assert_eq!(render("{x}/{x}", &v), PathBuf::new());
        assert_eq!(sanitize(".hidden"), "hidden");
        assert_eq!(sanitize("a*b?c\"d<e>f|g\u{0}"), "abcdefg");
        assert_eq!(render("{unknown} - x", &vals(&[])), PathBuf::from("x"));
    }
}
