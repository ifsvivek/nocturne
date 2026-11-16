//! The reads a detail page needs beyond `Backend`'s browse set (extras, album tracks, playlist items,
//! neighbouring episodes, the episode "Play" starts for a show), enum-dispatched like the rest.
//! A local source answers what it can and empty for the rest. GET only.
use crate::backend::{Backend, PlayError, PlayTarget};
use crate::source::{Failed, Media, episode_after, episode_before, pick_episode};
use crate::versions::with_source;

impl Backend {
    /// Local trailers and special features.
    pub async fn extras(&self, id: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.extras(id).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    /// Album tracks / collection members in their natural order.
    pub async fn ordered_children(&self, id: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.ordered_children(id).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    pub async fn artist_albums(&self, id: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.artist_albums(id).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    pub async fn playlist_items(&self, id: &str) -> Result<Vec<Media>, Failed> {
        match self {
            Self::Jellyfin(s) => s.playlist_items(id).await,
            Self::Local(_) => Ok(vec![]),
        }
    }

    /// Every episode of a show, season by season.
    async fn all_episodes(&self, series: &str) -> Result<Vec<Media>, Failed> {
        let mut v = vec![];
        for s in self.seasons(series).await? {
            v.extend(self.episodes(series, &s.id).await?);
        }
        Ok(v)
    }

    /// The episodes before and after `ep` in its show.
    pub async fn episode_around(&self, ep: &Media) -> (Option<Media>, Option<Media>) {
        if let Self::Jellyfin(s) = self {
            return s.episode_around(ep).await;
        }
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

    /// The episode "Play" starts for a show: the next-up one, else the first unwatched.
    pub async fn series_target(&self, series: &str) -> Result<Media, Failed> {
        if let Ok(v) = self.series_next_up(series).await
            && let Some(e) = v.into_iter().next()
        {
            return Ok(e);
        }
        let eps = self.all_episodes(series).await?;
        pick_episode(&eps).cloned().ok_or(Failed)
    }

    /// [`Backend::play_target`] for one chosen version of a server item (`source_id` = its media
    /// source id); a local file has only one version.
    pub async fn play_target_for(
        &self,
        m: &Media,
        source_id: Option<&str>,
    ) -> Result<PlayTarget, PlayError> {
        let t = self.play_target(m).await?;
        Ok(match source_id {
            Some(id) => with_source(t, id),
            None => t,
        })
    }
}
