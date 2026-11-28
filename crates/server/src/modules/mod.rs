//! The modules of this build, in start order. One line per module (see `crate::module` for how to add one).
use crate::fake::{FakeDownloader, FakeIndexer, FakeMeta};
use crate::library::Engine;
use crate::meta::Online;
use crate::module::Module;
use crate::real::Indexers;
use std::sync::Arc;

pub mod discovery;
pub mod downloads;
pub mod gateway;
pub mod library;
pub mod requests;
pub mod sources;
pub mod storage;
pub mod storage_agent;
pub mod tunnel;

/// The real modules. A debug build run with `NOCTURNE_SERVER_FAKE=1` uses the in-memory torrent engine, indexer and metadata instead (nothing leaves the machine; for UI work).
pub fn all() -> Vec<Arc<dyn Module>> {
    if cfg!(debug_assertions) && std::env::var_os("NOCTURNE_SERVER_FAKE").is_some_and(|v| v == "1")
    {
        let dl = Arc::new(FakeDownloader {
            auto: true,
            ..Default::default()
        });
        let ix = Arc::new(FakeIndexer {
            synth: true,
            ..Default::default()
        });
        return with(
            downloads::Backend::Fake(dl),
            Engine::new(ix, Arc::new(FakeMeta::default())),
        );
    }
    with(
        downloads::Backend::Real,
        Engine::new(Arc::new(Indexers::default()), Arc::new(Online::default())),
    )
}

/// The module list with a given torrent backend and library engine (tests build theirs with fakes).
pub fn with(dl: downloads::Backend, engine: Arc<Engine>) -> Vec<Arc<dyn Module>> {
    vec![
        Arc::new(downloads::Downloads::new(dl)),
        Arc::new(library::Library::new(engine)),
        Arc::new(requests::Requests),
        Arc::new(storage::Storage::default()),
        Arc::new(sources::Sources::default()),
        Arc::new(gateway::Gateway::default()),
        Arc::new(tunnel::Tunnel),
        Arc::new(discovery::Discovery),
    ]
}
