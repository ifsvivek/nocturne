//! Torznab/Newznab XML: the caps document and RSS result pages. Pure, so it is tested with fixtures.

use crate::indexer::{Caps, Category, Page};
use crate::{Error, Protocol, Release, Result};
use roxmltree::{Document, Node};

fn doc(xml: &str) -> Result<Document<'_>> {
    let d = Document::parse(xml).map_err(|e| Error::Parse(e.to_string()))?;
    let root = d.root_element();
    if root.tag_name().name() == "error" {
        return Err(Error::Indexer {
            code: root
                .attribute("code")
                .and_then(|c| c.parse().ok())
                .unwrap_or(0),
            description: root
                .attribute("description")
                .unwrap_or_default()
                .to_string(),
        });
    }
    Ok(d)
}

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| c.tag_name().name() == name)
}

fn text<'a>(n: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    child(n, name)
        .and_then(|c| c.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn params(n: Option<Node>) -> Option<Vec<String>> {
    let n = n?;
    if n.attribute("available") != Some("yes") {
        return None;
    }
    let p = n.attribute("supportedParams").unwrap_or("q");
    Some(
        p.split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

fn category(n: Node) -> Option<Category> {
    Some(Category {
        id: n.attribute("id")?.parse().ok()?,
        name: n.attribute("name").unwrap_or_default().to_string(),
        subcats: n
            .children()
            .filter(|c| c.tag_name().name() == "subcat")
            .filter_map(category)
            .collect(),
    })
}

/// Parse a `t=caps` document.
pub fn parse_caps(xml: &str) -> Result<Caps> {
    let d = doc(xml)?;
    let root = d.root_element();
    if root.tag_name().name() != "caps" {
        return Err(Error::Parse("not a caps document".into()));
    }
    let limits = child(root, "limits");
    let lim = |a| {
        limits
            .and_then(|l| l.attribute(a))
            .and_then(|v| v.parse().ok())
    };
    let searching = child(root, "searching");
    let mode = |name| params(searching.and_then(|s| child(s, name)));
    Ok(Caps {
        limit_max: lim("max"),
        limit_default: lim("default"),
        search: mode("search"),
        tv: mode("tv-search"),
        movie: mode("movie-search"),
        // `audio-search` is the older name some feeds still use.
        music: mode("music-search").or_else(|| mode("audio-search")),
        book: mode("book-search"),
        categories: child(root, "categories")
            .map(|c| {
                c.children()
                    .filter(|n| n.tag_name().name() == "category")
                    .filter_map(category)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// RFC 2822 date (`Sat, 18 May 2024 10:00:00 +0200`) to Unix seconds.
pub(crate) fn rfc2822(s: &str) -> Option<i64> {
    let s = s.split_once(',').map_or(s, |(_, r)| r);
    let mut it = s.split_whitespace();
    let day: i64 = it.next()?.parse().ok()?;
    let mon = it.next()?.get(..3)?.to_ascii_lowercase();
    let month = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|m| *m == mon)? as i64
        + 1;
    let year: i64 = it.next()?.parse().ok()?;
    let mut hms = it.next()?.split(':').map(|p| p.parse::<i64>().ok());
    let (h, mi, sec) = (
        hms.next()??,
        hms.next()??,
        hms.next().flatten().unwrap_or(0),
    );
    let off = match it.next() {
        Some(z) if z.len() == 5 && (z.starts_with('+') || z.starts_with('-')) => {
            let v: i64 = z[1..].parse().ok()?;
            let secs = (v / 100) * 3600 + (v % 100) * 60;
            if z.starts_with('-') { -secs } else { secs }
        }
        _ => 0,
    };
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + mi * 60 + sec - off)
}

fn imdb(v: &str) -> Option<String> {
    let digits = v.trim().trim_start_matches("tt");
    (!digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.trim_start_matches('0').is_empty())
    .then(|| format!("tt{digits:0>7}"))
}

/// Info hash from a magnet's `xt=urn:btih:` (hex form only; base32 hashes are left to the torrent crate).
fn magnet_hash(m: &str) -> Option<String> {
    let i = m.to_ascii_lowercase().find("urn:btih:")? + 9;
    let h: String = m[i..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

fn item(n: Node, indexer_id: i64, protocol: Protocol) -> Option<Release> {
    let mut r = Release {
        title: text(n, "title")?.to_string(),
        indexer_id,
        protocol,
        info_url: text(n, "comments").map(str::to_string),
        publish_date: text(n, "pubDate").and_then(rfc2822),
        size: text(n, "size").and_then(|s| s.parse().ok()),
        grabs: text(n, "grabs").and_then(|s| s.parse().ok()),
        ..Release::default()
    };
    let link = text(n, "link").map(str::to_string);
    if let Some(e) = child(n, "enclosure") {
        r.download_url = e.attribute("url").map(str::to_string);
        if r.size.is_none() {
            r.size = e
                .attribute("length")
                .and_then(|s| s.parse().ok())
                .filter(|&s| s > 0);
        }
    }
    if r.download_url.is_none() {
        r.download_url = link.clone();
    }
    let mut peers = None;
    for c in n.children().filter(|c| c.is_element()) {
        match c.tag_name().name() {
            "category" => {
                if let Some(id) = c.text().and_then(|t| t.trim().parse().ok()) {
                    r.categories.push(id);
                }
            }
            "attr" => {
                let (Some(k), Some(v)) = (c.attribute("name"), c.attribute("value")) else {
                    continue;
                };
                let num = || v.trim().parse::<u64>().ok();
                match k.to_ascii_lowercase().as_str() {
                    "size" => r.size = r.size.or(num()),
                    "seeders" => r.seeders = num().map(|n| n as u32),
                    "leechers" => r.leechers = num().map(|n| n as u32),
                    "peers" => peers = num().map(|n| n as u32),
                    "grabs" => r.grabs = num().map(|n| n as u32),
                    "infohash" => r.info_hash = Some(v.trim().to_ascii_lowercase()),
                    "magneturl" => r.magnet = Some(v.trim().to_string()),
                    "imdb" | "imdbid" => r.imdb_id = imdb(v),
                    "tmdbid" => r.tmdb_id = num().map(|n| n as u32).filter(|&n| n > 0),
                    "tvdbid" => r.tvdb_id = num().map(|n| n as u32).filter(|&n| n > 0),
                    "category" => {
                        if let Some(id) = num() {
                            r.categories.push(id as u32);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // Torznab `peers` counts seeders too.
    if r.leechers.is_none() {
        r.leechers = peers.map(|p| p.saturating_sub(r.seeders.unwrap_or(0)));
    }
    if r.download_url
        .as_deref()
        .is_some_and(|u| u.starts_with("magnet:"))
    {
        r.magnet = r.magnet.or(r.download_url.take());
    }
    if r.info_hash.is_none() {
        r.info_hash = r.magnet.as_deref().and_then(magnet_hash);
    }
    r.categories.sort_unstable();
    r.categories.dedup();
    r.guid = text(n, "guid")
        .map(str::to_string)
        .or_else(|| r.info_hash.clone())
        .or(link)
        .or_else(|| r.download_url.clone())
        .unwrap_or_else(|| r.title.clone());
    Some(r)
}

/// Parse an RSS result page (`t=search|tvsearch|movie|music|book`). Items without a title or anything to download are dropped.
pub fn parse_results(xml: &str, indexer_id: i64, protocol: Protocol) -> Result<Page> {
    let d = doc(xml)?;
    let channel =
        child(d.root_element(), "channel").ok_or_else(|| Error::Parse("no RSS channel".into()))?;
    let resp = child(channel, "response");
    let attr = |a| {
        resp.and_then(|r| r.attribute(a))
            .and_then(|v| v.parse().ok())
    };
    let releases: Vec<Release> = channel
        .children()
        .filter(|c| c.tag_name().name() == "item")
        .filter_map(|n| item(n, indexer_id, protocol))
        .filter(|r| r.download_url.is_some() || r.magnet.is_some())
        .collect();
    Ok(Page {
        offset: attr("offset").unwrap_or(0),
        total: attr("total"),
        count: releases.len() as u32,
        releases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(rfc2822("Thu, 01 Jan 1970 00:00:00 +0000"), Some(0));
        assert_eq!(
            rfc2822("Sat, 18 May 2024 10:00:00 +0200"),
            Some(1_716_019_200)
        );
        assert_eq!(rfc2822("18 May 2024 08:00:00 GMT"), Some(1_716_019_200));
        assert_eq!(rfc2822("garbage"), None);
    }

    #[test]
    fn ids() {
        assert_eq!(imdb("0133093").as_deref(), Some("tt0133093"));
        assert_eq!(imdb("tt10872600").as_deref(), Some("tt10872600"));
        assert_eq!(imdb("0"), None);
        assert_eq!(
            magnet_hash("magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=x")
                .as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
    }
}
