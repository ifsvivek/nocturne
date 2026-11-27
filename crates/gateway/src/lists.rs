//! Merged lists (plan section 2): the pure parts of scatter-gather. A list query goes to every source with `StartIndex=0` and `Limit=start+limit`, the answers are merge-sorted by the requested key, copies of one title are grouped by strong identity (provider ids), and the page is sliced from the merged order.
//! Nothing here does I/O except [`PageCache`], a 30 second cache of per-source answers.
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// ponytail: deep offsets cost O(offset) per server (every source is asked for `start + limit` items and the merge happens here), so a request past this many items gets the items up to the ceiling only.
/// Upgrade path: a persisted light index (id, type, sort name, year, dates, provider ids, genres, played) refreshed by paging `/Items`, with all list endpoints answered from it. Measure before building it.
pub const MAX_OVERFETCH: usize = 2000;
/// Page size clamp (plan section 5, malicious metadata and abuse).
pub const MAX_PAGE: usize = 500;
const CACHE_TTL: Duration = Duration::from_secs(30);
const CACHE_MAX: usize = 256;

// ---------------------------------------------------------------- queries

/// A parsed query string: names compare case-insensitively, values are percent-decoded.
#[derive(Clone, Debug, Default)]
pub struct Query(pub Vec<(String, String)>);

impl Query {
    pub fn parse(q: Option<&str>) -> Self {
        Self(
            q.unwrap_or("")
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|p| {
                    let (k, v) = p.split_once('=').unwrap_or((p, ""));
                    (
                        crate::auth::pct_decode(k, true),
                        crate::auth::pct_decode(v, true),
                    )
                })
                .collect(),
        )
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }

    pub fn csv(&self, k: &str) -> Vec<String> {
        self.get(k)
            .map(|v| {
                v.split([',', '|'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn usize(&self, k: &str) -> Option<usize> {
        self.get(k).and_then(|v| v.parse().ok())
    }

    pub fn flag(&self, k: &str) -> Option<bool> {
        self.get(k).map(|v| v.eq_ignore_ascii_case("true"))
    }
}

/// Percent-encode a query component (everything but the unreserved characters).
pub fn enc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

pub fn encode_query(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The window a client asked for and what to request from each source to be able to build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: usize,
    /// `None`: no limit given, the page clamp applies.
    pub limit: Option<usize>,
}

impl Window {
    pub fn of(q: &Query) -> Self {
        Self {
            start: q.usize("StartIndex").unwrap_or(0),
            limit: q.usize("Limit"),
        }
    }
    pub fn size(&self) -> usize {
        self.limit.unwrap_or(MAX_PAGE).min(MAX_PAGE)
    }
    /// `Limit` for every source: everything up to the end of the window, capped by [`MAX_OVERFETCH`].
    pub fn fetch(&self) -> usize {
        (self.start + self.size()).min(MAX_OVERFETCH)
    }
    pub fn slice<T>(&self, v: Vec<T>) -> Vec<T> {
        v.into_iter().skip(self.start).take(self.size()).collect()
    }
}

// ---------------------------------------------------------------- sorting

#[derive(Clone, Debug, Default)]
pub struct Sort(Vec<(String, bool)>);

impl Sort {
    /// From `SortBy` (comma list) and `SortOrder` (one value for all keys or one per key). No `SortBy`: by `SortName`.
    pub fn of(q: &Query) -> Self {
        let mut keys = q.csv("SortBy");
        if keys.is_empty() {
            keys.push("SortName".into());
        }
        let orders = q.csv("SortOrder");
        Self(
            keys.into_iter()
                .enumerate()
                .map(|(i, k)| {
                    let o = orders.get(i).or(orders.first());
                    (k, o.is_some_and(|o| o.eq_ignore_ascii_case("Descending")))
                })
                .collect(),
        )
    }
    pub fn is_random(&self) -> bool {
        self.0.iter().any(|(k, _)| k.eq_ignore_ascii_case("Random"))
    }
}

enum Key {
    Text(String),
    Num(f64),
}

fn num(v: &Value, f: &str) -> Key {
    Key::Num(
        v.get(f)
            .and_then(Value::as_f64)
            .unwrap_or(f64::NEG_INFINITY),
    )
}

fn text(v: &Value, f: &str) -> Key {
    Key::Text(v.get(f).and_then(Value::as_str).unwrap_or("").to_string())
}

fn key_of(v: &Value, k: &str) -> Key {
    let ud = |f: &str| {
        v.get("UserData")
            .map_or(Value::Null, |u| u.get(f).cloned().unwrap_or(Value::Null))
    };
    match k.to_ascii_lowercase().as_str() {
        "sortname" | "name" | "seriessortname" => Key::Text(
            v.get("SortName")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or(v.get("Name").and_then(Value::as_str))
                .unwrap_or("")
                .to_lowercase(),
        ),
        "datecreated" | "datelastcontentadded" => text(v, "DateCreated"),
        "premieredate" => text(v, "PremiereDate"),
        "dateplayed" => Key::Text(ud("LastPlayedDate").as_str().unwrap_or("").to_string()),
        "productionyear" => num(v, "ProductionYear"),
        "communityrating" => num(v, "CommunityRating"),
        "criticrating" => num(v, "CriticRating"),
        "runtime" | "runtimeticks" => num(v, "RunTimeTicks"),
        "playcount" => Key::Num(ud("PlayCount").as_f64().unwrap_or(0.0)),
        "indexnumber" => num(v, "IndexNumber"),
        "parentindexnumber" => num(v, "ParentIndexNumber"),
        "random" => {
            let id = v.get("Id").and_then(Value::as_str).unwrap_or("");
            let h = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
            });
            Key::Num((h % 1_000_003) as f64)
        }
        _ => Key::Text(String::new()),
    }
}

pub fn compare(a: &Value, b: &Value, s: &Sort) -> Ordering {
    for (k, desc) in &s.0 {
        let o = match (key_of(a, k), key_of(b, k)) {
            (Key::Text(x), Key::Text(y)) => x.cmp(&y),
            (Key::Num(x), Key::Num(y)) => x.total_cmp(&y),
            _ => Ordering::Equal,
        };
        let o = if *desc { o.reverse() } else { o };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

// ---------------------------------------------------------------- identity and grouping

/// One source's copy of an item, before merging. `dto` holds the item as the source sent it (rewritten except its top-level `Id` and `MediaSources`).
#[derive(Clone, Debug)]
pub struct Entry {
    /// Position of the source in priority order (lower is preferred).
    pub src: usize,
    pub label: String,
    /// The source's id-map key (`"local"` for the local library).
    pub src_key: String,
    pub local: bool,
    /// The item's id on that source (for the local library `"<source id>:<local id>"`).
    pub up_id: String,
    /// `host[:port]` of the server (empty for local), for rewriting its absolute URLs.
    pub authority: String,
    pub dto: Value,
}

const PRIORITY: [&str; 3] = ["tmdb", "imdb", "tvdb"];

/// Strong identity keys of an item: kind plus one provider id each (`movie:tmdb:603`). Only movies, series and episodes merge; nothing merges on title alone.
pub fn identity_keys(dto: &Value) -> Vec<String> {
    let kind = dto.get("Type").and_then(Value::as_str).unwrap_or("");
    if !matches!(kind, "Movie" | "Series" | "Episode") {
        return vec![];
    }
    let mut out = vec![];
    if let Some(p) = dto.get("ProviderIds").and_then(Value::as_object) {
        for (k, v) in p {
            let (k, v) = (
                k.to_ascii_lowercase(),
                v.as_str().unwrap_or("").trim().to_ascii_lowercase(),
            );
            if PRIORITY.contains(&k.as_str()) && !v.is_empty() {
                out.push(format!("{}:{k}:{v}", kind.to_ascii_lowercase()));
            }
        }
    }
    out
}

/// The key a merged title's id is made from: the first of tmdb, imdb, tvdb among the group's keys, so a copy without every provider id still lands on the same id.
pub fn canonical_key(keys: &[String]) -> Option<String> {
    PRIORITY
        .iter()
        .find_map(|p| {
            let mut m: Vec<&String> = keys
                .iter()
                .filter(|k| k.split(':').nth(1) == Some(p))
                .collect();
            m.sort();
            m.first().map(|s| (*s).clone())
        })
        .or_else(|| keys.first().cloned())
}

pub struct Group {
    pub copies: Vec<Entry>,
    /// Canonical identity key; `None` for an item that merges with nothing.
    pub key: Option<String>,
}

/// Group copies that share an identity key (directly or through a chain), keeping the order of first appearance and the order of copies inside a group. Items without keys are groups of one.
pub fn group(copies: Vec<Entry>) -> Vec<Group> {
    let mut groups: Vec<(Vec<Entry>, Vec<String>)> = vec![];
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for c in copies {
        let keys = identity_keys(&c.dto);
        let hits: Vec<usize> = {
            let mut h: Vec<usize> = keys.iter().filter_map(|k| by_key.get(k).copied()).collect();
            h.sort();
            h.dedup();
            h
        };
        let target = match hits.first() {
            None => {
                groups.push((vec![], vec![]));
                groups.len() - 1
            }
            Some(&t) => {
                // A copy that links two groups joins them into the earlier one.
                for &o in hits.iter().skip(1) {
                    let (cs, ks) = std::mem::take(&mut groups[o]);
                    for k in &ks {
                        by_key.insert(k.clone(), t);
                    }
                    groups[t].0.extend(cs);
                    groups[t].1.extend(ks);
                }
                t
            }
        };
        for k in keys {
            by_key.insert(k.clone(), target);
            if !groups[target].1.contains(&k) {
                groups[target].1.push(k);
            }
        }
        groups[target].0.push(c);
    }
    groups
        .into_iter()
        .filter(|(c, _)| !c.is_empty())
        .map(|(mut copies, keys)| {
            copies.sort_by_key(|c| c.src);
            Group {
                key: canonical_key(&keys),
                copies,
            }
        })
        .collect()
}

/// `TotalRecordCount` of a merged list: the sum of the sources' totals minus the copies merged away among the items seen (approximate on deep pages).
pub fn merged_total(sum: u64, seen: usize, groups: usize) -> u64 {
    sum.saturating_sub(seen.saturating_sub(groups) as u64)
}

// ---------------------------------------------------------------- cache

/// Per-source answers for 30 seconds, keyed by source and request, so a client paging through a list does not refetch.
#[derive(Default)]
pub struct PageCache(Mutex<HashMap<String, (Instant, Arc<Value>)>>);

impl PageCache {
    pub fn get(&self, key: &str) -> Option<Arc<Value>> {
        let m = self.0.lock().unwrap();
        m.get(key)
            .filter(|(t, _)| t.elapsed() < CACHE_TTL)
            .map(|(_, v)| v.clone())
    }
    pub fn put(&self, key: String, v: Arc<Value>) {
        let mut m = self.0.lock().unwrap();
        if m.len() >= CACHE_MAX {
            m.retain(|_, (t, _)| t.elapsed() < CACHE_TTL);
            if m.len() >= CACHE_MAX {
                m.clear();
            }
        }
        m.insert(key, (Instant::now(), v));
    }
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn copy(src: usize, v: Value) -> Entry {
        Entry {
            src,
            label: format!("S{src}"),
            src_key: format!("k{src}"),
            local: false,
            up_id: String::new(),
            authority: String::new(),
            dto: v,
        }
    }
    fn movie(name: &str, tmdb: Option<&str>, imdb: Option<&str>) -> Value {
        let mut p = serde_json::Map::new();
        if let Some(t) = tmdb {
            p.insert("Tmdb".into(), t.into());
        }
        if let Some(i) = imdb {
            p.insert("Imdb".into(), i.into());
        }
        json!({"Id": name, "Name": name, "Type": "Movie", "ProviderIds": p})
    }

    #[test]
    fn query_parse_and_window() {
        let q = Query::parse(Some(
            "sortby=SortName,DateCreated&SortOrder=Descending&StartIndex=40&limit=20&IncludeItemTypes=Movie%2CSeries",
        ));
        assert_eq!(q.csv("SortBy"), ["SortName", "DateCreated"]);
        assert_eq!(q.csv("includeitemtypes"), ["Movie", "Series"]);
        let w = Window::of(&q);
        assert_eq!((w.start, w.size(), w.fetch()), (40, 20, 60));
        let big = Window {
            start: 1900,
            limit: Some(10_000),
        };
        assert_eq!(big.size(), MAX_PAGE);
        assert_eq!(big.fetch(), MAX_OVERFETCH);
        assert_eq!(enc("a b&c"), "a%20b%26c");
    }

    #[test]
    fn merge_sort_by_name_date_and_direction() {
        let a = json!([{"Name":"b","SortName":"b"},{"Name":"d"}]);
        let b = json!([{"Name":"A"},{"Name":"c"}]);
        let mut all: Vec<Value> = a
            .as_array()
            .unwrap()
            .iter()
            .chain(b.as_array().unwrap())
            .cloned()
            .collect();
        let s = Sort::of(&Query::parse(Some("SortBy=SortName")));
        all.sort_by(|x, y| compare(x, y, &s));
        let names: Vec<_> = all.iter().map(|v| v["Name"].as_str().unwrap()).collect();
        assert_eq!(names, ["A", "b", "c", "d"], "case-insensitive");
        let s = Sort::of(&Query::parse(Some(
            "SortBy=DateCreated,SortName&SortOrder=Descending",
        )));
        let mut v = [
            json!({"Name":"x","DateCreated":"2024-01-01T00:00:00.0000000Z"}),
            json!({"Name":"y","DateCreated":"2025-01-01T00:00:00.0000000Z"}),
            json!({"Name":"z"}),
        ];
        v.sort_by(|x, y| compare(x, y, &s));
        assert_eq!(
            v.iter()
                .map(|v| v["Name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["y", "x", "z"]
        );
        let s = Sort::of(&Query::parse(Some("SortBy=CommunityRating,PlayCount")));
        assert_eq!(
            compare(
                &json!({"CommunityRating": 7.5}),
                &json!({"CommunityRating": 8.0}),
                &s
            ),
            Ordering::Less
        );
    }

    #[test]
    fn groups_merge_on_strong_identity_only() {
        let copies = vec![
            copy(0, movie("Heat", Some("949"), Some("tt0113277"))),
            copy(0, movie("Alien", None, None)),
            copy(1, movie("Heat (4K)", Some("949"), None)),
            copy(1, movie("Alien", None, None)),
            copy(1, movie("Other", None, Some("tt0113277"))),
        ];
        let g = group(copies);
        // Heat+Heat(4K)+Other chain through tmdb and imdb; the two title-only Alien stay apart.
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].copies.len(), 3);
        assert_eq!(g[0].copies[0].src, 0, "preferred copy first");
        assert_eq!(g[0].key.as_deref(), Some("movie:tmdb:949"));
        assert!(g[1].key.is_none() && g[1].copies.len() == 1);
        assert!(g[2].key.is_none() && g[2].copies.len() == 1);
        // Kinds never mix: a series with the same tmdb id is not the movie.
        let s = json!({"Type":"Series","Name":"s","ProviderIds":{"Tmdb":"949"}});
        assert_eq!(
            group(vec![copy(0, movie("Heat", Some("949"), None)), copy(1, s)]).len(),
            2
        );
        assert!(identity_keys(&json!({"Type":"Season","ProviderIds":{"Tmdb":"1"}})).is_empty());
    }

    #[test]
    fn canonical_key_is_stable_across_partial_copies() {
        let full = vec!["movie:imdb:tt1".to_string(), "movie:tmdb:5".to_string()];
        let partial = vec!["movie:tmdb:5".to_string()];
        assert_eq!(canonical_key(&full), canonical_key(&partial));
        assert_eq!(
            canonical_key(&["movie:imdb:tt1".to_string()]).as_deref(),
            Some("movie:imdb:tt1")
        );
    }

    #[test]
    fn paging_and_totals() {
        let w = Window {
            start: 2,
            limit: Some(3),
        };
        assert_eq!(w.slice((0..10).collect()), vec![2, 3, 4]);
        assert_eq!(w.slice(vec![0, 1, 2]), vec![2]);
        assert!(
            Window {
                start: 50,
                limit: Some(5)
            }
            .slice(vec![1])
            .is_empty()
        );
        // 10 + 8 items on two servers, 3 of the page's 12 seen items were duplicates.
        assert_eq!(merged_total(18, 12, 9), 15);
        assert_eq!(merged_total(2, 9, 1), 0);
    }

    #[test]
    fn cache_expires_and_clears() {
        let c = PageCache::default();
        c.put("a".into(), Arc::new(json!(1)));
        assert_eq!(*c.get("a").unwrap(), json!(1));
        assert!(c.get("b").is_none());
        c.clear();
        assert!(c.get("a").is_none());
    }
}
