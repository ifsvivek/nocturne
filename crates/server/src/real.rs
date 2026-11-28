//! The real engines behind the contract traits: [`Torrent`] wraps `nocturne_torrent::Engine` as a [`Downloader`], [`Indexers`] wraps `nocturne_indexer` as an [`Indexer`]. Pure mapping; the engines own the network.
use nocturne_arr_types::{
    AddOptions, Caps, Category, Downloader, Error, Ids, Indexer, IndexerDef, Limits, Link,
    MediaKind, ParsedRelease, Protocol, Quality, Release, Result, SearchQuery, TorrentFile,
    TorrentSource, TorrentState, TorrentStatus,
};
use nocturne_indexer as ix;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

pub struct Torrent(pub nocturne_torrent::Engine);

fn terr(e: nocturne_torrent::Error) -> Error {
    Error::Other(e.to_string())
}

/// `TorrentInfo` in the contract's shape: file paths relative to the save path, top folder included.
pub fn status(i: &nocturne_torrent::TorrentInfo) -> TorrentStatus {
    use nocturne_torrent::State as S;
    let (state, error) = match &i.state {
        S::Queued => (TorrentState::Queued, None),
        S::Checking => (TorrentState::Checking, None),
        S::Metadata => (TorrentState::Metadata, None),
        S::Downloading | S::Stalled => (TorrentState::Downloading, None),
        // Finished and no longer uploading still counts as complete for the import.
        S::Seeding | S::Completed => (TorrentState::Seeding, None),
        S::Paused => (TorrentState::Paused, None),
        S::Error(m) => (TorrentState::Error, Some(m.clone())),
    };
    let top = i
        .content_path
        .strip_prefix(&i.save_path)
        .unwrap_or(Path::new(""));
    let files = i
        .files
        .iter()
        .filter(|f| f.selected)
        .map(|f| TorrentFile {
            path: if i.content_path == i.save_path.join(&f.path) {
                f.path.clone()
            } else {
                top.join(&f.path)
            },
            size: f.size,
            done: f.downloaded,
        })
        .collect();
    TorrentStatus {
        id: i.id.clone(),
        name: i.name.clone(),
        state,
        error,
        category: i.category.clone().unwrap_or_default(),
        save_path: i.save_path.clone(),
        size: i.size,
        done: i.downloaded,
        uploaded: i.uploaded,
        down_rate: i.download_rate,
        up_rate: i.upload_rate,
        peers: i.peers,
        eta_secs: i.eta.map(|d| d.as_secs()),
        files,
        added: i
            .added
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    }
}

pub fn torrent_limits(l: Limits) -> nocturne_torrent::Limits {
    let c = |v: Option<u64>| v.map(|b| b.min(u64::from(u32::MAX)) as u32);
    nocturne_torrent::Limits {
        download_bps: c(l.down_bps),
        upload_bps: c(l.up_bps),
    }
}

impl Downloader for Torrent {
    async fn add(&self, src: TorrentSource, opts: AddOptions) -> Result<String> {
        let src = match src {
            TorrentSource::Magnet(m) => nocturne_torrent::Source::Magnet(m),
            TorrentSource::Bytes(b) => nocturne_torrent::Source::TorrentFile(b),
        };
        self.0
            .add(
                src,
                nocturne_torrent::AddOptions {
                    save_path: Some(opts.save_path),
                    category: Some(opts.category),
                    paused: opts.paused,
                    ..Default::default()
                },
            )
            .await
            .map_err(terr)
    }
    async fn pause(&self, id: &str) -> Result<()> {
        self.0.pause(id).await.map_err(terr)
    }
    async fn resume(&self, id: &str) -> Result<()> {
        self.0.resume(id).await.map_err(terr)
    }
    async fn remove(&self, id: &str, delete_files: bool) -> Result<()> {
        self.0.remove(id, delete_files).await.map_err(terr)
    }
    async fn list(&self) -> Result<Vec<TorrentStatus>> {
        Ok(self.0.list().iter().map(status).collect())
    }
    async fn set_limits(&self, limits: Limits) -> Result<()> {
        self.0.set_limits(torrent_limits(limits));
        Ok(())
    }
}

/// The contract's quality for the parser's (same names where both have one).
pub fn quality(q: ix::Quality) -> Quality {
    use ix::Quality as I;
    match q {
        I::WorkPrint => Quality::Cam,
        I::Regional | I::DvdScr => Quality::Telecine,
        I::DvdR => Quality::Dvd,
        I::BrDisk => Quality::Remux1080,
        I::RawHd => Quality::Hdtv1080,
        I::Mp3Unknown | I::Mp3_128 => Quality::Mp3Low,
        I::Mp3VbrV2 | I::Mp3VbrV0 => Quality::Mp3Vbr,
        I::Wav | I::Ape | I::WavPack => Quality::Flac,
        I::AudioMp3 => Quality::AudiobookMp3,
        I::AudioM4b | I::AudioFlac => Quality::AudiobookM4b,
        other => Quality::VIDEO
            .iter()
            .chain(Quality::AUDIO)
            .chain(Quality::BOOK)
            .copied()
            .find(|m| m.label() == other.name())
            .unwrap_or(Quality::Unknown),
    }
}

fn ix_kind(k: MediaKind) -> ix::MediaKind {
    match k {
        MediaKind::Movie => ix::MediaKind::Movie,
        MediaKind::Series => ix::MediaKind::Tv,
        MediaKind::Music => ix::MediaKind::Music,
        MediaKind::Book => ix::MediaKind::Book,
    }
}

fn video(p: &mut ParsedRelease, q: ix::QualityInfo) {
    p.quality = quality(q.quality);
    p.codec = q.codec.map(String::from);
    p.group = q.group;
    p.proper = q.proper;
    p.repack = q.repack || q.version > 2;
    p.languages = q.languages.iter().map(|l| l.to_string()).collect();
}

/// Parse a name as a release of `kind`.
pub fn parse_as(name: &str, kind: MediaKind) -> ParsedRelease {
    let mut p = ParsedRelease::default();
    match kind {
        MediaKind::Movie => {
            let m = ix::parse_movie(name);
            (p.title, p.year) = (m.title, m.year);
            video(&mut p, m.quality);
        }
        MediaKind::Series => match ix::parse_episode(name) {
            Some(e) => {
                (p.title, p.year, p.season, p.episodes, p.full_season) =
                    (e.series, e.year, e.season, e.episodes, e.full_season);
                video(&mut p, e.quality);
            }
            None => video(&mut p, ix::parse_quality(name)),
        },
        MediaKind::Music => {
            let m = ix::parse_music(name);
            (p.title, p.work, p.year, p.quality, p.group) =
                (m.artist, Some(m.album), m.year, quality(m.quality), m.group);
        }
        MediaKind::Book => {
            let b = ix::parse_book(name);
            (p.title, p.work, p.year, p.quality, p.group) =
                (b.author, Some(b.title), b.year, quality(b.quality), b.group);
        }
    }
    p.work = p.work.filter(|w| !w.is_empty());
    p
}

fn ierr(e: ix::Error) -> Error {
    match e {
        ix::Error::Unauthorized => Error::Auth,
        ix::Error::Backoff(d) => Error::RateLimited(Some(d.as_secs())),
        ix::Error::Status(404) => Error::NotFound,
        other => Error::Network(other.to_string()),
    }
}

/// One client per indexer, kept so its rate limiter and caps survive between calls (rebuilt when the address or key changes).
#[derive(Default)]
pub struct Indexers {
    clients: Mutex<HashMap<u64, (String, String, ix::Indexer)>>,
}

impl Indexers {
    fn client(&self, def: &IndexerDef, key: Option<&str>) -> Result<ix::Indexer> {
        let key = key.unwrap_or("");
        let mut m = self.clients.lock().unwrap();
        if let Some((u, k, c)) = m.get(&def.id)
            && *u == def.url
            && k == key
        {
            return Ok(c.clone());
        }
        let proto = match def.protocol {
            Protocol::Torrent => ix::Protocol::Torrent,
            Protocol::Usenet => ix::Protocol::Usenet,
        };
        // Public-tracker feeds through Jackett or Prowlarr want a little spacing.
        let c = ix::Indexer::new(def.id as i64, &def.url, key, proto, Duration::from_secs(2))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        m.insert(def.id, (def.url.clone(), key.to_string(), c.clone()));
        Ok(c)
    }

    fn release(def: &IndexerDef, kind: MediaKind, r: ix::Release) -> Option<Release> {
        let link = match (r.magnet, r.download_url) {
            (Some(m), _) => Link::Magnet(m),
            (None, Some(u)) => Link::Url(u),
            (None, None) => return None,
        };
        let parsed = parse_as(&r.title, kind);
        Some(Release {
            guid: r.guid,
            indexer_id: def.id,
            indexer: def.name.clone(),
            protocol: match r.protocol {
                ix::Protocol::Torrent => Protocol::Torrent,
                ix::Protocol::Usenet => Protocol::Usenet,
            },
            title: r.title,
            size: r.size.unwrap_or(0),
            seeders: r.seeders,
            leechers: r.leechers,
            link,
            info_hash: r.info_hash,
            published: r.publish_date.and_then(|d| u64::try_from(d).ok()),
            categories: r.categories,
            ids: Ids {
                imdb: r.imdb_id,
                tmdb: r.tmdb_id.map(u64::from),
                tvdb: r.tvdb_id.map(u64::from),
                ..Default::default()
            },
            parsed,
        })
    }
}

fn caps(c: ix::Caps) -> Caps {
    Caps {
        search: c.search,
        movie: c.movie,
        tv: c.tv,
        music: c.music,
        book: c.book,
        categories: c
            .categories
            .into_iter()
            .map(|x| Category {
                id: x.id,
                name: x.name,
            })
            .collect(),
        limit_max: c.limit_max,
    }
}

impl Indexer for Indexers {
    async fn caps(&self, def: &IndexerDef, key: Option<&str>) -> Result<Caps> {
        self.client(def, key)?.caps().await.map(caps).map_err(ierr)
    }

    async fn search(
        &self,
        def: &IndexerDef,
        key: Option<&str>,
        q: &SearchQuery,
    ) -> Result<Vec<Release>> {
        let c = self.client(def, key)?;
        let small = |v: Option<u64>| v.and_then(|x| u32::try_from(x).ok());
        let iq = ix::SearchQuery {
            kind: Some(ix_kind(q.kind)),
            text: Some(q.text.clone()),
            year: q.year,
            imdb_id: q.ids.imdb.clone(),
            tmdb_id: small(q.ids.tmdb),
            tvdb_id: small(q.ids.tvdb),
            season: q.season,
            episode: q.episode.map(|e| e.to_string()),
            artist: q.artist.clone(),
            album: q.album.clone(),
            author: q.author.clone(),
            title: q.book.clone(),
            categories: def.categories.clone(),
            ..Default::default()
        };
        let page = c.search(&iq).await.map_err(ierr)?;
        Ok(page
            .releases
            .into_iter()
            .filter_map(|r| Self::release(def, q.kind, r))
            .collect())
    }

    async fn rss(
        &self,
        def: &IndexerDef,
        key: Option<&str>,
        kind: MediaKind,
    ) -> Result<Vec<Release>> {
        let c = self.client(def, key)?;
        let cats = if def.categories.is_empty() {
            ix_kind(kind).default_categories().to_vec()
        } else {
            def.categories.clone()
        };
        let page = c.latest(&cats, 0, None).await.map_err(ierr)?;
        Ok(page
            .releases
            .into_iter()
            .filter(|r| r.kind().is_none_or(|k| k == ix_kind(kind)))
            .filter_map(|r| Self::release(def, kind, r))
            .collect())
    }

    async fn fetch(&self, def: &IndexerDef, key: Option<&str>, url: &str) -> Result<TorrentSource> {
        match self.client(def, key)?.download(url).await.map_err(ierr)? {
            ix::Download::File(b) => Ok(TorrentSource::Bytes(b)),
            ix::Download::Magnet(m) => Ok(TorrentSource::Magnet(m)),
        }
    }

    /// File names inside a download: an episode when it says `S01E02`, else a movie.
    fn parse(&self, name: &str) -> ParsedRelease {
        match ix::parse_episode(name) {
            Some(e) if e.season.is_some() => parse_as(name, MediaKind::Series),
            _ => parse_as(name, MediaKind::Movie),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualities_map_by_name_and_family() {
        assert_eq!(quality(ix::Quality::WebDl1080p), Quality::WebDl1080);
        assert_eq!(quality(ix::Quality::Remux2160p), Quality::Remux2160);
        assert_eq!(quality(ix::Quality::Flac24), Quality::Flac24);
        assert_eq!(quality(ix::Quality::Epub), Quality::Epub);
        assert_eq!(quality(ix::Quality::AudioM4b), Quality::AudiobookM4b);
        assert_eq!(quality(ix::Quality::Mp3VbrV0), Quality::Mp3Vbr);
        // Every parser quality maps somewhere sensible: only Unknown stays unknown.
        for q in ix::Quality::ALL {
            assert_eq!(
                quality(*q) == Quality::Unknown,
                *q == ix::Quality::Unknown,
                "{q:?}"
            );
        }
    }

    #[test]
    fn names_parse_per_kind() {
        let p = parse_as("Night.Train.2020.1080p.BluRay.x264-GRP", MediaKind::Movie);
        assert_eq!(
            (p.title.as_str(), p.year, p.quality),
            ("Night Train", Some(2020), Quality::Bluray1080)
        );
        let p = parse_as("Show.S01E02.720p.WEB-DL.x264-GRP", MediaKind::Series);
        assert_eq!(
            (p.season, p.episodes.clone(), p.full_season),
            (Some(1), vec![2], false)
        );
        let p = Indexers::default().parse("Show.S01E03.mkv");
        assert_eq!((p.season, p.episodes), (Some(1), vec![3]));
    }

    #[test]
    fn torrent_files_are_relative_to_the_save_path() {
        let info = |content: &str, file: &str| nocturne_torrent::TorrentInfo {
            id: "ab".into(),
            name: "n".into(),
            state: nocturne_torrent::State::Completed,
            progress: 1.0,
            size: 1,
            downloaded: 1,
            uploaded: 0,
            download_rate: 0,
            upload_rate: 0,
            eta: None,
            ratio: 0.0,
            peers: 0,
            seeders: 0,
            leechers: 0,
            save_path: "/d/movies".into(),
            content_path: content.into(),
            category: Some("nocturne-movie".into()),
            tags: vec![],
            sequential: false,
            files: vec![nocturne_torrent::FileInfo {
                index: 0,
                path: file.into(),
                size: 1,
                downloaded: 1,
                selected: true,
            }],
            added: UNIX_EPOCH,
            completed: None,
        };
        let multi = status(&info("/d/movies/Pack", "a/x.mkv"));
        assert_eq!(multi.files[0].path, Path::new("Pack/a/x.mkv"));
        assert_eq!(
            multi.state,
            TorrentState::Seeding,
            "finished counts as complete"
        );
        let single = status(&info("/d/movies/x.mkv", "x.mkv"));
        assert_eq!(single.files[0].path, Path::new("x.mkv"));
    }
}
