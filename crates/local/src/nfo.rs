//! Kodi `.nfo` reader: a small tag scanner, enough for movie / tvshow / episodedetails files.
//! ponytail: not a full XML parser (no same-name nesting, no namespaces); swap for quick-xml if real files need it.

use crate::{Meta, ProviderIds};
use regex::Regex;
use std::sync::LazyLock;

/// All `<name ...>body</name>` occurrences as (attributes, raw body).
fn tags<'a>(xml: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let (open, close) = (format!("<{name}"), format!("</{name}>"));
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = xml[pos..].find(&open) {
        let s = pos + i + open.len();
        if !matches!(
            xml.as_bytes().get(s),
            Some(b'>' | b' ' | b'/' | b'\t' | b'\n' | b'\r')
        ) {
            pos = s;
            continue;
        }
        let Some(gt) = xml[s..].find('>') else { break };
        let attrs = &xml[s..s + gt];
        let body = s + gt + 1;
        if attrs.ends_with('/') {
            pos = body;
            continue;
        }
        let Some(e) = xml[body..].find(&close) else {
            break;
        };
        out.push((attrs.trim(), &xml[body..body + e]));
        pos = body + e + close.len();
    }
    out
}

fn text(raw: &str) -> String {
    static ENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&(#x?[0-9a-fA-F]+|[a-z]+);").unwrap());
    let t = raw.trim();
    let t = t
        .strip_prefix("<![CDATA[")
        .and_then(|t| t.strip_suffix("]]>"))
        .map(|c| c.to_string())
        .unwrap_or_else(|| {
            ENT.replace_all(t, |c: &regex::Captures| match &c[1] {
                "lt" => "<".into(),
                "gt" => ">".into(),
                "quot" => "\"".into(),
                "apos" => "'".into(),
                "amp" => "&".into(),
                n => n
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32)
                    .map(String::from)
                    .unwrap_or_else(|| c[0].to_string()),
            })
            .into_owned()
        });
    t.trim().to_string()
}

fn first(xml: &str, name: &str) -> Option<String> {
    tags(xml, name)
        .into_iter()
        .map(|(_, b)| text(b))
        .find(|t| !t.is_empty())
}

fn leading_num<T: std::str::FromStr>(s: &str) -> Option<T> {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    s[..end].parse().ok()
}

fn attr<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let i = attrs.find(&format!("{key}=\""))? + key.len() + 2;
    attrs[i..].split('"').next()
}

pub fn parse(xml: &str) -> Meta {
    let year = first(xml, "year")
        .and_then(|y| leading_num(&y))
        .or_else(|| {
            ["premiered", "releasedate", "aired"]
                .iter()
                .find_map(|t| first(xml, t))
                .and_then(|d| d.get(..4).and_then(|y| y.parse().ok()))
        });
    let rating = tags(xml, "rating").into_iter().find_map(|(_, body)| {
        let v = first(body, "value").unwrap_or_else(|| text(body));
        leading_num(&v)
    });
    let mut ids = ProviderIds::default();
    for (attrs, body) in tags(xml, "uniqueid") {
        let v = text(body);
        match attr(attrs, "type").map(str::to_lowercase).as_deref() {
            Some("imdb") => ids.imdb = Some(v),
            Some("tmdb") => ids.tmdb = Some(v),
            Some("tvdb") => ids.tvdb = Some(v),
            _ => {}
        }
    }
    for (name, slot) in [
        ("imdbid", &mut ids.imdb),
        ("tmdbid", &mut ids.tmdb),
        ("tvdbid", &mut ids.tvdb),
    ] {
        if slot.is_none() {
            *slot = first(xml, name);
        }
    }
    if let Some(id) = first(xml, "id")
        && id.starts_with("tt")
        && ids.imdb.is_none()
    {
        ids.imdb = Some(id);
    }
    Meta {
        title: first(xml, "title"),
        year,
        plot: first(xml, "plot").or_else(|| first(xml, "outline")),
        genres: tags(xml, "genre")
            .into_iter()
            .map(|(_, b)| text(b))
            .filter(|g| !g.is_empty())
            .collect(),
        runtime_min: first(xml, "runtime")
            .and_then(|r| leading_num::<f32>(&r))
            .map(|m| m as u32),
        rating,
        ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_nfo() {
        let m = parse(
            r#"<?xml version="1.0"?>
            <movie><title>Tom &amp; Jerry</title><year>1992</year>
            <plot><![CDATA[Cat <chases> mouse.]]></plot>
            <genre>Animation</genre><genre>Comedy</genre><runtime>92 min</runtime>
            <ratings><rating name="imdb" max="10"><value>7.5</value><votes>10</votes></rating></ratings>
            <uniqueid type="imdb" default="true">tt0104940</uniqueid><uniqueid type="tmdb">123</uniqueid>
            </movie>"#,
        );
        assert_eq!(m.title.as_deref(), Some("Tom & Jerry"));
        assert_eq!(m.year, Some(1992));
        assert_eq!(m.plot.as_deref(), Some("Cat <chases> mouse."));
        assert_eq!(m.genres, ["Animation", "Comedy"]);
        assert_eq!(m.runtime_min, Some(92));
        assert_eq!(m.rating, Some(7.5));
        assert_eq!(m.ids.imdb.as_deref(), Some("tt0104940"));
        assert_eq!(m.ids.tmdb.as_deref(), Some("123"));
    }

    #[test]
    fn legacy_tags_and_premiered() {
        let m = parse(
            "<tvshow><title>Show</title><premiered>2008-01-20</premiered><rating>8.1</rating>\
             <id>tt0903747</id><tvdbid>81189</tvdbid><empty/></tvshow>",
        );
        assert_eq!(m.year, Some(2008));
        assert_eq!(m.rating, Some(8.1));
        assert_eq!(m.ids.imdb.as_deref(), Some("tt0903747"));
        assert_eq!(m.ids.tvdb.as_deref(), Some("81189"));
    }

    #[test]
    fn garbage_is_empty() {
        let m = parse("not xml <title>unclosed");
        assert_eq!(m.title, None);
    }
}
