//! Object-safe views of the `nocturne-arr-types` traits (they use `impl Future`, which `dyn` cannot hold), implemented for every implementation, so modules swap the torrent engine and indexer client at run time (real ones, or fakes in tests).
use crate::module::BoxFut;
use nocturne_arr_types::{
    AddOptions, Caps, Downloader, Indexer, IndexerDef, Limits, MediaKind, ParsedRelease, Release,
    Result, SearchQuery, TorrentSource, TorrentStatus,
};
use std::sync::{Arc, RwLock};

pub trait DynDownloader: Send + Sync + 'static {
    fn add(&self, src: TorrentSource, opts: AddOptions) -> BoxFut<'_, Result<String>>;
    fn pause<'a>(&'a self, id: &'a str) -> BoxFut<'a, Result<()>>;
    fn resume<'a>(&'a self, id: &'a str) -> BoxFut<'a, Result<()>>;
    fn remove<'a>(&'a self, id: &'a str, delete_files: bool) -> BoxFut<'a, Result<()>>;
    fn list(&self) -> BoxFut<'_, Result<Vec<TorrentStatus>>>;
    fn set_limits(&self, limits: Limits) -> BoxFut<'_, Result<()>>;
}

impl<T: Downloader> DynDownloader for T {
    fn add(&self, src: TorrentSource, opts: AddOptions) -> BoxFut<'_, Result<String>> {
        Box::pin(Downloader::add(self, src, opts))
    }
    fn pause<'a>(&'a self, id: &'a str) -> BoxFut<'a, Result<()>> {
        Box::pin(Downloader::pause(self, id))
    }
    fn resume<'a>(&'a self, id: &'a str) -> BoxFut<'a, Result<()>> {
        Box::pin(Downloader::resume(self, id))
    }
    fn remove<'a>(&'a self, id: &'a str, delete_files: bool) -> BoxFut<'a, Result<()>> {
        Box::pin(Downloader::remove(self, id, delete_files))
    }
    fn list(&self) -> BoxFut<'_, Result<Vec<TorrentStatus>>> {
        Box::pin(Downloader::list(self))
    }
    fn set_limits(&self, limits: Limits) -> BoxFut<'_, Result<()>> {
        Box::pin(Downloader::set_limits(self, limits))
    }
}

pub trait DynIndexer: Send + Sync + 'static {
    fn caps<'a>(&'a self, def: &'a IndexerDef, key: Option<&'a str>) -> BoxFut<'a, Result<Caps>>;
    fn search<'a>(
        &'a self,
        def: &'a IndexerDef,
        key: Option<&'a str>,
        q: &'a SearchQuery,
    ) -> BoxFut<'a, Result<Vec<Release>>>;
    fn rss<'a>(
        &'a self,
        def: &'a IndexerDef,
        key: Option<&'a str>,
        kind: MediaKind,
    ) -> BoxFut<'a, Result<Vec<Release>>>;
    fn fetch<'a>(
        &'a self,
        def: &'a IndexerDef,
        key: Option<&'a str>,
        url: &'a str,
    ) -> BoxFut<'a, Result<TorrentSource>>;
    fn parse(&self, name: &str) -> ParsedRelease;
}

impl<T: Indexer> DynIndexer for T {
    fn caps<'a>(&'a self, def: &'a IndexerDef, key: Option<&'a str>) -> BoxFut<'a, Result<Caps>> {
        Box::pin(Indexer::caps(self, def, key))
    }
    fn search<'a>(
        &'a self,
        def: &'a IndexerDef,
        key: Option<&'a str>,
        q: &'a SearchQuery,
    ) -> BoxFut<'a, Result<Vec<Release>>> {
        Box::pin(Indexer::search(self, def, key, q))
    }
    fn rss<'a>(
        &'a self,
        def: &'a IndexerDef,
        key: Option<&'a str>,
        kind: MediaKind,
    ) -> BoxFut<'a, Result<Vec<Release>>> {
        Box::pin(Indexer::rss(self, def, key, kind))
    }
    fn fetch<'a>(
        &'a self,
        def: &'a IndexerDef,
        key: Option<&'a str>,
        url: &'a str,
    ) -> BoxFut<'a, Result<TorrentSource>> {
        Box::pin(Indexer::fetch(self, def, key, url))
    }
    fn parse(&self, name: &str) -> ParsedRelease {
        Indexer::parse(self, name)
    }
}

/// The running torrent engine, set by the downloads module (`sh.ext::<DlSlot>()`); `None` while that module is off.
#[derive(Default)]
pub struct DlSlot(pub RwLock<Option<Arc<dyn DynDownloader>>>);

impl DlSlot {
    pub fn get(&self) -> std::result::Result<Arc<dyn DynDownloader>, String> {
        self.0
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| "the downloads module is off".to_string())
    }
}
