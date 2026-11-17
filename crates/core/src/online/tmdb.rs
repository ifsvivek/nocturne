//! TMDB (movies and TV). The user's own key: a v3 API key (32 hex characters, sent as `api_key`) or a v4 read
//! access token (`eyJ...`, sent as a Bearer header). Terms: docs/METADATA.md.

use super::http::{Err, Gate, Http};
use super::matching::Hit;
use super::{Endpoints, Fetched};
use nocturne_local::{OnlineMeta, ProviderIds, now_secs};
use serde_json::Value;

pub const NAME: &str = "tmdb";

fn year(v: &Value, k: &str) -> Option<u16> {
    v[k].as_str()?.get(..4)?.parse().ok()
}

fn text(v: &Value, k: &str) -> Option<String> {
    v[k].as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn rating(v: &Value) -> Option<f32> {
    v["vote_average"]
        .as_f64()
        .filter(|r| *r > 0.0)
        .map(|r| r as f32)
}

/// `results[]` of a search or find response.
pub fn parse_search(v: &Value, tv: bool) -> Vec<Hit> {
    let (t, d) = if tv {
        ("name", "first_air_date")
    } else {
        ("title", "release_date")
    };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Hit {
                id: r["id"].as_u64()?.to_string(),
                title: text(r, t)?,
                year: year(r, d),
                overview: text(r, "overview").unwrap_or_default(),
            })
        })
        .collect()
}

/// `/find/{imdb id}`: the movie or tv results of the requested kind.
pub fn parse_find(v: &Value, tv: bool) -> Vec<Hit> {
    let k = if tv { "tv_results" } else { "movie_results" };
    parse_search(&serde_json::json!({ "results": v[k] }), tv)
}

fn genres(v: &Value) -> Vec<String> {
    v["genres"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| text(g, "name"))
        .collect()
}

/// A movie, or (with `tv`) a series fetched with `append_to_response=external_ids`.
pub fn parse_details(v: &Value, tv: bool) -> Option<Fetched> {
    let id = v["id"].as_u64()?.to_string();
    let ext = &v["external_ids"];
    let imdb = text(v, "imdb_id").or_else(|| text(ext, "imdb_id"));
    let tvdb = ext["tvdb_id"].as_u64().map(|n| n.to_string());
    let (title, date) = if tv {
        ("name", "first_air_date")
    } else {
        ("title", "release_date")
    };
    Some(Fetched {
        meta: OnlineMeta {
            provider: NAME.into(),
            fetched: now_secs(),
            title: text(v, title),
            year: year(v, date),
            plot: text(v, "overview"),
            genres: genres(v),
            runtime_min: if tv {
                v["episode_run_time"][0].as_u64()
            } else {
                v["runtime"].as_u64()
            }
            .filter(|r| *r > 0)
            .map(|r| r as u32),
            rating: rating(v),
            ids: ProviderIds {
                imdb,
                tmdb: Some(id.clone()),
                tvdb,
            },
            native_id: Some(id),
            ..Default::default()
        },
        poster: text(v, "poster_path"),
        backdrop: text(v, "backdrop_path"),
    })
}

pub fn parse_episode(v: &Value) -> Option<Fetched> {
    v["id"].as_u64()?;
    Some(Fetched {
        meta: OnlineMeta {
            provider: NAME.into(),
            fetched: now_secs(),
            title: text(v, "name"),
            year: year(v, "air_date"),
            plot: text(v, "overview"),
            runtime_min: v["runtime"].as_u64().filter(|r| *r > 0).map(|r| r as u32),
            rating: rating(v),
            ..Default::default()
        },
        poster: None,
        backdrop: None,
    })
}

pub struct Tmdb<'a> {
    pub http: &'a Http,
    pub gate: &'a Gate,
    pub ep: &'a Endpoints,
    pub key: &'a str,
}

impl Tmdb<'_> {
    fn get(&self, path: &str, q: &[(&str, &str)]) -> reqwest::RequestBuilder {
        let rb = self.http.get(&format!("{}{path}", self.ep.tmdb)).query(q);
        if self.key.starts_with("eyJ") {
            rb.bearer_auth(self.key)
        } else {
            rb.query(&[("api_key", self.key)])
        }
    }

    async fn json(&self, path: &str, q: &[(&str, &str)]) -> Result<Value, Err> {
        self.http.json(self.gate, self.get(path, q)).await
    }

    /// One cheap authenticated request: is this key accepted?
    pub async fn validate(&self) -> Result<(), Err> {
        self.json("/configuration", &[]).await.map(|_| ())
    }

    pub async fn search(&self, tv: bool, title: &str, year: Option<u16>) -> Result<Vec<Hit>, Err> {
        let y = year.map(|y| y.to_string());
        let mut q = vec![("query", title)];
        if let Some(y) = &y {
            // Series use first_air_date_year; movies year.
            q.push((if tv { "first_air_date_year" } else { "year" }, y));
        }
        let v = self
            .json(if tv { "/search/tv" } else { "/search/movie" }, &q)
            .await?;
        Ok(parse_search(&v, tv))
    }

    pub async fn find_imdb(&self, tv: bool, imdb: &str) -> Result<Vec<Hit>, Err> {
        let v = self
            .json(&format!("/find/{imdb}"), &[("external_source", "imdb_id")])
            .await?;
        Ok(parse_find(&v, tv))
    }

    pub async fn details(&self, tv: bool, id: &str) -> Result<Fetched, Err> {
        let path = format!("/{}/{id}", if tv { "tv" } else { "movie" });
        let v = self
            .json(&path, &[("append_to_response", "external_ids")])
            .await?;
        parse_details(&v, tv).ok_or(Err::Failed)
    }

    pub async fn episode(&self, show: &str, season: u32, episode: u32) -> Result<Fetched, Err> {
        let v = self
            .json(
                &format!("/tv/{show}/season/{season}/episode/{episode}"),
                &[],
            )
            .await?;
        parse_episode(&v).ok_or(Err::Failed)
    }

    pub fn poster_url(&self, p: &str) -> String {
        format!("{}w500{p}", self.ep.tmdb_img)
    }

    pub fn backdrop_url(&self, p: &str) -> String {
        format!("{}w1280{p}", self.ep.tmdb_img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Shape of TMDB's v3 responses, reduced to the fields we read (invented values).
    #[test]
    fn parses_search_details_and_episode() {
        let s = json!({"page":1,"results":[
            {"id":603,"title":"The Matrix","release_date":"1999-03-30","overview":"A hacker learns.","popularity":80.1},
            {"id":604,"title":"No Date","release_date":"","overview":""},
            {"title":"No id"}]});
        let hits = parse_search(&s, false);
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].id.as_str(), hits[0].year), ("603", Some(1999)));
        assert_eq!(hits[1].year, None);

        let m = json!({"id":603,"title":"The Matrix","release_date":"1999-03-30","overview":"A hacker learns.",
            "genres":[{"id":28,"name":"Action"},{"id":878,"name":"Science Fiction"}],"runtime":136,
            "vote_average":8.2,"imdb_id":"tt0133093","poster_path":"/p.jpg","backdrop_path":"/b.jpg"});
        let f = parse_details(&m, false).unwrap();
        assert_eq!(f.meta.title.as_deref(), Some("The Matrix"));
        assert_eq!((f.meta.year, f.meta.runtime_min), (Some(1999), Some(136)));
        assert_eq!(f.meta.genres, ["Action", "Science Fiction"]);
        assert_eq!(f.meta.ids.imdb.as_deref(), Some("tt0133093"));
        assert_eq!(f.meta.ids.tmdb.as_deref(), Some("603"));
        assert_eq!(f.poster.as_deref(), Some("/p.jpg"));

        let tv = json!({"id":1396,"name":"Show","first_air_date":"2008-01-20","episode_run_time":[47],
            "external_ids":{"imdb_id":"tt0903747","tvdb_id":81189},"vote_average":0});
        let f = parse_details(&tv, true).unwrap();
        assert_eq!(f.meta.ids.tvdb.as_deref(), Some("81189"));
        assert_eq!((f.meta.runtime_min, f.meta.rating), (Some(47), None));

        let e = parse_episode(&json!({"id":9,"name":"Pilot","overview":"It begins.","air_date":"2008-01-20","runtime":58,"vote_average":8.1})).unwrap();
        assert_eq!(
            (e.meta.title.as_deref(), e.meta.runtime_min),
            (Some("Pilot"), Some(58))
        );
        assert!(parse_details(&json!({}), false).is_none());
    }

    #[test]
    fn find_picks_the_requested_kind() {
        let v = json!({"movie_results":[{"id":1,"title":"M","release_date":"2000-01-01"}],"tv_results":[{"id":2,"name":"T"}]});
        assert_eq!(parse_find(&v, false)[0].id, "1");
        assert_eq!(parse_find(&v, true)[0].id, "2");
    }
}
