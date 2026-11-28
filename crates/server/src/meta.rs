//! Where unit lists come from: episodes, albums and books (movies have one unit and need no lookup). [`Online`] uses the providers in `nocturne_core::online` (TVmaze, MusicBrainz, Open Library; no keys); tests use [`crate::fake::FakeMeta`].
use crate::module::BoxFut;
use nocturne_arr_types::{Ids, MediaKind};
use nocturne_core::server::{Title, Unit};

pub trait Metadata: Send + Sync + 'static {
    /// The units of a series, artist or author (`id` and `title` 0), with the ids learned on the way.
    fn units<'a>(&'a self, t: &'a Title) -> BoxFut<'a, Result<(Ids, Vec<Unit>), String>>;
}

pub struct Online(pub nocturne_core::online::Engine);

impl Default for Online {
    fn default() -> Self {
        Self(nocturne_core::online::Engine::new(
            Default::default(),
            Default::default(),
        ))
    }
}

impl Metadata for Online {
    fn units<'a>(&'a self, t: &'a Title) -> BoxFut<'a, Result<(Ids, Vec<Unit>), String>> {
        Box::pin(async move {
            let mut ids = t.ids.clone();
            let err =
                |e: nocturne_core::online::http::Err| format!("metadata lookup failed: {e:?}");
            let units = match t.kind {
                MediaKind::Movie => vec![],
                MediaKind::Series => {
                    let (id, u) = self.0.series_units(&t.ids, &t.name).await.map_err(err)?;
                    ids.tvmaze = Some(id);
                    u
                }
                MediaKind::Music => {
                    let (id, u) = self.0.artist_units(&t.ids, &t.name).await.map_err(err)?;
                    ids.musicbrainz = Some(id);
                    u
                }
                MediaKind::Book => {
                    let (id, u) = self.0.author_units(&t.ids, &t.name).await.map_err(err)?;
                    ids.openlibrary = Some(id);
                    u
                }
            };
            Ok((ids, units))
        })
    }
}
