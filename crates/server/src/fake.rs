//! In-memory stand-ins for the torrent engine, the indexer client and the metadata providers: used by the tests, and by a debug build run with `NOCTURNE_SERVER_FAKE=1` until `nocturne-torrent` and `nocturne-indexer` are wired in. Nothing here touches the network.
use crate::meta::Metadata;
use crate::module::BoxFut;
use nocturne_arr_types::{
    AddOptions, Caps, Downloader, Error, Ids, Indexer, IndexerDef, Limits, Link, MediaKind,
    ParsedRelease, Protocol, Quality, Release, Result, SearchQuery, TorrentFile, TorrentSource,
    TorrentState, TorrentStatus,
};
use nocturne_core::server::{Title, Unit};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// A small release-name parser: enough for the fakes and the tests (`nocturne-indexer` has the real one).
pub fn parse(name: &str) -> ParsedRelease {
    let lower = name.to_lowercase();
    let mut p = ParsedRelease::default();
    // "Artist - Album (Year) [FLAC]" (music, books).
    let (head, work) = match name.split_once(" - ") {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (name.to_string(), None),
    };
    let words: Vec<&str> = head
        .split(['.', ' ', '_', '(', ')', '[', ']'])
        .filter(|w| !w.is_empty())
        .collect();
    let mut title = vec![];
    for w in &words {
        let l = w.to_lowercase();
        let year = l.len() == 4
            && (l.starts_with("19") || l.starts_with("20"))
            && l.bytes().all(|b| b.is_ascii_digit());
        if year {
            p.year = l.parse().ok();
            break;
        }
        if let Some(rest) = l.strip_prefix('s')
            && rest.len() >= 2
            && rest[..2].bytes().all(|b| b.is_ascii_digit())
        {
            p.season = rest[..2].parse().ok();
            let mut eps = &rest[2..];
            while let Some(e) = eps.strip_prefix('e') {
                let n: String = e.chars().take_while(char::is_ascii_digit).collect();
                if n.is_empty() {
                    break;
                }
                p.episodes.push(n.parse().unwrap_or(0));
                eps = &e[n.len()..];
            }
            p.full_season = p.episodes.is_empty();
            break;
        }
        if ["2160p", "1080p", "720p", "480p", "flac", "mp3", "epub"].contains(&l.as_str()) {
            break;
        }
        title.push(*w);
    }
    p.title = title.join(" ");
    if let Some(w) = work {
        let w = w.split(['(', '[']).next().unwrap_or("").trim().to_string();
        p.work = Some(w);
    }
    let has = |s: &str| lower.contains(s);
    let res = if has("2160p") {
        2160
    } else if has("1080p") {
        1080
    } else if has("720p") {
        720
    } else {
        480
    };
    use Quality::*;
    p.quality = if has("flac") {
        if has("24bit") { Flac24 } else { Flac }
    } else if has("320") {
        Mp3_320
    } else if has("mp3") {
        Mp3Vbr
    } else if has("epub") {
        Epub
    } else if has("m4b") {
        AudiobookM4b
    } else if has("pdf") {
        Pdf
    } else if has("remux") {
        if res == 2160 { Remux2160 } else { Remux1080 }
    } else if has("bluray") || has("blu-ray") {
        match res {
            2160 => Bluray2160,
            1080 => Bluray1080,
            720 => Bluray720,
            _ => Bluray480,
        }
    } else if has("web-dl") || has("webdl") {
        match res {
            2160 => WebDl2160,
            1080 => WebDl1080,
            720 => WebDl720,
            _ => WebDl480,
        }
    } else if has("webrip") {
        match res {
            2160 => WebRip2160,
            1080 => WebRip1080,
            720 => WebRip720,
            _ => WebRip480,
        }
    } else if has("hdtv") {
        match res {
            2160 => Hdtv2160,
            1080 => Hdtv1080,
            720 => Hdtv720,
            _ => Sdtv,
        }
    } else if has("cam") {
        Cam
    } else {
        Unknown
    };
    p.proper = has("proper");
    p.repack = has("repack");
    p.group = name
        .rsplit_once('-')
        .map(|(_, g)| g.to_string())
        .filter(|g| !g.contains(' ') && !g.contains('.'));
    p
}

/// A release as an indexer would give it, parsed with [`parse`].
pub fn release(indexer: u64, title: &str, size: u64, seeders: u32) -> Release {
    Release {
        guid: format!("{indexer}:{title}"),
        indexer_id: indexer,
        indexer: format!("fake {indexer}"),
        protocol: Protocol::Torrent,
        title: title.into(),
        size,
        seeders: Some(seeders),
        leechers: Some(0),
        link: Link::Magnet(format!(
            "magnet:?xt=urn:btih:{:040x}&dn={}&xl={size}",
            hash(title),
            title.replace(' ', "+")
        )),
        info_hash: Some(format!("{:040x}", hash(title))),
        published: None,
        categories: vec![],
        ids: Ids::default(),
        parsed: parse(title),
    }
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

/// Per indexer id, the releases every search and RSS call returns. With `synth`, an indexer without entries makes two up from the query (demo mode).
#[derive(Default)]
pub struct FakeIndexer {
    pub releases: Mutex<HashMap<u64, Vec<Release>>>,
    pub synth: bool,
    /// Every query seen (tests).
    pub queries: Mutex<Vec<SearchQuery>>,
}

impl FakeIndexer {
    pub fn set(&self, indexer: u64, rels: Vec<Release>) {
        self.releases.lock().unwrap().insert(indexer, rels);
    }
}

impl Indexer for FakeIndexer {
    async fn caps(&self, _: &IndexerDef, _: Option<&str>) -> Result<Caps> {
        let all = Some(vec!["q".to_string()]);
        Ok(Caps {
            search: all.clone(),
            movie: all.clone(),
            tv: all.clone(),
            music: all.clone(),
            book: all,
            ..Default::default()
        })
    }

    async fn search(
        &self,
        def: &IndexerDef,
        _: Option<&str>,
        q: &SearchQuery,
    ) -> Result<Vec<Release>> {
        self.queries.lock().unwrap().push(q.clone());
        if let Some(r) = self.releases.lock().unwrap().get(&def.id) {
            return Ok(r.clone());
        }
        if !self.synth {
            return Ok(vec![]);
        }
        let name = match (q.kind, q.season, q.episode) {
            (MediaKind::Series, Some(s), Some(e)) => format!("{} S{s:02}E{e:02}", q.text),
            (MediaKind::Series, Some(s), None) => format!("{} S{s:02}", q.text),
            (MediaKind::Music, ..) => format!(
                "{} - {} [",
                q.artist.clone().unwrap_or_default(),
                q.album.clone().unwrap_or_default()
            ),
            (MediaKind::Book, ..) => format!(
                "{} - {} [",
                q.author.clone().unwrap_or_default(),
                q.book.clone().unwrap_or_default()
            ),
            _ => q.text.clone(),
        };
        let tags: &[&str] = match q.kind {
            MediaKind::Movie | MediaKind::Series => {
                &[" 1080p BluRay x264-FAKE", " 720p HDTV x264-FAKE"]
            }
            MediaKind::Music => &["FLAC]", "MP3 320]"],
            MediaKind::Book => &["EPUB]", "PDF]"],
        };
        Ok(tags
            .iter()
            .map(|t| release(def.id, &format!("{name}{t}"), 700 << 20, 20))
            .collect())
    }

    async fn rss(&self, def: &IndexerDef, _: Option<&str>, _: MediaKind) -> Result<Vec<Release>> {
        Ok(self
            .releases
            .lock()
            .unwrap()
            .get(&def.id)
            .cloned()
            .unwrap_or_default())
    }

    async fn fetch(&self, _: &IndexerDef, _: Option<&str>, url: &str) -> Result<TorrentSource> {
        Ok(TorrentSource::Bytes(url.as_bytes().to_vec()))
    }

    fn parse(&self, name: &str) -> ParsedRelease {
        parse(name)
    }
}

/// Torrents in memory. Tests finish them with [`FakeDownloader::finish`]; with `auto`, each `list` call advances them by a fifth and a finished one gets a small placeholder file.
#[derive(Default)]
pub struct FakeDownloader {
    pub torrents: Mutex<Vec<TorrentStatus>>,
    pub auto: bool,
    pub limits: Mutex<Limits>,
}

fn magnet_param(m: &str, k: &str) -> Option<String> {
    m.split(['?', '&']).find_map(|kv| {
        kv.strip_prefix(k)?
            .strip_prefix('=')
            .map(|v| v.replace('+', " "))
    })
}

impl FakeDownloader {
    /// Write `files` under the torrent's folder and mark it complete (seeding).
    pub fn finish(&self, id: &str, files: &[(&str, &[u8])]) {
        let mut ts = self.torrents.lock().unwrap();
        let t = ts.iter_mut().find(|t| t.id == id).expect("known torrent");
        t.files.clear();
        for (name, data) in files {
            let rel = PathBuf::from(&t.name).join(name);
            let abs = t.save_path.join(&rel);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(&abs, data).unwrap();
            t.files.push(TorrentFile {
                path: rel,
                size: data.len() as u64,
                done: data.len() as u64,
            });
        }
        t.size = t.files.iter().map(|f| f.size).sum::<u64>().max(1);
        t.done = t.size;
        t.state = TorrentState::Seeding;
    }

    pub fn fail(&self, id: &str, why: &str) {
        let mut ts = self.torrents.lock().unwrap();
        let t = ts.iter_mut().find(|t| t.id == id).expect("known torrent");
        t.state = TorrentState::Error;
        t.error = Some(why.into());
    }

    pub fn ids(&self) -> Vec<String> {
        self.torrents
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.id.clone())
            .collect()
    }
}

impl Downloader for FakeDownloader {
    async fn add(&self, src: TorrentSource, opts: AddOptions) -> Result<String> {
        let (id, name, size) = match &src {
            TorrentSource::Magnet(m) => (
                magnet_param(m, "xt")
                    .and_then(|x| x.rsplit(':').next().map(String::from))
                    .ok_or(Error::Invalid("no info hash".into()))?,
                magnet_param(m, "dn").unwrap_or_else(|| "torrent".into()),
                magnet_param(m, "xl")
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(1 << 20),
            ),
            TorrentSource::Bytes(b) => {
                let s = String::from_utf8_lossy(b).to_string();
                (
                    format!("{:040x}", hash(&s)),
                    crate::naming::sanitize(&s),
                    1 << 20,
                )
            }
        };
        let mut ts = self.torrents.lock().unwrap();
        if !ts.iter().any(|t| t.id == id) {
            ts.push(TorrentStatus {
                id: id.clone(),
                name: crate::naming::sanitize(&name),
                state: if opts.paused {
                    TorrentState::Paused
                } else {
                    TorrentState::Downloading
                },
                error: None,
                category: opts.category,
                save_path: opts.save_path,
                size,
                done: 0,
                uploaded: 0,
                down_rate: 0,
                up_rate: 0,
                peers: 1,
                eta_secs: None,
                files: vec![],
                added: crate::state::now(),
            });
        }
        Ok(id)
    }

    async fn pause(&self, id: &str) -> Result<()> {
        self.set_state(id, TorrentState::Paused)
    }

    async fn resume(&self, id: &str) -> Result<()> {
        self.set_state(id, TorrentState::Downloading)
    }

    async fn remove(&self, id: &str, delete_files: bool) -> Result<()> {
        let mut ts = self.torrents.lock().unwrap();
        if let Some(i) = ts.iter().position(|t| t.id == id) {
            let t = ts.remove(i);
            if delete_files && !t.name.is_empty() {
                let _ = std::fs::remove_dir_all(t.save_path.join(&t.name));
            }
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<TorrentStatus>> {
        let mut ts = self.torrents.lock().unwrap();
        if self.auto {
            for t in ts.iter_mut() {
                match t.state {
                    TorrentState::Downloading => {
                        t.done = (t.done + t.size / 5 + 1).min(t.size);
                        if t.done == t.size {
                            let ext = match t.category.as_str() {
                                "nocturne-music" => "flac",
                                "nocturne-book" => "epub",
                                _ => "mkv",
                            };
                            let rel = PathBuf::from(&t.name).join(format!("{}.{ext}", t.name));
                            let abs = t.save_path.join(&rel);
                            if let Some(d) = abs.parent() {
                                let _ = std::fs::create_dir_all(d);
                            }
                            let _ = std::fs::write(&abs, b"NOCTURNE demo file");
                            t.files = vec![TorrentFile {
                                path: rel,
                                size: 18,
                                done: 18,
                            }];
                            t.state = TorrentState::Seeding;
                        }
                    }
                    TorrentState::Seeding => t.uploaded += t.size / 4 + 1,
                    _ => {}
                }
            }
        }
        Ok(ts.clone())
    }

    async fn set_limits(&self, limits: Limits) -> Result<()> {
        *self.limits.lock().unwrap() = limits;
        Ok(())
    }
}

impl FakeDownloader {
    fn set_state(&self, id: &str, s: TorrentState) -> Result<()> {
        let mut ts = self.torrents.lock().unwrap();
        let t = ts.iter_mut().find(|t| t.id == id).ok_or(Error::NotFound)?;
        t.state = s;
        Ok(())
    }
}

/// Units by title name; unknown titles get two of each kind (demo mode).
#[derive(Default)]
pub struct FakeMeta {
    pub units: Mutex<HashMap<String, Vec<Unit>>>,
}

pub fn unit(name: &str, season: Option<u32>, episode: Option<u32>, date: &str) -> Unit {
    Unit {
        id: 0,
        title: 0,
        name: name.into(),
        season,
        episode,
        year: date.get(..4).and_then(|y| y.parse().ok()),
        air_date: Some(date.into()),
        ids: Ids::default(),
        monitored: true,
        file: None,
    }
}

impl Metadata for FakeMeta {
    fn units<'a>(
        &'a self,
        t: &'a Title,
    ) -> BoxFut<'a, std::result::Result<(Ids, Vec<Unit>), String>> {
        Box::pin(async move {
            if let Some(u) = self.units.lock().unwrap().get(&t.name) {
                return Ok((t.ids.clone(), u.clone()));
            }
            let u = match t.kind {
                MediaKind::Movie => vec![],
                MediaKind::Series => (1..=2)
                    .flat_map(|s| {
                        (1..=3).map(move |e| {
                            unit(&format!("Episode {e}"), Some(s), Some(e), "2020-01-01")
                        })
                    })
                    .collect(),
                MediaKind::Music => vec![
                    unit("First Album", None, None, "2001-01-01"),
                    unit("Second Album", None, None, "2005-01-01"),
                ],
                MediaKind::Book => vec![
                    unit("First Book", None, None, "1990-01-01"),
                    unit("Second Book", None, None, "1995-01-01"),
                ],
            };
            Ok((t.ids.clone(), u))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_reads_the_usual_shapes() {
        let p = parse("Night.Train.2020.1080p.BluRay.x264-GRP");
        assert_eq!(
            (p.title.as_str(), p.year, p.quality, p.group.as_deref()),
            ("Night Train", Some(2020), Quality::Bluray1080, Some("GRP"))
        );
        let p = parse("Show.S01E02E03.720p.WEB-DL.PROPER");
        assert_eq!(
            (
                p.title.as_str(),
                p.season,
                p.episodes.clone(),
                p.full_season,
                p.quality,
                p.proper
            ),
            ("Show", Some(1), vec![2, 3], false, Quality::WebDl720, true)
        );
        let p = parse("Show S02 1080p WEBRip");
        assert!(p.full_season && p.season == Some(2));
        let p = parse("Simon & Garfunkel - Bookends (1968) [FLAC]");
        assert_eq!(
            (p.title.as_str(), p.work.as_deref(), p.quality),
            ("Simon & Garfunkel", Some("Bookends"), Quality::Flac)
        );
    }
}
