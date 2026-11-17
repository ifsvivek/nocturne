//! Subtitles from OpenSubtitles.com (REST API v1), with the user's own API key; downloading usually also needs
//! the user's OpenSubtitles login. Both live in the keyring. Subtitles are stored in the local library's data
//! directory (`Library::save_subtitle`), never next to the media. Terms and limits: docs/METADATA.md.
//!
//! Sent: the title (a show's name for an episode), year, IMDb id, season / episode number and the wanted
//! languages. Never a file hash, a file name or a path.

use super::http::Err;
use super::{Engine, KEY_SUBS, KEY_SUBS_LOGIN};
use crate::source::{Kind, Media};
use serde_json::{Value, json};

/// Subtitle files are text; anything larger is not one.
const MAX_BYTES: usize = 4 << 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub title: String,
    pub year: Option<u32>,
    /// Digits only (OpenSubtitles wants the number).
    pub imdb: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// Two-letter codes, e.g. `en`.
    pub langs: Vec<String>,
}

impl Query {
    /// A movie or an episode; `None` for anything else. `lang`: the user's subtitle language setting
    /// (a code from the language table) or none for English.
    pub fn for_media(m: &Media, lang: Option<&str>) -> Option<Self> {
        let imdb = m
            .ids
            .imdb
            .as_deref()
            .map(|i| i.trim_start_matches("tt").to_string())
            .filter(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()));
        let langs = vec![two_letter(lang.unwrap_or("en"))];
        match m.kind {
            Kind::Movie => Some(Self {
                title: m.title.clone(),
                year: m.year,
                imdb,
                langs,
                ..Default::default()
            }),
            Kind::Episode => Some(Self {
                title: m.series_name.clone()?,
                season: m.season_number,
                episode: m.number,
                // The ids of a local episode are its show's.
                imdb: None,
                langs,
                ..Default::default()
            }),
            _ => None,
        }
    }
}

/// `eng` -> `en` via the language table the settings use; unknown codes pass through.
pub fn two_letter(code: &str) -> String {
    crate::video::LANGUAGES
        .iter()
        .find(|(_, cs)| cs.contains(&code))
        .and_then(|(_, cs)| cs.iter().find(|c| c.len() == 2))
        .map_or_else(|| code.to_string(), |c| (*c).to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub file_id: u64,
    pub language: String,
    pub release: String,
    pub rating: f32,
    pub downloads: u64,
    pub hearing_impaired: bool,
    pub file_name: String,
}

pub fn parse_search(v: &Value) -> Vec<Hit> {
    let mut hits: Vec<Hit> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let a = &d["attributes"];
            Some(Hit {
                file_id: a["files"][0]["file_id"].as_u64()?,
                language: a["language"].as_str()?.to_string(),
                release: a["release"].as_str().unwrap_or_default().to_string(),
                rating: a["ratings"].as_f64().unwrap_or(0.0) as f32,
                downloads: a["download_count"].as_u64().unwrap_or(0),
                hearing_impaired: a["hearing_impaired"].as_bool().unwrap_or(false),
                file_name: a["files"][0]["file_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect();
    // Best first: rated, then popular.
    hits.sort_by(|a, b| {
        b.rating
            .total_cmp(&a.rating)
            .then(b.downloads.cmp(&a.downloads))
            .then(a.file_id.cmp(&b.file_id))
    });
    hits
}

/// The file name to keep a download under: language and file id, with the original extension.
pub fn local_name(h: &Hit) -> String {
    let ext = h
        .file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "srt" | "ass" | "ssa" | "vtt" | "sub"))
        .unwrap_or_else(|| "srt".into());
    format!("{}-{}.{ext}", h.language, h.file_id)
}

/// The stored API key and optional login, read from the keyring (blocking).
pub fn credentials() -> Option<(String, Option<(String, String)>)> {
    let key = nocturne_session::load_secret(KEY_SUBS)
        .ok()
        .flatten()
        .filter(|k| !k.trim().is_empty())?;
    let login = nocturne_session::load_secret(KEY_SUBS_LOGIN)
        .ok()
        .flatten()
        .and_then(|s| {
            s.split_once('\n')
                .map(|(u, p)| (u.to_string(), p.to_string()))
        });
    Some((key, login))
}

/// Keep (or with a blank user remove) the OpenSubtitles login.
pub fn set_login(user: &str, password: &str) -> Result<(), String> {
    let v = if user.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n{password}", user.trim())
    };
    super::set_secret(KEY_SUBS_LOGIN, &v)
}

impl Engine {
    fn subs_req(&self, rb: reqwest::RequestBuilder, key: &str) -> reqwest::RequestBuilder {
        rb.header("Api-Key", key)
            .header("Accept", "application/json")
    }

    pub async fn find_subtitles(&self, key: &str, q: &Query) -> Result<Vec<Hit>, Err> {
        let (y, s, e) = (
            q.year.map(|v| v.to_string()),
            q.season.map(|v| v.to_string()),
            q.episode.map(|v| v.to_string()),
        );
        let langs = q.langs.join(",");
        // The API wants lower-case parameter names in alphabetical order.
        let mut p: Vec<(&str, &str)> = vec![];
        if let Some(e) = &e {
            p.push(("episode_number", e));
        }
        if let Some(i) = &q.imdb {
            p.push(("imdb_id", i));
        }
        if !langs.is_empty() {
            p.push(("languages", &langs));
        }
        p.push(("query", &q.title));
        if let Some(s) = &s {
            p.push(("season_number", s));
        }
        if q.season.is_some() {
            p.push(("type", "episode"));
        }
        if let Some(y) = &y {
            p.push(("year", y));
        }
        let rb = self.subs_req(
            self.http
                .get(&format!("{}/subtitles", self.ep.subs))
                .query(&p),
            key,
        );
        Ok(parse_search(&self.http.json(&self.subs, rb).await?))
    }

    /// Log in; the token is good for the download only (not kept).
    async fn subs_login(&self, key: &str, user: &str, pass: &str) -> Result<String, Err> {
        let rb = self
            .subs_req(self.http.post(&format!("{}/login", self.ep.subs)), key)
            .json(&json!({ "username": user, "password": pass }));
        let v = self.http.json(&self.subs, rb).await?;
        v["token"].as_str().map(String::from).ok_or(Err::Auth)
    }

    /// Download one subtitle: `(file name to keep it under, bytes)`.
    pub async fn download_subtitle(
        &self,
        key: &str,
        login: Option<&(String, String)>,
        h: &Hit,
    ) -> Result<(String, Vec<u8>), Err> {
        let mut rb = self
            .subs_req(self.http.post(&format!("{}/download", self.ep.subs)), key)
            .json(&json!({ "file_id": h.file_id }));
        if let Some((u, p)) = login {
            rb = rb.bearer_auth(self.subs_login(key, u, p).await?);
        }
        let v = self.http.json(&self.subs, rb).await?;
        let link = v["link"].as_str().ok_or(Err::Failed)?;
        let bytes = self
            .http
            .bytes(&self.subs, self.http.get(link), MAX_BYTES)
            .await?;
        Ok((local_name(h), bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_results() {
        let v = json!({"total_count":2,"data":[
          {"id":"1","type":"subtitle","attributes":{"language":"en","download_count":50,"ratings":0.0,"hearing_impaired":true,
            "release":"Film.2000.WEB","files":[{"file_id":111,"file_name":"film.srt"}]}},
          {"id":"2","type":"subtitle","attributes":{"language":"en","download_count":10,"ratings":8.5,
            "release":"Film.2000.BluRay","files":[{"file_id":222,"file_name":"film.bluray.ass"}]}},
          {"id":"3","attributes":{"language":"en","files":[]}}]});
        let h = parse_search(&v);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].file_id, h[0].rating), (222, 8.5));
        assert!(h[1].hearing_impaired);
        assert_eq!(local_name(&h[0]), "en-222.ass");
        assert_eq!(
            local_name(&Hit {
                file_name: "x.exe".into(),
                ..h[1].clone()
            }),
            "en-111.srt"
        );
    }

    #[test]
    fn query_is_built_from_ids_titles_and_numbers_only() {
        let mut m = Media::from_item(&nocturne_jellyfin::Item::default());
        m.kind = Kind::Movie;
        m.title = "Film".into();
        m.year = Some(2000);
        m.ids.imdb = Some("tt0123456".into());
        let q = Query::for_media(&m, Some("fra")).unwrap();
        assert_eq!(
            (q.imdb.as_deref(), q.langs.clone()),
            (Some("0123456"), vec!["fr".to_string()])
        );
        m.kind = Kind::Episode;
        m.series_name = Some("Show".into());
        (m.season_number, m.number) = (Some(2), Some(3));
        let q = Query::for_media(&m, None).unwrap();
        assert_eq!(
            (q.title.as_str(), q.season, q.episode, q.imdb),
            ("Show", Some(2), Some(3), None)
        );
        assert_eq!(q.langs, ["en"]);
        m.kind = Kind::Series;
        assert!(Query::for_media(&m, None).is_none());
    }
}
