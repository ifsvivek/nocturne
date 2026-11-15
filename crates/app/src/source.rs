//! Media-source boundary. The UI only sees `Media`, `Source` and `ConnectError`;
//! Jellyfin is the first (and currently only) implementation behind it.
use nocturne_jellyfin as jf;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Library,
    Movie,
    Series,
    Season,
    Episode,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Art {
    Poster,
    Backdrop,
    /// 16:9 thumb: an episode's still, otherwise the backdrop.
    Landscape,
}

/// UI-facing media record. Never printed or logged (it holds personal titles).
#[derive(Clone, Debug)]
pub struct Media {
    pub id: String,
    pub title: String,
    pub kind: Kind,
    pub collection: Option<String>,
    pub year: Option<u32>,
    pub overview: Option<String>,
    pub runtime_min: Option<u32>,
    pub rating: Option<f32>,
    pub number: Option<u32>,
    pub season_number: Option<u32>,
    pub poster_tag: Option<String>,
    pub backdrop_tag: Option<String>,
    pub genres: Vec<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    /// Saved playback position and total length in seconds (read only).
    pub resume_s: Option<f64>,
    pub duration_s: Option<f64>,
    pub played: bool,
}

impl Media {
    pub fn from_item(i: &jf::Item) -> Self {
        let kind = match i.kind.as_str() {
            "Movie" => Kind::Movie,
            "Series" => Kind::Series,
            "Season" => Kind::Season,
            "Episode" => Kind::Episode,
            "CollectionFolder" | "UserView" => Kind::Library,
            _ => Kind::Other,
        };
        Self {
            id: i.id.clone(),
            title: i.name.clone(),
            kind,
            collection: i.collection_type.clone(),
            year: i.production_year,
            overview: i.overview.clone().filter(|s| !s.trim().is_empty()),
            // 1 tick = 100 ns.
            runtime_min: i
                .run_time_ticks
                .map(|t| (t / 600_000_000) as u32)
                .filter(|m| *m > 0),
            rating: i.community_rating,
            number: i.index_number,
            season_number: i.parent_index_number,
            poster_tag: i.image_tags.get("Primary").cloned(),
            backdrop_tag: i.backdrop_image_tags.first().cloned(),
            genres: i.genres.clone(),
            series_id: i.series_id.clone(),
            series_name: i.series_name.clone(),
            resume_s: i
                .user_data
                .as_ref()
                .map(|u| u.playback_position_ticks as f64 / TICKS_PER_S)
                .filter(|s| *s > 0.0),
            duration_s: i.run_time_ticks.map(|t| t as f64 / TICKS_PER_S),
            played: i.user_data.as_ref().is_some_and(|u| u.played),
        }
    }

    /// Fraction watched (0..1) for a partly watched item.
    pub fn progress(&self) -> Option<f32> {
        let (r, d) = (self.resume_s?, self.duration_s?);
        (!self.played && d > 0.0).then(|| (r / d).clamp(0.0, 1.0) as f32)
    }

    pub fn is_series(&self) -> bool {
        self.kind == Kind::Series
    }

    /// Lowercase kind tag for mono annotations.
    pub fn kind_label(&self) -> String {
        match self.kind {
            Kind::Movie => "movie".into(),
            Kind::Series => "series".into(),
            Kind::Season => "season".into(),
            Kind::Episode => "episode".into(),
            Kind::Library => self.collection.clone().unwrap_or_else(|| "library".into()),
            Kind::Other => "item".into(),
        }
    }

    /// "2021 · 2h 03m" style line (only what is known).
    pub fn meta_line(&self) -> String {
        let mut v = vec![];
        if let Some(y) = self.year {
            v.push(y.to_string());
        }
        if let Some(m) = self.runtime_min {
            v.push(fmt_runtime(m));
        }
        v.join(" · ")
    }

    /// "S2:E9" for episodes, "" otherwise.
    pub fn short_ep(&self) -> String {
        match (self.season_number, self.number) {
            (Some(s), Some(e)) => format!("S{s}:E{e}"),
            (None, Some(e)) => format!("E{e}"),
            _ => String::new(),
        }
    }

    /// "S1 · E4" style label for episodes, "" otherwise.
    pub fn episode_label(&self) -> String {
        match (self.season_number, self.number) {
            (Some(s), Some(e)) => format!("S{s} · E{e}"),
            (None, Some(e)) => format!("E{e}"),
            _ => String::new(),
        }
    }
}

/// 1 tick = 100 ns.
const TICKS_PER_S: f64 = 10_000_000.0;

/// "1:05:09" / "5:09" clock for the now-playing bar.
pub fn fmt_clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// What "play this series" means: first unplayed episode, else the first one.
pub fn pick_episode(eps: &[Media]) -> Option<&Media> {
    eps.iter().find(|e| !e.played).or(eps.first())
}

pub fn fmt_runtime(min: u32) -> String {
    if min >= 60 {
        format!("{}h {:02}m", min / 60, min % 60)
    } else {
        format!("{min}m")
    }
}

/// How the first screen failed, in terms a person can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectError {
    NotConfigured,
    Unreachable,
    Rejected,
}

#[derive(Clone, Copy, Debug)]
pub struct Failed;

/// Query for the contents of a library (or any container).
pub fn children_query(parent: &Media) -> jf::ItemsQuery {
    let browse = matches!(parent.collection.as_deref(), Some("movies" | "tvshows"));
    jf::ItemsQuery {
        parent_id: Some(parent.id.clone()),
        include_types: if browse {
            vec!["Movie".into(), "Series".into()]
        } else {
            vec![]
        },
        sort_by: Some("SortName".into()),
        recursive: browse,
        limit: Some(500),
        ..Default::default()
    }
}

/// Most recently added items of a library, for the Home rows.
pub fn latest_query(parent: &Media, limit: u32) -> jf::ItemsQuery {
    jf::ItemsQuery {
        sort_by: Some("DateCreated".into()),
        descending: true,
        limit: Some(limit),
        ..children_query(parent)
    }
}

pub fn classify(e: &jf::Error) -> ConnectError {
    match e {
        jf::Error::Config(_) => ConnectError::NotConfigured,
        jf::Error::Status {
            status: 401 | 403, ..
        } => ConnectError::Rejected,
        _ => ConnectError::Unreachable,
    }
}

/// Detects the container so raw bytes can be handed to GPUI's decoder.
pub fn sniff(b: &[u8]) -> Option<gpui::ImageFormat> {
    use gpui::ImageFormat::*;
    match b {
        [0x89, b'P', b'N', b'G', ..] => Some(Png),
        [0xFF, 0xD8, 0xFF, ..] => Some(Jpeg),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(Webp),
        [b'G', b'I', b'F', ..] => Some(Gif),
        _ => None,
    }
}

#[derive(Clone)]
pub struct Source {
    client: Arc<jf::Client>,
    user: Arc<str>,
    gate: Arc<Semaphore>,
}

impl Source {
    /// `None` means no usable configuration was found.
    pub async fn connect(cfg: Option<jf::Config>) -> Result<Self, ConnectError> {
        let client =
            jf::Client::new(cfg.ok_or(ConnectError::NotConfigured)?).map_err(|e| classify(&e))?;
        let user = client.first_user_id().await.map_err(|e| classify(&e))?;
        Ok(Self {
            client: Arc::new(client),
            user: user.into(),
            gate: Arc::new(Semaphore::new(6)),
        })
    }

    fn media(r: jf::Result<jf::Items>) -> Result<Vec<Media>, Failed> {
        r.map(|i| i.items.iter().map(Media::from_item).collect())
            .map_err(|_| Failed)
    }

    pub async fn libraries(&self) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.libraries(&self.user).await)
    }

    pub async fn children(&self, parent: &Media) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.items(&self.user, &children_query(parent)).await)
    }

    pub async fn latest(&self, parent: &Media) -> Result<Vec<Media>, Failed> {
        Self::media(
            self.client
                .items(&self.user, &latest_query(parent, 16))
                .await,
        )
    }

    pub async fn detail(&self, id: &str) -> Result<Media, Failed> {
        self.client
            .item(&self.user, id)
            .await
            .map(|i| Media::from_item(&i))
            .map_err(|_| Failed)
    }

    pub async fn seasons(&self, series: &str) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.seasons(&self.user, series).await)
    }

    pub async fn episodes(&self, series: &str, season: &str) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.episodes(&self.user, series, Some(season)).await)
    }

    /// Image bytes (GET only). At most a few requests in flight at once.
    pub async fn image(&self, m: &Media, art: Art, width: u32) -> Result<Vec<u8>, Failed> {
        let _permit = self.gate.acquire().await.map_err(|_| Failed)?;
        let (ty, tag) = match art {
            Art::Poster => (jf::ImageType::Primary, m.poster_tag.as_deref()),
            Art::Backdrop => (jf::ImageType::Backdrop, m.backdrop_tag.as_deref()),
            Art::Landscape if m.kind == Kind::Episode || m.backdrop_tag.is_none() => {
                (jf::ImageType::Primary, m.poster_tag.as_deref())
            }
            Art::Landscape => (jf::ImageType::Backdrop, m.backdrop_tag.as_deref()),
        };
        self.client
            .image_bytes(&m.id, ty, Some(width), tag)
            .await
            .map_err(|_| Failed)
    }

    /// Items with a saved position, most recently watched first.
    pub async fn resume(&self) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.resume(&self.user, 12).await)
    }

    /// Next unwatched episode of each followed series.
    pub async fn next_up(&self) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.next_up(&self.user, 12).await)
    }

    async fn all_episodes(&self, series: &str) -> Result<Vec<Media>, Failed> {
        Self::media(self.client.episodes(&self.user, series, None).await)
    }

    /// The episode "Play" should start for a series.
    pub async fn series_target(&self, series: &str) -> Result<Media, Failed> {
        let eps = self.all_episodes(series).await?;
        pick_episode(&eps).cloned().ok_or(Failed)
    }

    /// The client and user id, for the player page's own reads.
    pub fn client(&self) -> (Arc<jf::Client>, String) {
        (self.client.clone(), self.user.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: &str) -> jf::Item {
        jf::Item {
            id: "x1".into(),
            name: "Sample".into(),
            kind: kind.into(),
            ..Default::default()
        }
    }

    #[test]
    fn maps_kinds_and_runtime() {
        let mut i = item("Movie");
        i.run_time_ticks = Some(90 * 600_000_000);
        i.overview = Some("  ".into());
        i.image_tags.insert("Primary".into(), "t".into());
        let m = Media::from_item(&i);
        assert_eq!(m.kind, Kind::Movie);
        assert_eq!(m.runtime_min, Some(90));
        assert_eq!(m.overview, None, "blank overview is dropped");
        assert_eq!(m.poster_tag.as_deref(), Some("t"));
        assert_eq!(
            Media::from_item(&item("CollectionFolder")).kind,
            Kind::Library
        );
        assert_eq!(Media::from_item(&item("Trailer")).kind, Kind::Other);
    }

    #[test]
    fn runtime_and_meta() {
        assert_eq!(fmt_runtime(123), "2h 03m");
        assert_eq!(fmt_runtime(45), "45m");
        let mut i = item("Movie");
        i.production_year = Some(2001);
        i.run_time_ticks = Some(123 * 600_000_000);
        let m = Media::from_item(&i);
        assert_eq!(m.meta_line(), "2001 · 2h 03m");
        assert_eq!(m.kind_label(), "movie");
    }

    #[test]
    fn progress_and_clock() {
        let mut i = item("Episode");
        i.run_time_ticks = Some(100 * 10_000_000);
        i.user_data = Some(jf::UserData {
            playback_position_ticks: 25 * 10_000_000,
            played: false,
        });
        let m = Media::from_item(&i);
        assert_eq!(m.progress(), Some(0.25));
        assert_eq!(Media::from_item(&item("Movie")).progress(), None);
        assert_eq!(fmt_clock(3909.7), "1:05:09");
        assert_eq!(fmt_clock(309.0), "5:09");
        assert_eq!(fmt_clock(-1.0), "0:00");
    }

    #[test]
    fn episode_picking() {
        let ep = |id: &str, played| Media {
            id: id.into(),
            played,
            ..Media::from_item(&item("Episode"))
        };
        let eps = [ep("a", true), ep("b", false), ep("c", false)];
        assert_eq!(pick_episode(&eps).unwrap().id, "b");
        let seen = [ep("a", true), ep("b", true)];
        assert_eq!(pick_episode(&seen).unwrap().id, "a");
        assert!(pick_episode(&[]).is_none());
    }

    #[test]
    fn episode_label() {
        let mut i = item("Episode");
        i.parent_index_number = Some(2);
        i.index_number = Some(5);
        assert_eq!(Media::from_item(&i).episode_label(), "S2 · E5");
        assert_eq!(Media::from_item(&item("Movie")).episode_label(), "");
    }

    #[test]
    fn queries_depend_on_library_type() {
        let mut lib = Media::from_item(&item("CollectionFolder"));
        lib.collection = Some("movies".into());
        let q = children_query(&lib);
        assert!(q.recursive && q.include_types == ["Movie", "Series"]);
        lib.collection = Some("music".into());
        let q = children_query(&lib);
        assert!(!q.recursive && q.include_types.is_empty());
    }

    #[test]
    fn latest_is_newest_first_and_bounded() {
        let mut lib = Media::from_item(&item("CollectionFolder"));
        lib.collection = Some("movies".into());
        let q = latest_query(&lib, 16);
        assert!(q.descending && q.limit == Some(16) && q.sort_by.as_deref() == Some("DateCreated"));
        assert!(q.recursive, "inherits library-type behaviour");
    }

    #[test]
    fn error_classes() {
        let st = |s| jf::Error::Status {
            status: s,
            path: String::new(),
        };
        assert_eq!(classify(&st(401)), ConnectError::Rejected);
        assert_eq!(classify(&st(500)), ConnectError::Unreachable);
        assert_eq!(
            classify(&jf::Error::Config(String::new())),
            ConnectError::NotConfigured
        );
        assert_eq!(classify(&jf::Error::NoUsers), ConnectError::Unreachable);
    }

    #[test]
    fn sniffs_formats() {
        assert_eq!(
            sniff(&[0x89, b'P', b'N', b'G', 0]),
            Some(gpui::ImageFormat::Png)
        );
        assert_eq!(
            sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(gpui::ImageFormat::Jpeg)
        );
        assert_eq!(
            sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(gpui::ImageFormat::Webp)
        );
        assert_eq!(sniff(b"<html>"), None);
    }
}
