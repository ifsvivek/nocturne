//! Media-source boundary. The UI only sees `Media`, `Source` and `ConnectError`;
//! Jellyfin is the first (and currently only) implementation behind it.
use crate::detail::Full;
use crate::imgcache::DiskCache;
use nocturne_jellyfin as jf;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

pub use nocturne_local::ProviderIds;

/// Which source (profile) an item came from. Assigned once when a profile is created and never reused.
/// `(origin, id)` is an item's identity everywhere: navigation, caches, image keys, playback, writes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SourceId(pub u32);

/// Provider ids out of Jellyfin's `ProviderIds` object (key case varies; blank values dropped).
pub fn provider_ids(m: &HashMap<String, Option<String>>) -> ProviderIds {
    let get = |k: &str| {
        m.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .and_then(|(_, v)| v.as_deref())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    ProviderIds {
        imdb: get("imdb"),
        tmdb: get("tmdb"),
        tvdb: get("tvdb"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Library,
    Movie,
    Series,
    Season,
    Episode,
    BoxSet,
    Playlist,
    Person,
    Genre,
    Studio,
    MusicAlbum,
    MusicArtist,
    Audio,
    Photo,
    Book,
    /// A plain folder or photo album: opens as a grid of its children.
    Folder,
    Video,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Art {
    Poster,
    Backdrop,
    /// 16:9 thumb: an episode's still, otherwise the backdrop.
    Landscape,
    /// A chapter thumbnail: `number` is the chapter index, `poster_tag` its image tag.
    Chapter,
    Logo,
}

/// UI-facing media record. Never printed or logged (it holds personal titles).
#[derive(Clone, Debug)]
pub struct Media {
    /// The source this item belongs to; `id` is only meaningful inside it.
    pub origin: SourceId,
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
    pub favorite: bool,
    /// The user may delete it; for a playlist that means they own it and may edit it.
    pub can_delete: bool,
    pub official_rating: Option<String>,
    /// Series/season: episodes not yet watched.
    pub unplayed: Option<u32>,
    pub child_count: Option<u32>,
    /// Blurhashes for the primary and first backdrop image.
    pub blur_primary: Option<String>,
    pub blur_backdrop: Option<String>,
    /// Secondary line: a cast member's role, an album's artist.
    pub sub: Option<String>,
    pub added: Option<String>,
    pub premiere: Option<String>,
    /// (item id, tag) of the title logo: its own, else the show's.
    pub logo: Option<(String, String)>,
    /// Item that owns the backdrop when it is the show's (seasons, episodes).
    pub backdrop_id: Option<String>,
    /// Provider ids, for matching the same title across sources.
    pub ids: ProviderIds,
    /// False for a local item whose folder is unmounted or whose file is gone: it is demoted when
    /// duplicates are merged and marked in the UI. Server items are always available here (a server
    /// that is down is a source status, not an item flag).
    pub available: bool,
    /// A local item whose details came (in part) from an online provider: its name ("TMDB").
    pub details_from: Option<String>,
}

impl Media {
    /// Origin 0: placeholders and single-server call sites. `Source` stamps its own origin.
    pub fn from_item(i: &jf::Item) -> Self {
        Self::from_item_in(SourceId::default(), i)
    }

    pub fn from_item_in(origin: SourceId, i: &jf::Item) -> Self {
        let kind = match i.kind.as_str() {
            "Movie" => Kind::Movie,
            "Series" => Kind::Series,
            "Season" => Kind::Season,
            "Episode" => Kind::Episode,
            "CollectionFolder" | "UserView" => Kind::Library,
            "BoxSet" => Kind::BoxSet,
            "Playlist" => Kind::Playlist,
            "Person" => Kind::Person,
            "Genre" | "MusicGenre" => Kind::Genre,
            "Studio" => Kind::Studio,
            "MusicAlbum" => Kind::MusicAlbum,
            "MusicArtist" => Kind::MusicArtist,
            "Audio" | "AudioBook" => Kind::Audio,
            "Photo" => Kind::Photo,
            "Book" => Kind::Book,
            "Folder" | "PhotoAlbum" => Kind::Folder,
            "Video" | "MusicVideo" | "Trailer" => Kind::Video,
            _ => Kind::Other,
        };
        let blur = |ty: &str, tag: Option<&String>| {
            tag.and_then(|t| i.x.image_blur_hashes.get(ty)?.get(t).cloned())
        };
        Self {
            origin,
            ids: provider_ids(&i.x.provider_ids),
            available: true,
            details_from: None,
            id: i.id.clone(),
            title: i.name.clone(),
            kind,
            collection: i.collection_type.clone(),
            year: i.production_year,
            overview: i.overview.clone().filter(|s| !s.trim().is_empty()),
            // 1 tick = 100 ns.
            runtime_min: i
                .run_time_ticks
                .filter(|t| *t > 0)
                // To the nearest minute, at least one (jellyfin-web shows "1m" for a clip of a few seconds).
                .map(|t| ((t + 300_000_000) / 600_000_000).max(1) as u32),
            rating: i.community_rating,
            number: i.index_number,
            season_number: i.parent_index_number,
            poster_tag: i.image_tags.get("Primary").cloned(),
            backdrop_tag: i
                .backdrop_image_tags
                .first()
                .or(i.x.parent_backdrop_image_tags.first())
                .cloned(),
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
            favorite: i.user_data.as_ref().is_some_and(|u| u.is_favorite),
            can_delete: i.x.can_delete.unwrap_or(false),
            official_rating: i.x.official_rating.clone(),
            unplayed: i.user_data.as_ref().and_then(|u| u.unplayed_item_count),
            child_count: i.x.child_count.or(i.x.recursive_item_count),
            blur_primary: blur("Primary", i.image_tags.get("Primary")),
            blur_backdrop: blur("Backdrop", i.backdrop_image_tags.first()),
            sub: i
                .x
                .album_artist
                .clone()
                .or_else(|| i.x.artists.first().cloned()),
            added: i.x.date_created.clone(),
            premiere: i.x.premiere_date.clone(),
            logo: i
                .image_tags
                .get("Logo")
                .map(|t| (i.id.clone(), t.clone()))
                .or_else(|| {
                    Some((
                        i.x.parent_logo_item_id.clone()?,
                        i.x.parent_logo_image_tag.clone()?,
                    ))
                }),
            backdrop_id: i
                .backdrop_image_tags
                .is_empty()
                .then(|| i.x.parent_backdrop_item_id.clone())
                .flatten(),
        }
    }

    /// A search hit (a thin record: enough to show and open).
    pub fn from_hint(h: &jf::SearchHint) -> Self {
        Self::from_hint_in(SourceId::default(), h)
    }

    pub fn from_hint_in(origin: SourceId, h: &jf::SearchHint) -> Self {
        let mut m = Self::from_item_in(
            origin,
            &jf::Item {
                id: h.item_id().to_string(),
                name: h.name.clone(),
                kind: h.kind.clone(),
                production_year: h.production_year,
                run_time_ticks: h.run_time_ticks,
                series_name: h.series.clone(),
                ..Default::default()
            },
        );
        m.poster_tag = h.primary_image_tag.clone();
        m.sub = h
            .series
            .clone()
            .or(h.album_artist.clone())
            .or(h.album.clone());
        m
    }

    /// Fraction watched (0..1) for a partly watched item.
    pub fn progress(&self) -> Option<f32> {
        let (r, d) = (self.resume_s?, self.duration_s?);
        (!self.played && d > 0.0).then(|| (r / d).clamp(0.0, 1.0) as f32)
    }

    /// Blurhash of the image `art` will show, if the server has one.
    pub fn blur_for(&self, art: Art) -> Option<&str> {
        match art {
            Art::Poster => self.blur_primary.as_deref(),
            Art::Backdrop => self.blur_backdrop.as_deref(),
            Art::Landscape if self.kind == Kind::Episode || self.backdrop_tag.is_none() => {
                self.blur_primary.as_deref()
            }
            Art::Landscape => self.blur_backdrop.as_deref(),
            Art::Chapter | Art::Logo => None,
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
            Kind::BoxSet => "collection".into(),
            Kind::Playlist => "playlist".into(),
            Kind::Person => "person".into(),
            Kind::Genre => "genre".into(),
            Kind::Studio => "studio".into(),
            Kind::MusicAlbum => "album".into(),
            Kind::MusicArtist => "artist".into(),
            Kind::Audio => "track".into(),
            Kind::Photo => "photo".into(),
            Kind::Book => "book".into(),
            Kind::Folder => "folder".into(),
            Kind::Video => "video".into(),
            Kind::Other => "item".into(),
        }
    }

    /// Something the player can start directly (not a container or a person).
    pub fn playable(&self) -> bool {
        matches!(
            self.kind,
            Kind::Movie | Kind::Episode | Kind::Audio | Kind::Video
        )
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

/// The episode after `id` in `eps`, if any.
pub fn episode_after<'a>(eps: &'a [Media], id: &str) -> Option<&'a Media> {
    let i = eps.iter().position(|e| e.id == id)?;
    eps.get(i + 1)
}

/// The episode before `id` in `eps`, if any.
pub fn episode_before<'a>(eps: &'a [Media], id: &str) -> Option<&'a Media> {
    let i = eps.iter().position(|e| e.id == id)?;
    i.checked_sub(1).and_then(|j| eps.get(j))
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

/// Image container formats `sniff` can tell apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImgFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
}

/// Detects the container so raw bytes can be handed to the UI toolkit's decoder.
pub fn sniff(b: &[u8]) -> Option<ImgFormat> {
    use ImgFormat::*;
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
    disk: Option<Arc<DiskCache>>,
    /// Server identity for cache keys (the base URL; hashed, never written as-is).
    scope: Arc<str>,
    origin: SourceId,
}

/// Backoff before retry `n` (1-based) of a failed image download.
pub fn backoff(n: u32) -> Duration {
    Duration::from_millis(300 * 3u64.pow(n.saturating_sub(1)))
}

impl Source {
    /// A logged-in session: the user id is known, so nothing is asked of the server yet.
    pub fn with_user(cfg: jf::Config, user_id: &str) -> Result<Self, ConnectError> {
        let scope = cfg.base_url.as_str().into();
        let client = jf::Client::new(cfg).map_err(|e| classify(&e))?;
        Ok(Self {
            client: Arc::new(client),
            user: user_id.into(),
            gate: Arc::new(Semaphore::new(6)),
            disk: None,
            scope,
            origin: SourceId::default(),
        })
    }

    /// `None` means no usable configuration was found.
    pub async fn connect(cfg: Option<jf::Config>) -> Result<Self, ConnectError> {
        let cfg = cfg.ok_or(ConnectError::NotConfigured)?;
        let scope = cfg.base_url.as_str().into();
        let client = jf::Client::new(cfg).map_err(|e| classify(&e))?;
        let user = client.first_user_id().await.map_err(|e| classify(&e))?;
        Ok(Self {
            client: Arc::new(client),
            user: user.into(),
            gate: Arc::new(Semaphore::new(6)),
            disk: None,
            scope,
            origin: SourceId::default(),
        })
    }

    /// Every `Media` this source returns carries `origin`.
    pub fn with_origin(mut self, origin: SourceId) -> Self {
        self.origin = origin;
        self
    }

    pub fn origin(&self) -> SourceId {
        self.origin
    }

    /// Images are then kept on disk (bounded LRU) between runs.
    pub fn with_disk(mut self, d: Arc<DiskCache>) -> Self {
        self.disk = Some(d);
        self
    }

    fn media(&self, r: jf::Result<jf::Items>) -> Result<Vec<Media>, Failed> {
        r.map(|i| {
            i.items
                .iter()
                .map(|x| Media::from_item_in(self.origin, x))
                .collect()
        })
        .map_err(|_| Failed)
    }

    /// The user's libraries for the top nav. Collections and Playlists are views of their own
    /// (always in the nav), so a server library with that type or name is not listed twice.
    pub async fn libraries(&self) -> Result<Vec<Media>, Failed> {
        let mut r = self.client.libraries(&self.user).await;
        if let Ok(i) = &mut r {
            i.items
                .retain(|x| !is_global_view(x.collection_type.as_deref(), &x.name));
        }
        self.media(r)
    }

    /// What the server's own UI is made of (home sections, library order, branding), for Server
    /// UI mode. Branding is public and optional: without it only the CSS and disclaimer are empty.
    pub async fn server_ui(&self) -> Result<crate::server_ui::ServerUi, Failed> {
        let prefs = self.client.display_prefs(&self.user).await;
        let cfg = self.client.user_config(&self.user).await;
        let branding = self.client.branding().await.unwrap_or_default();
        match (prefs, cfg) {
            (Ok(p), Ok(c)) => {
                let mut ui = crate::server_ui::ServerUi::new(&p, &c, &branding);
                if let Some(w) = crate::webcss::fetch(&self.client, &self.scope).await {
                    ui.web_css = Some(w.css.into());
                    ui.web_font = w.font.map(|(f, b)| (f, b.into()));
                }
                Ok(ui)
            }
            _ => Err(Failed),
        }
    }

    pub async fn latest(&self, parent: &Media) -> Result<Vec<Media>, Failed> {
        // `Items/Latest` groups new episodes under their show; if a server refuses it, fall back
        // to the plain newest-first query.
        let r = match self.client.latest(&self.user, &parent.id, 16).await {
            Ok(items) => Ok(jf::Items {
                items,
                ..Default::default()
            }),
            Err(_) => {
                self.client
                    .items(&self.user, &latest_query(parent, 16))
                    .await
            }
        };
        self.media(r)
    }

    pub async fn seasons(&self, series: &str) -> Result<Vec<Media>, Failed> {
        self.media(self.client.seasons(&self.user, series).await)
    }

    pub async fn episodes(&self, series: &str, season: &str) -> Result<Vec<Media>, Failed> {
        self.media(self.client.episodes(&self.user, series, Some(season)).await)
    }

    /// Direct URL of the title logo (no credentials in it), bounded to `max_w` x `max_h`; `None` without a logo.
    pub fn logo_url(&self, m: &Media, max_w: u32, max_h: u32) -> Option<String> {
        let (id, tag) = m.logo.as_ref()?;
        Some(self.client.image_url_bounded(
            id,
            jf::ImageType::Logo,
            Some(max_w),
            Some(max_h),
            Some(tag),
        ))
    }

    /// Image bytes (GET only). Disk cache first; a failed download is retried with backoff
    /// (not for 4xx: a missing image stays missing). At most a few requests in flight.
    pub async fn image(&self, m: &Media, art: Art, width: u32) -> Result<Vec<u8>, Failed> {
        let own = m.id.as_str();
        let (id, ty, tag) = match art {
            Art::Poster => (own, jf::ImageType::Primary, m.poster_tag.as_deref()),
            Art::Backdrop => (
                m.backdrop_id.as_deref().unwrap_or(own),
                jf::ImageType::Backdrop,
                m.backdrop_tag.as_deref(),
            ),
            Art::Logo => match &m.logo {
                Some((i, t)) => (i.as_str(), jf::ImageType::Logo, Some(t.as_str())),
                None => return Err(Failed),
            },
            Art::Landscape if m.kind == Kind::Episode || m.backdrop_tag.is_none() => {
                (own, jf::ImageType::Primary, m.poster_tag.as_deref())
            }
            Art::Landscape => (
                m.backdrop_id.as_deref().unwrap_or(own),
                jf::ImageType::Backdrop,
                m.backdrop_tag.as_deref(),
            ),
            Art::Chapter => (own, jf::ImageType::Primary, m.poster_tag.as_deref()),
        };
        let tag = tag.ok_or(Failed)?; // no tag = the server has no such image
        let chapter = (art == Art::Chapter).then_some(m.number.unwrap_or(0));
        let key = format!(
            "{}|{}|{ty:?}|{}|{tag}|{width}",
            self.scope,
            id,
            chapter.map_or(String::new(), |c| c.to_string())
        );
        if let Some(b) = self.disk.as_ref().and_then(|d| d.get(&key)) {
            return Ok(b);
        }
        let _permit = self.gate.acquire().await.map_err(|_| Failed)?;
        for attempt in 1..=3u32 {
            let r = match chapter {
                Some(i) => self.client.chapter_image(id, i, width, Some(tag)).await,
                None => {
                    self.client
                        .image_bytes(id, ty, Some(width), Some(tag))
                        .await
                }
            };
            match r {
                Ok(b) => {
                    if let Some(d) = &self.disk {
                        d.put(&key, &b);
                    }
                    return Ok(b);
                }
                Err(jf::Error::Status {
                    status: 400..=499, ..
                }) => return Err(Failed),
                Err(_) if attempt < 3 => tokio::time::sleep(backoff(attempt)).await,
                Err(_) => {}
            }
        }
        Err(Failed)
    }

    /// A user's profile picture (GET only), disk cached under server + user + tag + size so a
    /// changed picture (new tag) is fetched again. A 4xx means the picture is gone.
    pub async fn user_image(&self, uid: &str, tag: &str, size: u32) -> Result<Vec<u8>, Failed> {
        let key = user_image_key(&self.scope, uid, tag, size);
        if let Some(b) = self.disk.as_ref().and_then(|d| d.get(&key)) {
            return Ok(b);
        }
        let _permit = self.gate.acquire().await.map_err(|_| Failed)?;
        for attempt in 1..=3u32 {
            match self.client.user_image_bytes(uid, size, tag).await {
                Ok(b) => {
                    if let Some(d) = &self.disk {
                        d.put(&key, &b);
                    }
                    return Ok(b);
                }
                Err(jf::Error::Status {
                    status: 400..=499, ..
                }) => return Err(Failed),
                Err(_) if attempt < 3 => tokio::time::sleep(backoff(attempt)).await,
                Err(_) => {}
            }
        }
        Err(Failed)
    }

    // ---- browse ----

    /// One page of a filtered/sorted query plus the server's total.
    pub async fn browse(&self, q: &jf::BrowseQuery) -> Result<(Vec<Media>, usize), Failed> {
        let r = self
            .client
            .browse(&self.user, q)
            .await
            .map_err(|_| Failed)?;
        Ok((
            r.items
                .iter()
                .map(|x| Media::from_item_in(self.origin, x))
                .collect(),
            r.total_record_count as usize,
        ))
    }

    pub async fn filter_options(
        &self,
        parent: Option<&str>,
        types: &[String],
    ) -> Result<jf::FilterOptions, Failed> {
        self.client
            .filter_options(&self.user, parent, types)
            .await
            .map_err(|_| Failed)
    }

    /// `(id, name)` of every studio under a parent.
    pub async fn studios(&self, parent: Option<&str>) -> Result<Vec<(String, String)>, Failed> {
        let r = self
            .client
            .studios(&self.user, parent)
            .await
            .map_err(|_| Failed)?;
        Ok(r.items.into_iter().map(|i| (i.id, i.name)).collect())
    }

    /// The full record: the item itself plus cast, chapters, streams and the rest.
    pub async fn full(&self, id: &str) -> Result<(Media, Full), Failed> {
        let i = self.client.item(&self.user, id).await.map_err(|_| Failed)?;
        let mut full = Full::from_item(&i);
        for p in full.people.iter_mut().chain(&mut full.chapters) {
            p.origin = self.origin;
        }
        Ok((Media::from_item_in(self.origin, &i), full))
    }

    pub async fn similar(&self, id: &str) -> Result<Vec<Media>, Failed> {
        self.media(self.client.similar(&self.user, id, 16).await)
    }

    /// Collections that include this item; empty on servers without the endpoint (older than 12).
    pub async fn collections_of(&self, id: &str) -> Result<Vec<Media>, Failed> {
        let v = self
            .client
            .collections_of(&self.user, id)
            .await
            .unwrap_or_default();
        Ok(v.iter()
            .map(|x| Media::from_item_in(self.origin, x))
            .collect())
    }

    /// Local trailers and special features.
    pub async fn extras(&self, id: &str) -> Result<Vec<Media>, Failed> {
        let v = self
            .client
            .extras(&self.user, id)
            .await
            .map_err(|_| Failed)?;
        Ok(v.iter()
            .map(|x| Media::from_item_in(self.origin, x))
            .collect())
    }

    /// The playlists this user owns and may edit, by name: `(id, name)`.
    pub async fn own_playlists(&self) -> Result<Vec<(String, String)>, Failed> {
        let q = jf::BrowseQuery {
            include_types: vec!["Playlist".into()],
            sort_by: vec!["SortName".into()],
            recursive: true,
            limit: Some(200),
            ..Default::default()
        };
        let (items, _) = self.browse(&q).await?;
        Ok(items
            .into_iter()
            .filter(|m| m.can_delete)
            .map(|m| (m.id, m.title))
            .collect())
    }

    pub async fn playlist_items(&self, id: &str) -> Result<Vec<Media>, Failed> {
        self.media(self.client.playlist_items(&self.user, id).await)
    }

    /// Direct children in their natural order (album tracks, collection members, folders).
    pub async fn ordered_children(&self, id: &str) -> Result<Vec<Media>, Failed> {
        let q = jf::BrowseQuery {
            parent_id: Some(id.into()),
            sort_by: ["ParentIndexNumber", "IndexNumber", "SortName"]
                .map(String::from)
                .to_vec(),
            ..Default::default()
        };
        Ok(self.browse(&q).await?.0)
    }

    /// Everything a person appears in, of one or more types.
    pub async fn filmography(&self, person: &str, types: &[&str]) -> Result<Vec<Media>, Failed> {
        let q = jf::BrowseQuery {
            person_ids: vec![person.into()],
            include_types: types.iter().map(|t| t.to_string()).collect(),
            sort_by: vec!["PremiereDate".into(), "SortName".into()],
            descending: true,
            recursive: true,
            limit: Some(60),
            ..Default::default()
        };
        Ok(self.browse(&q).await?.0)
    }

    /// Albums of an artist, newest first.
    pub async fn artist_albums(&self, id: &str) -> Result<Vec<Media>, Failed> {
        let q = jf::BrowseQuery {
            album_artist_ids: vec![id.into()],
            include_types: vec!["MusicAlbum".into()],
            sort_by: vec!["PremiereDate".into(), "SortName".into()],
            descending: true,
            recursive: true,
            ..Default::default()
        };
        Ok(self.browse(&q).await?.0)
    }

    /// Mixed ranked search results (grouped by the caller).
    pub async fn search(&self, term: &str) -> Result<Vec<Media>, Failed> {
        let h = self
            .client
            .search_hints(&self.user, term, 60)
            .await
            .map_err(|_| Failed)?;
        Ok(h.iter()
            .map(|x| Media::from_hint_in(self.origin, x))
            .collect())
    }

    /// Episodes before and after `ep` in its series.
    pub async fn episode_around(&self, ep: &Media) -> (Option<Media>, Option<Media>) {
        let Some(series) = ep.series_id.as_deref() else {
            return (None, None);
        };
        let Ok(eps) = self.all_episodes(series).await else {
            return (None, None);
        };
        (
            episode_before(&eps, &ep.id).cloned(),
            episode_after(&eps, &ep.id).cloned(),
        )
    }

    /// Items with a saved position, most recently watched first.
    pub async fn resume(&self) -> Result<Vec<Media>, Failed> {
        self.media(self.client.resume(&self.user, 12).await)
    }

    /// Next unwatched episode of each followed series.
    pub async fn next_up(&self) -> Result<Vec<Media>, Failed> {
        self.media(self.client.next_up(&self.user, 12).await)
    }

    /// The episode to watch next in one series, if any.
    pub async fn series_next_up(&self, series: &str) -> Result<Vec<Media>, Failed> {
        self.media(self.client.series_next_up(&self.user, series).await)
    }

    async fn all_episodes(&self, series: &str) -> Result<Vec<Media>, Failed> {
        self.media(self.client.episodes(&self.user, series, None).await)
    }

    /// The episode "Play" should start for a series.
    pub async fn series_target(&self, series: &str) -> Result<Media, Failed> {
        if let Some(e) = self.series_next_up(series).await?.into_iter().next() {
            return Ok(e);
        }
        let eps = self.all_episodes(series).await?;
        pick_episode(&eps).cloned().ok_or(Failed)
    }

    /// Provider ids of items by id (`GET Items?ids=`, batched), for matching series across servers.
    pub async fn provider_ids(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, ProviderIds>, Failed> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(50) {
            let q = jf::ItemsQuery {
                ids: chunk.to_vec(),
                ..Default::default()
            };
            let r = self
                .client
                .items(&self.user, &q)
                .await
                .map_err(|_| Failed)?;
            for i in r.items {
                out.insert(i.id.clone(), provider_ids(&i.x.provider_ids));
            }
        }
        Ok(out)
    }

    /// What the latest requests say about the connection.
    pub fn health(&self) -> jf::Health {
        self.client.health()
    }

    /// The name the server announces (`GET /System/Info/Public`).
    pub async fn server_name(&self) -> Option<String> {
        self.client
            .public_info()
            .await
            .ok()
            .map(|i| i.server_name)
            .filter(|n| !n.trim().is_empty())
    }

    /// A cheap unauthenticated read, used to notice the server coming back.
    pub async fn ping(&self) -> bool {
        self.client.public_info().await.is_ok()
    }

    /// The client and user id, for the player page's own reads.
    pub fn client(&self) -> (Arc<jf::Client>, String) {
        (self.client.clone(), self.user.to_string())
    }
}

/// Cache key of one drawn image. It includes the tags the server gave the item, so an item
/// first seen without tags (a search hit) does not keep its "missing" mark once the full
/// record brings the tags.
pub fn image_key(m: &Media, art: Art, width: u32, extra: u32) -> String {
    format!(
        "{}:{}:{:?}:{width}:{extra}:{:?}{:?}{:?}{:?}",
        m.origin.0, m.id, art, m.poster_tag, m.backdrop_tag, m.backdrop_id, m.logo
    )
}

/// Disk-cache key of a profile picture (the tag changes whenever the picture does).
pub fn user_image_key(scope: &str, uid: &str, tag: &str, size: u32) -> String {
    format!("{scope}|user|{uid}|{tag}|{size}")
}

/// Server views that the nav already offers as Collections and Playlists.
fn is_global_view(collection_type: Option<&str>, name: &str) -> bool {
    matches!(collection_type, Some("boxsets" | "playlists"))
        || ["collections", "playlists"].contains(&name.trim().to_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    #[test]
    fn logo_is_the_items_own_else_the_shows() {
        use super::*;
        let own: jf::Item = serde_json::from_str(
            r#"{"Id":"m1","Name":"A","Type":"Movie","ImageTags":{"Logo":"lt"}}"#,
        )
        .unwrap();
        assert_eq!(
            Media::from_item(&own).logo,
            Some(("m1".into(), "lt".into()))
        );
        let ep: jf::Item = serde_json::from_str(
            r#"{"Id":"e1","Name":"B","Type":"Episode","ParentLogoItemId":"s1","ParentLogoImageTag":"pt"}"#,
        )
        .unwrap();
        assert_eq!(Media::from_item(&ep).logo, Some(("s1".into(), "pt".into())));
        let none: jf::Item = serde_json::from_str(r#"{"Id":"x","Name":"C"}"#).unwrap();
        assert_eq!(Media::from_item(&none).logo, None);
    }

    #[test]
    fn user_image_key_follows_server_user_tag_and_size() {
        use super::user_image_key as k;
        let a = k("http://s", "u1", "t1", 96);
        assert_eq!(a, k("http://s", "u1", "t1", 96));
        for other in [
            k("http://s2", "u1", "t1", 96),
            k("http://s", "u2", "t1", 96),
            k("http://s", "u1", "t2", 96),
            k("http://s", "u1", "t1", 128),
        ] {
            assert_ne!(a, other);
        }
    }

    #[test]
    fn image_key_changes_when_the_server_supplies_tags() {
        use super::*;
        let bare = Media::from_item(&jf::Item {
            id: "s1".into(),
            ..Default::default()
        });
        let mut full = bare.clone();
        full.backdrop_tag = Some("t".into());
        assert_ne!(
            image_key(&bare, Art::Backdrop, 1920, 0),
            image_key(&full, Art::Backdrop, 1920, 0),
            "a search hit's failed slot is not reused for the full item"
        );
    }

    #[test]
    fn collections_and_playlists_are_not_listed_as_libraries() {
        use super::is_global_view as g;
        assert!(g(Some("boxsets"), "Anything") && g(Some("playlists"), "x"));
        assert!(g(None, "Collections") && g(Some("movies"), " playlists "));
        assert!(!g(Some("movies"), "Films") && !g(None, "Mixed"));
    }

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
        assert_eq!(Media::from_item(&item("Tombstone")).kind, Kind::Other);
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
            ..Default::default()
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
    fn episode_neighbours_and_backoff() {
        let ep = |id: &str| Media {
            id: id.into(),
            ..Media::from_item(&item("Episode"))
        };
        let eps = [ep("a"), ep("b"), ep("c")];
        assert_eq!(episode_before(&eps, "b").unwrap().id, "a");
        assert!(episode_before(&eps, "a").is_none());
        assert!(episode_before(&eps, "zz").is_none());
        assert_eq!(backoff(1), Duration::from_millis(300));
        assert_eq!(backoff(2), Duration::from_millis(900));
    }

    #[test]
    fn blurhash_follows_the_art_shown() {
        let mut i = item("Movie");
        i.image_tags.insert("Primary".into(), "p".into());
        i.backdrop_image_tags = vec!["b".into()];
        i.x.image_blur_hashes.insert(
            "Primary".into(),
            [("p".to_string(), "HP".to_string())].into(),
        );
        i.x.image_blur_hashes.insert(
            "Backdrop".into(),
            [("b".to_string(), "HB".to_string())].into(),
        );
        let m = Media::from_item(&i);
        assert_eq!(m.blur_for(Art::Poster), Some("HP"));
        assert_eq!(m.blur_for(Art::Backdrop), Some("HB"));
        assert_eq!(m.blur_for(Art::Landscape), Some("HB"));
        i.kind = "Episode".into();
        assert_eq!(Media::from_item(&i).blur_for(Art::Landscape), Some("HP"));
        assert_eq!(m.blur_for(Art::Chapter), None);
    }

    #[test]
    fn new_kinds_and_hints_map() {
        for (t, k) in [
            ("BoxSet", Kind::BoxSet),
            ("Playlist", Kind::Playlist),
            ("Person", Kind::Person),
            ("MusicAlbum", Kind::MusicAlbum),
            ("Audio", Kind::Audio),
            ("Photo", Kind::Photo),
        ] {
            assert_eq!(Media::from_item(&item(t)).kind, k);
        }
        let h = jf::SearchHint {
            id: "old".into(),
            name: "Hit".into(),
            kind: "Episode".into(),
            series: Some("Show".into()),
            ..Default::default()
        };
        let m = Media::from_hint(&h);
        assert_eq!((m.id.as_str(), m.kind), ("old", Kind::Episode));
        assert_eq!(m.sub.as_deref(), Some("Show"));
    }

    #[test]
    fn origin_and_provider_ids_come_from_the_item() {
        let i: jf::Item = serde_json::from_str(
            r#"{"Id":"1","Type":"Movie","ProviderIds":{"Imdb":"tt01","tmdb":"7","Tvdb":null,"Zap2It":"z"}}"#,
        )
        .unwrap();
        let m = Media::from_item_in(SourceId(4), &i);
        assert_eq!(m.origin, SourceId(4));
        assert_eq!(
            (m.ids.imdb.as_deref(), m.ids.tmdb.as_deref(), m.ids.tvdb),
            (Some("tt01"), Some("7"), None)
        );
        assert_eq!(Media::from_item(&i).origin, SourceId(0));
    }

    #[test]
    fn image_key_includes_the_origin() {
        let a = Media::from_item_in(
            SourceId(1),
            &jf::Item {
                id: "same".into(),
                ..Default::default()
            },
        );
        let b = Media {
            origin: SourceId(2),
            ..a.clone()
        };
        assert_ne!(
            image_key(&a, Art::Poster, 300, 0),
            image_key(&b, Art::Poster, 300, 0)
        );
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
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0]), Some(ImgFormat::Png));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(ImgFormat::Jpeg));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImgFormat::Webp));
        assert_eq!(sniff(b"<html>"), None);
    }
}
