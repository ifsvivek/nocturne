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
        }
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

    /// "S1 · E4" style label for episodes, "" otherwise.
    pub fn episode_label(&self) -> String {
        match (self.season_number, self.number) {
            (Some(s), Some(e)) => format!("S{s} · E{e}"),
            (None, Some(e)) => format!("E{e}"),
            _ => String::new(),
        }
    }
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
    http: reqwest::Client,
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
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .map_err(|_| ConnectError::Unreachable)?,
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
            Art::Backdrop => (jf::ImageType::Backdrop, None),
        };
        let url = self.client.image_url(&m.id, ty, Some(width), tag);
        let resp = self
            .http
            .get(url)
            .header("Authorization", self.client.auth_header())
            .send()
            .await
            .map_err(|_| Failed)?;
        if !resp.status().is_success() {
            return Err(Failed);
        }
        resp.bytes().await.map(|b| b.to_vec()).map_err(|_| Failed)
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
