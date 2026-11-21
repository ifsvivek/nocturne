//! What NOCTURNE reads of Seerr's JSON. Seerr's payloads are large and change between minor
//! versions, so every struct takes only the fields used, with defaults and `Option` everywhere;
//! unknown fields are ignored. There is deliberately no field for `jellyfinAuthToken` (Seerr
//! echoes it in sign-in replies): it can never reach a log or a `Debug` print from here.
use serde::{Deserialize, Serialize};

/// `GET /status` (no sign-in needed).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Status {
    pub version: String,
}

/// `GET /settings/public` (no sign-in needed): what a client may offer.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PublicSettings {
    pub initialized: bool,
    pub local_login: bool,
    pub media_server_login: bool,
    /// 1 Plex, 2 Jellyfin, 3 Emby, 4 not configured.
    pub media_server_type: u32,
    #[serde(rename = "movie4kEnabled")]
    pub movie_4k_enabled: bool,
    #[serde(rename = "series4kEnabled")]
    pub series_4k_enabled: bool,
    pub partial_requests_enabled: bool,
    /// Season 0 (Specials) may be requested.
    pub enable_special_episodes: bool,
    pub application_title: String,
}

/// The signed-in user (`GET /auth/me`, and the reply of every sign-in).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Me {
    pub id: u64,
    /// Bit flags, see [`crate::perm`].
    pub permissions: u64,
    pub display_name: Option<String>,
    pub jellyfin_username: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
}

impl Me {
    pub fn name(&self) -> String {
        [&self.display_name, &self.jellyfin_username, &self.username]
            .into_iter()
            .flatten()
            .find(|s| !s.is_empty())
            .or(self.email.as_ref())
            .cloned()
            .unwrap_or_else(|| format!("user {}", self.id))
    }
}

/// One page of `GET /search`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchPage {
    pub page: u32,
    pub total_pages: u32,
    pub total_results: u32,
    pub results: Vec<SearchHit>,
}

/// A search result: movie, tv or person (`media_type`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchHit {
    /// The TMDB id.
    pub id: u64,
    pub media_type: String,
    pub title: Option<String>,
    pub name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
    pub media_info: Option<MediaInfo>,
}

/// Movie or show: the only two things that can be requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Movie,
    Tv,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "movie" => Some(Self::Movie),
            "tv" => Some(Self::Tv),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }
}

fn year_of(date: &Option<String>) -> Option<u32> {
    date.as_deref()?.get(..4)?.parse().ok()
}

impl SearchHit {
    pub fn kind(&self) -> Option<Kind> {
        Kind::parse(&self.media_type)
    }

    pub fn display_title(&self) -> String {
        self.title.clone().or(self.name.clone()).unwrap_or_default()
    }

    pub fn year(&self) -> Option<u32> {
        year_of(&self.release_date).or(year_of(&self.first_air_date))
    }
}

/// What Seerr knows about a title (`mediaInfo`). Absent when it has never seen it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaInfo {
    pub tmdb_id: Option<u64>,
    /// 1 unknown, 2 pending, 3 processing, 4 partially available, 5 available, 6 blocklisted, 7 deleted.
    pub status: u32,
    pub requests: Vec<Request>,
    pub seasons: Vec<SeasonState>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonState {
    pub season_number: u32,
    pub status: u32,
}

/// `GET /movie/{id}` and `GET /tv/{id}`: the TMDB data plus Seerr's state. Also what a search hit
/// is completed with when the request dialog opens.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Detail {
    pub id: u64,
    pub title: Option<String>,
    pub name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub seasons: Vec<TmdbSeason>,
    pub media_info: Option<MediaInfo>,
}

impl Detail {
    pub fn display_title(&self) -> String {
        self.title.clone().or(self.name.clone()).unwrap_or_default()
    }

    pub fn year(&self) -> Option<u32> {
        year_of(&self.release_date).or(year_of(&self.first_air_date))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TmdbSeason {
    pub season_number: u32,
    pub episode_count: u32,
}

/// A media request. Requests carry no title or poster: the page fetches the details per entry.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Request {
    pub id: u64,
    /// 1 pending approval, 2 approved, 3 declined, 4 failed, 5 completed.
    pub status: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub is4k: bool,
    pub created_at: Option<String>,
    pub media: Option<RequestMedia>,
    pub seasons: Vec<SeasonState>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RequestMedia {
    pub tmdb_id: Option<u64>,
    pub media_type: String,
    pub status: u32,
}

impl Request {
    pub fn tmdb_id(&self) -> Option<u64> {
        self.media.as_ref()?.tmdb_id
    }

    pub fn media_kind(&self) -> Option<Kind> {
        Kind::parse(&self.kind).or_else(|| Kind::parse(&self.media.as_ref()?.media_type))
    }
}

/// `GET /user/{id}/requests`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RequestsPage {
    pub page_info: PageInfo,
    pub results: Vec<Request>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageInfo {
    pub pages: u32,
    pub results: u32,
    pub page: u32,
}

/// `GET /user/{id}/quota`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Quota {
    pub movie: QuotaPart,
    pub tv: QuotaPart,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct QuotaPart {
    pub days: Option<u32>,
    pub limit: Option<u32>,
    pub used: u32,
    pub remaining: Option<u32>,
    pub restricted: bool,
}

/// Quick Connect step one.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct QuickConnect {
    pub code: String,
    pub secret: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct QuickCheck {
    pub authenticated: bool,
}

/// Which seasons a show request asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seasons {
    All,
    Some(Vec<u32>),
}

/// The body of `POST /request`. Only the plain fields: the advanced ones (server, profile, root
/// folder, user, 4K) are never sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewRequest {
    pub kind: Kind,
    pub tmdb_id: u64,
    pub seasons: Seasons,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum SeasonsBody<'a> {
    All(&'a str),
    List(&'a [u32]),
}
