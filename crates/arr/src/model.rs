//! What NOCTURNE reads of Radarr's and Sonarr's JSON: only the fields used, with defaults.
use serde::Deserialize;

/// `GET /system/status`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    pub app_name: String,
}

/// An entry of `GET /qualityprofile`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: u32,
    pub name: String,
}

/// An entry of `GET /rootfolder`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RootFolder {
    pub id: u32,
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Image {
    pub cover_type: String,
    pub remote_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Rating {
    pub value: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Ratings {
    pub tmdb: Rating,
    pub imdb: Rating,
    /// Sonarr's single `ratings.value`.
    pub value: f32,
}

/// A result of `GET /movie/lookup` or `GET /series/lookup`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Lookup {
    pub title: String,
    pub year: u32,
    pub tmdb_id: u64,
    pub tvdb_id: u64,
    pub imdb_id: Option<String>,
    pub overview: Option<String>,
    pub images: Vec<Image>,
    pub ratings: Ratings,
    /// Sonarr: the seasons of the show.
    pub seasons: Vec<SeasonInfo>,
    /// Already in the service's library when `> 0`.
    pub id: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonInfo {
    pub season_number: u32,
    pub monitored: bool,
}

impl Lookup {
    pub fn poster(&self) -> Option<&str> {
        self.images
            .iter()
            .find(|i| i.cover_type == "poster")
            .and_then(|i| i.remote_url.as_deref())
    }

    pub fn rating(&self) -> Option<f32> {
        [
            self.ratings.tmdb.value,
            self.ratings.imdb.value,
            self.ratings.value,
        ]
        .into_iter()
        .find(|v| *v > 0.0)
    }
}

/// Where a title stands in the service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Progress {
    /// The service has not been asked for it (or it was removed).
    #[default]
    Unknown,
    /// Added and monitored, nothing grabbed yet.
    Wanted,
    /// In the download queue, not started or paused.
    Queued,
    Downloading,
    /// A file exists (a series: at least one episode file).
    Available,
}

/// One record of `GET /queue` (only the ids and the status).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QueueRecord {
    pub movie_id: Option<u64>,
    pub series_id: Option<u64>,
    /// `downloading`, `queued`, `paused`, `completed`, `delay`, ...
    pub status: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct QueuePage {
    pub records: Vec<QueueRecord>,
}

/// An entry of `GET /movie?tmdbId=` or `GET /series?tvdbId=`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Existing {
    pub id: u64,
    pub has_file: bool,
    pub statistics: Statistics,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Statistics {
    pub episode_file_count: u32,
}

/// Turn a library entry and the queue into a [`Progress`].
pub fn progress_of(e: &Existing, queue: &[QueueRecord], movie: bool) -> Progress {
    if e.id == 0 {
        return Progress::Unknown;
    }
    if e.has_file || e.statistics.episode_file_count > 0 {
        return Progress::Available;
    }
    let mut best = Progress::Wanted;
    for r in queue
        .iter()
        .filter(|r| if movie { r.movie_id } else { r.series_id } == Some(e.id))
    {
        match r.status.as_str() {
            "downloading" => return Progress::Downloading,
            _ => best = Progress::Queued,
        }
    }
    best
}

/// A movie to add.
#[derive(Clone, Debug)]
pub struct NewMovie {
    pub title: String,
    pub year: u32,
    pub tmdb_id: u64,
    pub profile_id: u32,
    pub root_folder: String,
}

/// A series to add with the seasons the user asked for.
#[derive(Clone, Debug)]
pub struct NewSeries {
    pub title: String,
    pub tvdb_id: u64,
    pub profile_id: u32,
    pub root_folder: String,
    /// Every season of the show (from the lookup) and whether it is wanted.
    pub seasons: Vec<SeasonInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(movie: bool, id: u64, status: &str) -> QueueRecord {
        QueueRecord {
            movie_id: movie.then_some(id),
            series_id: (!movie).then_some(id),
            status: status.into(),
        }
    }

    #[test]
    fn progress_follows_file_then_queue() {
        let e = Existing {
            id: 7,
            ..Default::default()
        };
        assert_eq!(
            progress_of(&Existing::default(), &[], true),
            Progress::Unknown
        );
        assert_eq!(progress_of(&e, &[], true), Progress::Wanted);
        assert_eq!(
            progress_of(&e, &[rec(true, 7, "queued")], true),
            Progress::Queued
        );
        assert_eq!(
            progress_of(
                &e,
                &[rec(true, 7, "queued"), rec(true, 7, "downloading")],
                true
            ),
            Progress::Downloading
        );
        // another title's record, or the other service's id space, does not count
        assert_eq!(
            progress_of(&e, &[rec(true, 8, "downloading")], true),
            Progress::Wanted
        );
        assert_eq!(
            progress_of(&e, &[rec(false, 7, "downloading")], true),
            Progress::Wanted
        );
        let file = Existing {
            id: 7,
            has_file: true,
            ..Default::default()
        };
        assert_eq!(
            progress_of(&file, &[rec(true, 7, "downloading")], true),
            Progress::Available
        );
        let eps = Existing {
            id: 7,
            statistics: Statistics {
                episode_file_count: 3,
            },
            ..Default::default()
        };
        assert_eq!(progress_of(&eps, &[], false), Progress::Available);
    }

    #[test]
    fn lookup_parses_radarr_and_sonarr_shapes() {
        let m: Lookup = serde_json::from_str(
            r#"{"title":"Alien","year":1979,"tmdbId":348,"imdbId":"tt0078748","overview":"x",
            "images":[{"coverType":"fanart","remoteUrl":"https://e/f.jpg"},{"coverType":"poster","remoteUrl":"https://e/p.jpg"}],
            "ratings":{"tmdb":{"value":8.1},"imdb":{"value":0}},"unknown":1}"#,
        )
        .unwrap();
        assert_eq!(
            (m.tmdb_id, m.poster(), m.rating()),
            (348, Some("https://e/p.jpg"), Some(8.1))
        );
        assert_eq!(m.id, 0);
        let s: Lookup = serde_json::from_str(
            r#"{"title":"Show","year":2008,"tvdbId":81189,"seasons":[{"seasonNumber":0,"monitored":false},{"seasonNumber":1,"monitored":true}],"ratings":{"value":8.9}}"#,
        )
        .unwrap();
        assert_eq!(
            (s.tvdb_id, s.seasons.len(), s.rating()),
            (81189, 2, Some(8.9))
        );
    }
}
