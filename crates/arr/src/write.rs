//! The only writes of this crate: add a movie to Radarr, add a series to Sonarr. Each takes an
//! [`ArrConfirmed`]. `tests/structure.rs` fails if a write verb appears elsewhere.
use crate::model::*;
use crate::{ArrClient, ArrConfirmed, Result};
use serde_json::{Value, json};

/// The body of `POST /movie`: monitored, search for it at once, grab it once it is released.
pub fn add_movie_body(m: &NewMovie) -> Value {
    json!({
        "title": m.title,
        "year": m.year,
        "tmdbId": m.tmdb_id,
        "qualityProfileId": m.profile_id,
        "rootFolderPath": m.root_folder,
        "monitored": true,
        "minimumAvailability": "released",
        "addOptions": { "searchForMovie": true },
    })
}

/// The body of `POST /series`: only the chosen seasons are monitored (the `seasons` flags decide,
/// no `monitor` preset), and the service searches for what is missing.
pub fn add_series_body(s: &NewSeries) -> Value {
    json!({
        "title": s.title,
        "tvdbId": s.tvdb_id,
        "qualityProfileId": s.profile_id,
        "rootFolderPath": s.root_folder,
        "monitored": true,
        "seasonFolder": true,
        "seasons": s.seasons.iter().map(|x| json!({"seasonNumber": x.season_number, "monitored": x.monitored})).collect::<Vec<_>>(),
        "addOptions": {
            "ignoreEpisodesWithFiles": false,
            "ignoreEpisodesWithoutFiles": false,
            "searchForMissingEpisodes": true,
        },
    })
}

impl ArrClient {
    /// Add a movie to Radarr.
    pub async fn add_movie(&self, m: &NewMovie, _c: ArrConfirmed) -> Result<()> {
        let path = "/movie";
        let rb = self.authed(self.http.post(self.url(path)).json(&add_movie_body(m)));
        self.send(rb, path).await.map(|_| ())
    }

    /// Add a series to Sonarr.
    pub async fn add_series(&self, s: &NewSeries, _c: ArrConfirmed) -> Result<()> {
        let path = "/series";
        let rb = self.authed(self.http.post(self.url(path)).json(&add_series_body(s)));
        self.send(rb, path).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_body_searches_and_waits_for_release() {
        let b = add_movie_body(&NewMovie {
            title: "Alien".into(),
            year: 1979,
            tmdb_id: 348,
            profile_id: 4,
            root_folder: "/movies".into(),
        });
        assert_eq!(b["tmdbId"], 348);
        assert_eq!(b["qualityProfileId"], 4);
        assert_eq!(b["rootFolderPath"], "/movies");
        assert_eq!(b["addOptions"]["searchForMovie"], true);
        assert_eq!(b["minimumAvailability"], "released");
    }

    #[test]
    fn series_body_monitors_only_the_chosen_seasons() {
        let b = add_series_body(&NewSeries {
            title: "Show".into(),
            tvdb_id: 81189,
            profile_id: 1,
            root_folder: "/tv".into(),
            seasons: vec![
                SeasonInfo {
                    season_number: 0,
                    monitored: false,
                },
                SeasonInfo {
                    season_number: 1,
                    monitored: false,
                },
                SeasonInfo {
                    season_number: 2,
                    monitored: true,
                },
            ],
        });
        assert_eq!(b["tvdbId"], 81189);
        let s = b["seasons"].as_array().unwrap();
        assert_eq!(s[2], json!({"seasonNumber": 2, "monitored": true}));
        assert_eq!(s[1]["monitored"], false);
        assert!(b["addOptions"].get("monitor").is_none());
        assert_eq!(b["addOptions"]["searchForMissingEpisodes"], true);
    }
}
