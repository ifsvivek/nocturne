//! TVmaze (TV only, no key). Data CC BY-SA 4.0: credit shown on the folders page. Terms: docs/METADATA.md.

use super::http::{Err, Gate, Http};
use super::matching::Hit;
use super::{Endpoints, Fetched};
use nocturne_local::{OnlineMeta, ProviderIds, now_secs};
use serde_json::Value;

pub const NAME: &str = "tvmaze";

/// TVmaze summaries are small HTML fragments (`<p><b>..</b> ..</p>`): keep the text.
pub fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn text(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(strip_html).filter(|s| !s.is_empty())
}

fn year(v: &Value, k: &str) -> Option<u16> {
    v[k].as_str()?.get(..4)?.parse().ok()
}

fn show_hit(s: &Value) -> Option<Hit> {
    Some(Hit {
        id: s["id"].as_u64()?.to_string(),
        title: text(s, "name")?,
        year: year(s, "premiered"),
        overview: text(s, "summary").unwrap_or_default(),
    })
}

/// `/search/shows?q=` returns `[{score, show}]`.
pub fn parse_search(v: &Value) -> Vec<Hit> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| show_hit(&r["show"]))
        .collect()
}

pub fn parse_show(v: &Value) -> Option<Fetched> {
    let id = v["id"].as_u64()?.to_string();
    let img = &v["image"];
    Some(Fetched {
        meta: OnlineMeta {
            provider: NAME.into(),
            fetched: now_secs(),
            title: text(v, "name"),
            year: year(v, "premiered"),
            plot: text(v, "summary"),
            genres: v["genres"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| g.as_str().map(String::from))
                .collect(),
            runtime_min: v["runtime"]
                .as_u64()
                .or(v["averageRuntime"].as_u64())
                .map(|r| r as u32),
            rating: v["rating"]["average"].as_f64().map(|r| r as f32),
            ids: ProviderIds {
                imdb: v["externals"]["imdb"].as_str().map(String::from),
                tvdb: v["externals"]["thetvdb"].as_u64().map(|n| n.to_string()),
                tmdb: None,
            },
            native_id: Some(id),
            ..Default::default()
        },
        poster: img["original"]
            .as_str()
            .or(img["medium"].as_str())
            .map(String::from),
        backdrop: None,
    })
}

pub fn parse_episode(v: &Value) -> Option<Fetched> {
    v["id"].as_u64()?;
    Some(Fetched {
        meta: OnlineMeta {
            provider: NAME.into(),
            fetched: now_secs(),
            title: text(v, "name"),
            year: year(v, "airdate"),
            plot: text(v, "summary"),
            runtime_min: v["runtime"].as_u64().map(|r| r as u32),
            rating: v["rating"]["average"].as_f64().map(|r| r as f32),
            ..Default::default()
        },
        poster: None,
        backdrop: None,
    })
}

pub struct Tvmaze<'a> {
    pub http: &'a Http,
    pub gate: &'a Gate,
    pub ep: &'a Endpoints,
}

impl Tvmaze<'_> {
    async fn json(&self, path: &str, q: &[(&str, &str)]) -> Result<Value, Err> {
        self.http
            .json(
                self.gate,
                self.http.get(&format!("{}{path}", self.ep.tvmaze)).query(q),
            )
            .await
    }

    pub async fn search(&self, title: &str) -> Result<Vec<Hit>, Err> {
        Ok(parse_search(
            &self.json("/search/shows", &[("q", title)]).await?,
        ))
    }

    /// By an external id (`imdb` or `thetvdb`); a miss is an empty list.
    pub async fn lookup(&self, kind: &str, id: &str) -> Result<Vec<Hit>, Err> {
        match self.json("/lookup/shows", &[(kind, id)]).await {
            Ok(v) => Ok(show_hit(&v).into_iter().collect()),
            Err(Err::NotFound) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    pub async fn show(&self, id: &str) -> Result<Fetched, Err> {
        parse_show(&self.json(&format!("/shows/{id}"), &[]).await?).ok_or(Err::Failed)
    }

    pub async fn episode(&self, show: &str, season: u32, number: u32) -> Result<Fetched, Err> {
        let (s, n) = (season.to_string(), number.to_string());
        let v = self
            .json(
                &format!("/shows/{show}/episodebynumber"),
                &[("season", &s), ("number", &n)],
            )
            .await?;
        parse_episode(&v).ok_or(Err::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_search_show_and_episode() {
        let s = json!([{"score":0.9,"show":{"id":82,"name":"Game of Show","premiered":"2011-04-17",
            "summary":"<p>Nine <b>noble</b> families &amp; more.</p>"}}, {"score":0.1,"show":{"name":"no id"}}]);
        let hits = parse_search(&s);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].overview, "Nine noble families & more.");
        assert_eq!(hits[0].year, Some(2011));

        let show = json!({"id":82,"name":"Game of Show","premiered":"2011-04-17","genres":["Drama","Fantasy"],
            "averageRuntime":60,"rating":{"average":8.9},"externals":{"thetvdb":121361,"imdb":"tt0944947"},
            "image":{"medium":"https://static.example/m.jpg","original":"https://static.example/o.jpg"},"summary":null});
        let f = parse_show(&show).unwrap();
        assert_eq!(f.meta.genres, ["Drama", "Fantasy"]);
        assert_eq!(f.meta.ids.tvdb.as_deref(), Some("121361"));
        assert_eq!(f.meta.plot, None);
        assert_eq!(f.poster.as_deref(), Some("https://static.example/o.jpg"));
        assert_eq!(f.meta.native_id.as_deref(), Some("82"));

        let e = parse_episode(&json!({"id":4952,"name":"Winter","airdate":"2011-04-17","runtime":62,"rating":{"average":null},"summary":"<p>Ep.</p>"})).unwrap();
        assert_eq!(
            (
                e.meta.title.as_deref(),
                e.meta.rating,
                e.meta.plot.as_deref()
            ),
            (Some("Winter"), None, Some("Ep."))
        );
    }
}
