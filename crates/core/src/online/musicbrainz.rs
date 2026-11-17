//! MusicBrainz (albums: year, genres/tags, ids) and the Cover Art Archive (front cover). No key. MusicBrainz
//! needs a descriptive User-Agent (see `http::user_agent`) and about one request per second.

use super::http::{Err, Gate, Http};
use super::matching::Hit;
use super::{Endpoints, Fetched};
use nocturne_local::{OnlineMeta, now_secs};
use serde_json::Value;

pub const NAME: &str = "musicbrainz";

/// Escape for a Lucene phrase.
fn phrase(s: &str) -> String {
    s.replace(['\\', '"'], " ")
}

pub fn parse_search(v: &Value) -> Vec<Hit> {
    v["release-groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let artist = g["artist-credit"][0]["name"].as_str().unwrap_or_default();
            Some(Hit {
                id: g["id"].as_str()?.into(),
                title: g["title"].as_str()?.into(),
                year: g["first-release-date"]
                    .as_str()
                    .and_then(|d| d.get(..4)?.parse().ok()),
                overview: artist.into(),
            })
        })
        .collect()
}

/// A release group lookup (`?inc=tags`), cover art added by the caller.
pub fn parse_group(v: &Value) -> Option<Fetched> {
    let id = v["id"].as_str()?.to_string();
    let mut tags: Vec<(i64, String)> = v["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some((
                t["count"].as_i64().unwrap_or(0),
                t["name"].as_str()?.to_string(),
            ))
        })
        .collect();
    tags.sort_by_key(|a| std::cmp::Reverse(a.0));
    Some(Fetched {
        meta: OnlineMeta {
            provider: NAME.into(),
            fetched: now_secs(),
            title: v["title"].as_str().map(String::from),
            year: v["first-release-date"]
                .as_str()
                .and_then(|d| d.get(..4)?.parse().ok()),
            genres: tags.into_iter().take(3).map(|t| t.1).collect(),
            native_id: Some(id),
            ..Default::default()
        },
        poster: None,
        backdrop: None,
    })
}

pub struct MusicBrainz<'a> {
    pub http: &'a Http,
    pub gate: &'a Gate,
    pub caa_gate: &'a Gate,
    pub ep: &'a Endpoints,
}

impl MusicBrainz<'_> {
    pub async fn search(&self, artist: &str, album: &str) -> Result<Vec<Hit>, Err> {
        let q = format!(
            "releasegroup:\"{}\" AND artist:\"{}\"",
            phrase(album),
            phrase(artist)
        );
        let rb = self
            .http
            .get(&format!("{}/release-group", self.ep.musicbrainz))
            .query(&[("query", q.as_str()), ("fmt", "json"), ("limit", "6")]);
        Ok(parse_search(&self.http.json(self.gate, rb).await?))
    }

    pub async fn group(&self, id: &str) -> Result<Fetched, Err> {
        let rb = self
            .http
            .get(&format!("{}/release-group/{id}", self.ep.musicbrainz))
            .query(&[("inc", "tags"), ("fmt", "json")]);
        parse_group(&self.http.json(self.gate, rb).await?).ok_or(Err::Failed)
    }

    /// The front cover (a 404 is simply "no cover").
    pub async fn cover(&self, id: &str) -> Option<Vec<u8>> {
        let rb = self
            .http
            .get(&format!("{}/release-group/{id}/front-500", self.ep.caa));
        self.http.bytes(self.caa_gate, rb, 8 << 20).await.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_search_and_group() {
        let s = json!({"release-groups":[{"id":"aaaa-1","score":100,"title":"Abbey Road","first-release-date":"1969-09-26",
            "primary-type":"Album","artist-credit":[{"name":"The Beatles"}]},{"title":"no id"}]});
        let h = parse_search(&s);
        assert_eq!(h.len(), 1);
        assert_eq!(
            (h[0].year, h[0].overview.as_str()),
            (Some(1969), "The Beatles")
        );
        let g = json!({"id":"aaaa-1","title":"Abbey Road","first-release-date":"1969-09-26",
            "tags":[{"count":1,"name":"pop"},{"count":9,"name":"rock"},{"count":4,"name":"british"},{"count":2,"name":"x"}]});
        let f = parse_group(&g).unwrap();
        assert_eq!(f.meta.genres, ["rock", "british", "x"]);
        assert_eq!(f.meta.native_id.as_deref(), Some("aaaa-1"));
    }
}
