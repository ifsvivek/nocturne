//! The unit lists of the library manager of `nocturne-server` (docs/dev/SERVER_PLAN.md): a series' episodes (TVmaze), an artist's albums (MusicBrainz) and an author's books (Open Library). No keys. What is sent: the title's name or its provider id, never a path.
use super::Engine;
use super::http::Err;
use crate::server::types::{Ids, MediaKind};
use crate::server::{LookupHit, Unit};
use serde_json::Value;

pub const OPENLIBRARY_CREDIT: &str = "Book data from Open Library (openlibrary.org).";

fn unit(name: &str) -> Unit {
    Unit {
        id: 0,
        title: 0,
        name: name.into(),
        season: None,
        episode: None,
        year: None,
        air_date: None,
        ids: Ids::default(),
        monitored: true,
        file: None,
    }
}

fn date(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|d| d.len() >= 4 && d[..4].bytes().all(|b| b.is_ascii_digit()))
        .map(String::from)
}

/// TVmaze `/shows/{id}/episodes`: numbered episodes only (specials have no number).
pub fn parse_episodes(v: &Value) -> Vec<Unit> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let mut u = unit(e["name"].as_str().unwrap_or_default());
            u.season = Some(e["season"].as_u64()? as u32);
            u.episode = Some(e["number"].as_u64()? as u32);
            u.air_date = date(&e["airdate"]);
            u.year = u.air_date.as_ref().and_then(|d| d[..4].parse().ok());
            u.ids.tvmaze = e["id"].as_u64();
            Some(u)
        })
        .collect()
}

/// MusicBrainz `/release-group?artist=`: albums (not singles, compilations or live records).
pub fn parse_albums(v: &Value) -> Vec<Unit> {
    v["release-groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|g| {
            g["primary-type"] == "Album"
                && g["secondary-types"].as_array().is_none_or(|s| s.is_empty())
        })
        .filter_map(|g| {
            let mut u = unit(g["title"].as_str()?);
            u.air_date = date(&g["first-release-date"]);
            u.year = u.air_date.as_ref().and_then(|d| d[..4].parse().ok());
            u.ids.musicbrainz = g["id"].as_str().map(String::from);
            Some(u)
        })
        .collect()
}

/// The first id of a MusicBrainz `/artist?query=` or Open Library `/search/authors.json` answer.
pub fn parse_first_id(v: &Value) -> Option<String> {
    let first = v["artists"].get(0).or_else(|| v["docs"].get(0))?;
    first["id"]
        .as_str()
        .or(first["key"].as_str())
        .map(|k| k.trim_start_matches("/authors/").to_string())
}

/// MusicBrainz `/artist?query=` or Open Library `/search/authors.json`: every hit.
pub fn parse_people(v: &Value, kind: MediaKind) -> Vec<LookupHit> {
    let rows = if kind == MediaKind::Music {
        &v["artists"]
    } else {
        &v["docs"]
    };
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let mut ids = Ids::default();
            let (name, year, overview) = if kind == MediaKind::Music {
                ids.musicbrainz = Some(r["id"].as_str()?.to_string());
                (
                    r["name"].as_str()?,
                    r["life-span"]["begin"]
                        .as_str()
                        .and_then(|d| d.get(..4)?.parse().ok()),
                    r["disambiguation"].as_str().unwrap_or_default(),
                )
            } else {
                ids.openlibrary = Some(
                    r["key"]
                        .as_str()?
                        .trim_start_matches("/authors/")
                        .to_string(),
                );
                (
                    r["name"].as_str()?,
                    r["birth_date"]
                        .as_str()
                        .and_then(|d| d.rsplit(' ').next()?.parse().ok()),
                    r["top_work"].as_str().unwrap_or_default(),
                )
            };
            Some(LookupHit {
                kind,
                name: name.into(),
                year,
                overview: overview.into(),
                ids,
            })
        })
        .collect()
}

/// Open Library `/authors/{id}/works.json`.
pub fn parse_works(v: &Value) -> Vec<Unit> {
    v["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let mut u = unit(w["title"].as_str()?);
            u.air_date = date(&w["first_publish_date"]);
            u.year = u.air_date.as_ref().and_then(|d| d[..4].parse().ok());
            u.ids.openlibrary = w["key"]
                .as_str()
                .map(|k| k.trim_start_matches("/works/").to_string());
            Some(u)
        })
        .collect()
}

impl Engine {
    /// Search a provider by name to add a title: TMDB for movies (needs the user's key), TVmaze for series, MusicBrainz for artists, Open Library for authors.
    pub async fn search_titles(
        &self,
        kind: MediaKind,
        q: &str,
        tmdb_key: Option<&str>,
    ) -> Result<Vec<LookupHit>, Err> {
        let hit = |h: super::matching::Hit, ids: Ids| LookupHit {
            kind,
            name: h.title,
            year: h.year,
            overview: h.overview,
            ids,
        };
        Ok(match kind {
            MediaKind::Movie => {
                let key = tmdb_key.ok_or(Err::Auth)?;
                self.tmdb_client(key)
                    .search(false, q, None)
                    .await?
                    .into_iter()
                    .map(|h| {
                        let ids = Ids {
                            tmdb: h.id.parse().ok(),
                            ..Default::default()
                        };
                        hit(h, ids)
                    })
                    .collect()
            }
            MediaKind::Series => self
                .tvmaze_client()
                .search(q)
                .await?
                .into_iter()
                .map(|h| {
                    let ids = Ids {
                        tvmaze: h.id.parse().ok(),
                        ..Default::default()
                    };
                    hit(h, ids)
                })
                .collect(),
            MediaKind::Music => {
                let query = format!("artist:\"{}\"", q.replace(['\\', '"'], " "));
                let rb = self
                    .http
                    .get(&format!("{}/artist", self.ep.musicbrainz))
                    .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "10")]);
                parse_people(&self.http.json(&self.mb, rb).await?, kind)
            }
            MediaKind::Book => {
                let rb = self
                    .http
                    .get(&format!("{}/search/authors.json", self.ep.openlibrary))
                    .query(&[("q", q), ("limit", "10")]);
                parse_people(&self.http.json(&self.ol, rb).await?, kind)
            }
        })
    }

    /// A series' episodes, found by TVmaze id, else IMDb / TVDB id, else name. Returns the TVmaze id too.
    pub async fn series_units(&self, ids: &Ids, name: &str) -> Result<(u64, Vec<Unit>), Err> {
        let tv = self.tvmaze_client();
        let id = match ids.tvmaze {
            Some(id) => id,
            None => {
                let mut hits = vec![];
                if let Some(i) = &ids.imdb {
                    hits = tv.lookup("imdb", i).await?;
                }
                if hits.is_empty()
                    && let Some(i) = ids.tvdb
                {
                    hits = tv.lookup("thetvdb", &i.to_string()).await?;
                }
                if hits.is_empty() {
                    hits = tv.search(name).await?;
                }
                hits.first()
                    .and_then(|h| h.id.parse().ok())
                    .ok_or(Err::NotFound)?
            }
        };
        let rb = self
            .http
            .get(&format!("{}/shows/{id}/episodes", self.ep.tvmaze));
        Ok((id, parse_episodes(&self.http.json(&self.tvmaze, rb).await?)))
    }

    /// An artist's albums, by MusicBrainz artist id or name. Returns the artist id too.
    pub async fn artist_units(&self, ids: &Ids, name: &str) -> Result<(String, Vec<Unit>), Err> {
        let mb = &self.ep.musicbrainz;
        let id = match &ids.musicbrainz {
            Some(id) => id.clone(),
            None => {
                let q = format!("artist:\"{}\"", name.replace(['\\', '"'], " "));
                let rb = self.http.get(&format!("{mb}/artist")).query(&[
                    ("query", q.as_str()),
                    ("fmt", "json"),
                    ("limit", "1"),
                ]);
                parse_first_id(&self.http.json(&self.mb, rb).await?).ok_or(Err::NotFound)?
            }
        };
        let rb = self.http.get(&format!("{mb}/release-group")).query(&[
            ("artist", id.as_str()),
            ("type", "album"),
            ("fmt", "json"),
            ("limit", "100"),
        ]);
        Ok((
            id.clone(),
            parse_albums(&self.http.json(&self.mb, rb).await?),
        ))
    }

    /// An author's works, by Open Library author key (`OL23919A`) or name. Returns the key too.
    pub async fn author_units(&self, ids: &Ids, name: &str) -> Result<(String, Vec<Unit>), Err> {
        let ol = &self.ep.openlibrary;
        let id = match &ids.openlibrary {
            Some(id) => id.clone(),
            None => {
                let rb = self
                    .http
                    .get(&format!("{ol}/search/authors.json"))
                    .query(&[("q", name), ("limit", "1")]);
                parse_first_id(&self.http.json(&self.ol, rb).await?).ok_or(Err::NotFound)?
            }
        };
        let rb = self
            .http
            .get(&format!("{ol}/authors/{id}/works.json"))
            .query(&[("limit", "200")]);
        Ok((
            id.clone(),
            parse_works(&self.http.json(&self.ol, rb).await?),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn episodes_skip_specials_and_carry_dates() {
        let v = json!([
            {"id": 1, "name": "Pilot", "season": 1, "number": 1, "airdate": "2005-03-24"},
            {"id": 2, "name": "Special", "season": 1, "number": null, "airdate": ""},
            {"id": 3, "name": "Two", "season": 1, "number": 2, "airdate": ""}
        ]);
        let u = parse_episodes(&v);
        assert_eq!(u.len(), 2);
        assert_eq!(
            (u[0].season, u[0].episode, u[0].year),
            (Some(1), Some(1), Some(2005))
        );
        assert_eq!(u[0].air_date.as_deref(), Some("2005-03-24"));
        assert_eq!((u[1].air_date.clone(), u[1].ids.tvmaze), (None, Some(3)));
    }

    #[test]
    fn albums_are_plain_studio_albums() {
        let v = json!({"release-groups": [
            {"id": "a", "title": "Bookends", "primary-type": "Album", "secondary-types": [], "first-release-date": "1968-04-03"},
            {"id": "b", "title": "Greatest Hits", "primary-type": "Album", "secondary-types": ["Compilation"]},
            {"id": "c", "title": "Single", "primary-type": "Single"}
        ]});
        let u = parse_albums(&v);
        assert_eq!(u.len(), 1);
        assert_eq!(
            (
                u[0].name.as_str(),
                u[0].year,
                u[0].ids.musicbrainz.as_deref()
            ),
            ("Bookends", Some(1968), Some("a"))
        );
    }

    #[test]
    fn people_searches_list_every_hit() {
        let a = parse_people(
            &json!({"artists": [{"id": "x", "name": "Simon & Garfunkel", "life-span": {"begin": "1956"}, "disambiguation": "duo"}, {"name": "no id"}]}),
            MediaKind::Music,
        );
        assert_eq!(a.len(), 1);
        assert_eq!(
            (
                a[0].name.as_str(),
                a[0].year,
                a[0].ids.musicbrainz.as_deref()
            ),
            ("Simon & Garfunkel", Some(1956), Some("x"))
        );
        let b = parse_people(
            &json!({"docs": [{"key": "OL31A", "name": "Ursula K. Le Guin", "birth_date": "21 October 1929", "top_work": "A Wizard of Earthsea"}]}),
            MediaKind::Book,
        );
        assert_eq!(
            (
                b[0].year,
                b[0].ids.openlibrary.as_deref(),
                b[0].overview.as_str()
            ),
            (Some(1929), Some("OL31A"), "A Wizard of Earthsea")
        );
    }

    #[test]
    fn works_and_first_ids() {
        let v = json!({"entries": [
            {"key": "/works/OL59W", "title": "The Dispossessed", "first_publish_date": "1974"},
            {"key": "/works/OL60W"}
        ]});
        let u = parse_works(&v);
        assert_eq!(u.len(), 1);
        assert_eq!(
            (u[0].year, u[0].ids.openlibrary.as_deref()),
            (Some(1974), Some("OL59W"))
        );
        assert_eq!(
            parse_first_id(&json!({"docs": [{"key": "OL31A"}]})).as_deref(),
            Some("OL31A")
        );
        assert_eq!(
            parse_first_id(&json!({"artists": [{"id": "x-y"}]})).as_deref(),
            Some("x-y")
        );
        assert_eq!(parse_first_id(&json!({"docs": []})), None);
    }
}
