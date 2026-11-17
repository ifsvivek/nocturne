//! Offline copies: download an item's original file from a server into a local library folder.
//! GET only (`Items/{id}/Download`), resumable (`.part` + Range, then a no-overwrite publish), named the
//! Jellyfin way and accompanied by a small `.nfo` with the provider ids, so the local scanner matches
//! the copy to the server item and the merged library shows it as a second source. Only new files are
//! created, in the folder the user chose; nothing existing is touched. Pure queue logic lives here,
//! the transfer in `fetch`, names in `naming`.

pub mod fetch;
pub mod naming;

use crate::backend::Backend;
use crate::source::{Kind, Media, ProviderIds, SourceId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What a copy needs to know about its item to be named and described.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub title: String,
    pub year: Option<u32>,
    /// Episodes: the show's name, season and episode number.
    pub show: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub ids: ProviderIds,
    /// The show's ids (episodes match through these).
    pub show_ids: ProviderIds,
}

impl Meta {
    /// `None` for anything that is not a movie or an episode.
    pub fn of(m: &Media, show_ids: ProviderIds) -> Option<Self> {
        match m.kind {
            Kind::Movie => Some(Self {
                title: m.title.clone(),
                year: m.year,
                ids: m.ids.clone(),
                ..Default::default()
            }),
            Kind::Episode => Some(Self {
                title: m.title.clone(),
                year: m.year,
                show: Some(m.series_name.clone()?),
                season: m.season_number,
                episode: m.number,
                ids: m.ids.clone(),
                show_ids,
            }),
            _ => None,
        }
    }

    /// The list label: `Title (Year)` or `Show S01E02`.
    pub fn label(&self) -> String {
        match (&self.show, self.episode) {
            (Some(s), Some(e)) => format!("{s} S{:02}E{e:02}", self.season.unwrap_or(1)),
            _ => match self.year {
                Some(y) => format!("{} ({y})", self.title),
                None => self.title.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "why", rename_all = "snake_case")]
pub enum State {
    Queued,
    Active,
    Paused,
    Done,
    Failed(Why),
}

/// Why a job stopped, in words the list can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// The user's account may not download (server policy).
    Denied,
    /// The server no longer has the item.
    Missing,
    /// Sign in again.
    Auth,
    Network,
    Disk,
}

impl Why {
    pub fn text(self) -> &'static str {
        match self {
            Self::Denied => {
                "This server doesn't let your account download files. Ask its owner to allow downloads."
            }
            Self::Missing => "The server no longer has this item.",
            Self::Auth => "Sign in to the server again to continue.",
            Self::Network => "The connection dropped. Resume to continue where it stopped.",
            Self::Disk => "Couldn't write to the folder. Check the space and permissions.",
        }
    }

    /// Worth trying again by itself later (a dropped connection), unlike a refusal.
    pub fn retryable(self) -> bool {
        matches!(self, Self::Network)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    /// The server the item is on, and its id there.
    pub origin: SourceId,
    pub item: String,
    pub meta: Meta,
    /// The library folder the copy goes into.
    pub root: PathBuf,
    /// The local source that owns `root` (rescanned after the copy lands).
    pub into: SourceId,
    /// Final path: known once the server has said what kind of file it is.
    pub dest: Option<PathBuf>,
    pub done: u64,
    pub total: u64,
    pub state: State,
}

impl Job {
    pub fn part(&self) -> Option<PathBuf> {
        self.dest.as_deref().map(naming::part_of)
    }
}

/// The persisted queue. One transfer runs at a time (kind to the server and the connection).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Queue {
    next_id: u64,
    pub jobs: Vec<Job>,
}

impl Queue {
    pub fn path(data: &Path) -> PathBuf {
        data.join("downloads.json")
    }

    /// A damaged or missing file is an empty queue. What was running when the app closed is
    /// queued again, so it continues from its `.part`.
    pub fn load(path: &Path) -> Self {
        let mut q: Self = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for j in &mut q.jobs {
            if j.state == State::Active {
                j.state = State::Queued;
            }
        }
        q
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("json.tmp");
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }

    pub fn add(&mut self, mut job: Job) -> u64 {
        self.next_id += 1;
        job.id = self.next_id;
        job.state = State::Queued;
        let id = job.id;
        self.jobs.push(job);
        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn get(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn active(&self) -> Option<&Job> {
        self.jobs.iter().find(|j| j.state == State::Active)
    }

    /// The next job to start: none while one runs, else the first queued.
    pub fn next(&self) -> Option<u64> {
        if self.active().is_some() {
            return None;
        }
        self.jobs
            .iter()
            .find(|j| j.state == State::Queued)
            .map(|j| j.id)
    }

    /// Already asked for (and not finished or failed): the same item is not queued twice.
    pub fn has(&self, origin: SourceId, item: &str) -> bool {
        self.jobs.iter().any(|j| {
            j.origin == origin
                && j.item == item
                && matches!(j.state, State::Queued | State::Active | State::Paused)
        })
    }

    pub fn remove(&mut self, id: u64) -> Option<Job> {
        let i = self.jobs.iter().position(|j| j.id == id)?;
        Some(self.jobs.remove(i))
    }

    /// Forget the finished and failed ones.
    pub fn clear_finished(&mut self) {
        self.jobs
            .retain(|j| !matches!(j.state, State::Done | State::Failed(_)));
    }
}

/// The episodes under a season or series (a movie or episode is just itself), in order.
pub async fn expand(b: &Backend, m: &Media) -> Vec<Media> {
    match m.kind {
        Kind::Movie | Kind::Episode => vec![m.clone()],
        Kind::Season => match m.series_id.as_deref() {
            Some(s) => b.episodes(s, &m.id).await.unwrap_or_default(),
            None => vec![],
        },
        Kind::Series => {
            let mut out = vec![];
            for s in b.seasons(&m.id).await.unwrap_or_default() {
                out.extend(b.episodes(&m.id, &s.id).await.unwrap_or_default());
            }
            out
        }
        _ => vec![],
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn ids_xml(ids: &ProviderIds) -> String {
    [
        ("imdb", &ids.imdb),
        ("tmdb", &ids.tmdb),
        ("tvdb", &ids.tvdb),
    ]
    .iter()
    .filter_map(|(k, v)| {
        Some(format!(
            "  <uniqueid type=\"{k}\">{}</uniqueid>\n",
            esc(v.as_deref()?)
        ))
    })
    .collect()
}

/// Kodi-style `.nfo` for the file: what the local scanner reads for title, year and ids.
pub fn nfo_for(m: &Meta) -> String {
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    match (&m.show, m.episode) {
        (Some(_), Some(e)) => format!(
            "{head}<episodedetails>\n  <title>{}</title>\n  <season>{}</season>\n  <episode>{e}</episode>\n</episodedetails>\n",
            esc(&m.title),
            m.season.unwrap_or(1)
        ),
        _ => format!(
            "{head}<movie>\n  <title>{}</title>\n{}{}</movie>\n",
            esc(&m.title),
            m.year
                .map(|y| format!("  <year>{y}</year>\n"))
                .unwrap_or_default(),
            ids_xml(&m.ids)
        ),
    }
}

/// `tvshow.nfo`: the show's ids, which the local episodes match the server's through.
pub fn show_nfo_for(m: &Meta) -> Option<String> {
    let show = m.show.as_deref()?;
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<tvshow>\n  <title>{}</title>\n{}</tvshow>\n",
        esc(show),
        ids_xml(&m.show_ids)
    ))
}

/// Write `text` as a new file; an existing one is left alone (false).
pub fn write_new(path: &Path, text: &str) -> bool {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(item: &str) -> Job {
        Job {
            id: 0,
            origin: SourceId(2),
            item: item.into(),
            meta: Meta {
                title: "Heat".into(),
                year: Some(1995),
                ..Default::default()
            },
            root: "/tmp/x".into(),
            into: SourceId(1),
            dest: None,
            done: 0,
            total: 0,
            state: State::Queued,
        }
    }

    #[test]
    fn one_runs_at_a_time_in_order_and_duplicates_are_refused() {
        let mut q = Queue::default();
        let (a, b) = (q.add(job("a")), q.add(job("b")));
        assert_eq!(q.next(), Some(a));
        q.get_mut(a).unwrap().state = State::Active;
        assert_eq!(q.next(), None, "a second transfer waits");
        assert!(q.has(SourceId(2), "a") && !q.has(SourceId(3), "a"));
        q.get_mut(a).unwrap().state = State::Done;
        assert_eq!(q.next(), Some(b));
        assert!(
            !q.has(SourceId(2), "a"),
            "a finished one can be fetched again"
        );
    }

    #[test]
    fn the_queue_survives_a_restart_and_resumes_the_running_job() {
        let t = tempfile::tempdir().unwrap();
        let p = Queue::path(t.path());
        let mut q = Queue::default();
        let a = q.add(job("a"));
        q.add(job("b"));
        q.get_mut(a).unwrap().state = State::Active;
        q.get_mut(a).unwrap().done = 5;
        q.jobs[1].state = State::Failed(Why::Denied);
        q.save(&p).unwrap();
        let q = Queue::load(&p);
        assert_eq!(q.jobs[0].state, State::Queued, "interrupted: queued again");
        assert_eq!(q.jobs[0].done, 5);
        assert_eq!(q.jobs[1].state, State::Failed(Why::Denied));
        assert_eq!(Queue::load(&t.path().join("none.json")).jobs.len(), 0);
        std::fs::write(&p, "garbage").unwrap();
        assert_eq!(Queue::load(&p).jobs.len(), 0);
    }

    #[test]
    fn sidecars_carry_the_ids_and_never_replace_a_file() {
        let m = Meta {
            title: "Tom & Jerry".into(),
            year: Some(1992),
            ids: ProviderIds {
                imdb: Some("tt0104940".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let x = nfo_for(&m);
        assert!(x.contains("Tom &amp; Jerry") && x.contains("type=\"imdb\">tt0104940"));
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.nfo");
        assert!(write_new(&p, "one") && !write_new(&p, "two"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one");
        // The local scanner reads back what was written.
        let back = nocturne_local::nfo::parse(&x);
        assert_eq!(back.ids.imdb.as_deref(), Some("tt0104940"));
        assert_eq!(back.year, Some(1992));
    }

    #[test]
    fn a_show_nfo_holds_the_show_ids() {
        let m = Meta {
            title: "Pilot".into(),
            show: Some("Show".into()),
            episode: Some(1),
            show_ids: ProviderIds {
                tvdb: Some("81189".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(show_nfo_for(&m).unwrap().contains("type=\"tvdb\">81189"));
        assert!(show_nfo_for(&Meta::default()).is_none());
    }
}
