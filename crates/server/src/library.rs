//! The library manager's engine (docs/dev/SERVER_PLAN.md section 4): libraries, profiles, indexers, monitored titles and their units, search and grab, the download queue, import, failed-download handling and the scheduled jobs. The decisions are in [`crate::decide`], placing files in [`crate::import`]; this file does the I/O around them.
use crate::db::Res;
use crate::decide::{self, Verdict};
use crate::dl::{DlSlot, DynDownloader, DynIndexer};
use crate::import;
use crate::meta::Metadata;
use crate::state::{Shared, now};
use nocturne_arr_types::{
    AddOptions, Caps, IndexerDef, Link, MediaKind, Release, TorrentSource, TorrentStatus,
};
use nocturne_core::calendar::Day;
use nocturne_core::server::{
    BlockEntry, Candidate, Event, FileRecord, Grab, GrabState, HistoryEntry, HistoryEvent, Job,
    JobEvent, JobInfo, Library, Monitor, NewTitle, Profile, QueueItem, Title, TitleDetail, Unit,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub fn today() -> String {
    Day((now() / 86_400) as i64).iso()
}

fn kind_dir(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Movie => "movies",
        MediaKind::Series => "series",
        MediaKind::Music => "music",
        MediaKind::Book => "books",
    }
}

fn kind_tag(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Movie => "movie",
        MediaKind::Series => "series",
        MediaKind::Music => "music",
        MediaKind::Book => "book",
    }
}

fn indexer_key(id: u64) -> String {
    format!("indexer:{id}")
}

fn job_key(j: Job) -> String {
    format!(
        "job:{}",
        serde_json::to_value(j)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default()
    )
}

pub struct Engine {
    pub ix: Arc<dyn DynIndexer>,
    pub meta: Arc<dyn Metadata>,
    /// One search or RSS grab at a time, so two of them cannot grab the same unit.
    // ponytail: one global search lock; per-title locks if searches queue up noticeably.
    search: tokio::sync::Mutex<()>,
    /// Queue changes (grab, poll, remove) one at a time.
    queue: tokio::sync::Mutex<()>,
    running: Mutex<HashSet<Job>>,
    /// Wakes the poll loop when something was grabbed.
    pub wake: tokio::sync::Notify,
}

impl Engine {
    pub fn new(ix: Arc<dyn DynIndexer>, meta: Arc<dyn Metadata>) -> Arc<Self> {
        Arc::new(Self {
            ix,
            meta,
            search: tokio::sync::Mutex::new(()),
            queue: tokio::sync::Mutex::new(()),
            running: Mutex::default(),
            wake: tokio::sync::Notify::new(),
        })
    }

    fn dl(sh: &Shared) -> Res<Arc<dyn DynDownloader>> {
        sh.ext::<DlSlot>().get()
    }

    // ------------------------------------------------------------ settings records

    pub fn save_library(sh: &Shared, mut l: Library) -> Res<u64> {
        if !l.root.is_absolute() {
            return Err("the library folder must be an absolute path".into());
        }
        let (f, n) = Library::default_templates(l.kind);
        if l.folder_template.trim().is_empty() {
            l.folder_template = f.into();
        }
        if l.file_template.trim().is_empty() {
            l.file_template = n.into();
        }
        if l.id != 0 {
            let old = sh.db.get::<Library>(l.id)?.ok_or("no such library")?;
            if old.kind != l.kind && sh.db.all::<Title>()?.iter().any(|t| t.library == l.id) {
                return Err("titles use this library; its kind cannot change".into());
            }
        }
        match l.mode {
            nocturne_core::server::LibraryMode::Managed => std::fs::create_dir_all(&l.root)
                .map_err(|e| format!("{}: {e}", l.root.display()))?,
            nocturne_core::server::LibraryMode::Existing if !l.root.is_dir() => {
                return Err(format!("{} is not an existing folder", l.root.display()));
            }
            _ => {}
        }
        sh.db.put(&mut l)
    }

    pub fn delete_library(sh: &Shared, id: u64) -> Res<()> {
        if sh.db.all::<Title>()?.iter().any(|t| t.library == id) {
            return Err("in use: titles are stored in this library".into());
        }
        sh.db.delete::<Library>(&[id])
    }

    pub fn save_profile(sh: &Shared, mut p: Profile) -> Res<u64> {
        if p.allowed.is_empty() {
            return Err("allow at least one quality".into());
        }
        let fits = nocturne_arr_types::Quality::for_kind(p.kind);
        if let Some(q) = p.allowed.iter().find(|q| !fits.contains(q)) {
            return Err(format!("{} does not fit this kind of media", q.label()));
        }
        if !p.allowed.contains(&p.cutoff) {
            return Err("the cutoff must be one of the allowed qualities".into());
        }
        sh.db.put(&mut p)
    }

    pub fn delete_profile(sh: &Shared, id: u64) -> Res<()> {
        if sh.db.all::<Title>()?.iter().any(|t| t.profile == id) {
            return Err("in use: titles use this profile".into());
        }
        sh.db.delete::<Profile>(&[id])
    }

    /// `key`: `None` keeps the stored one, `Some("")` deletes it.
    pub fn save_indexer(sh: &Shared, mut def: IndexerDef, key: Option<&str>) -> Res<u64> {
        let u = def.url.trim();
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err("the indexer address must start with http:// or https://".into());
        }
        if def.name.trim().is_empty() {
            return Err("give the indexer a name".into());
        }
        let id = sh.db.put(&mut def)?;
        match key {
            Some("") => sh.secrets.set(&indexer_key(id), None)?,
            Some(k) => sh.secrets.set(&indexer_key(id), Some(k))?,
            None => {}
        }
        Ok(id)
    }

    pub fn delete_indexer(sh: &Shared, id: u64) -> Res<()> {
        sh.db.delete::<IndexerDef>(&[id])?;
        let _ = sh.secrets.set(&indexer_key(id), None);
        Ok(())
    }

    pub async fn test_indexer(&self, sh: &Shared, id: u64) -> Res<Caps> {
        let def = sh.db.get::<IndexerDef>(id)?.ok_or("no such indexer")?;
        let key = sh.secrets.get(&indexer_key(id));
        self.ix
            .caps(&def, key.as_deref())
            .await
            .map_err(|e| e.to_string())
    }

    // ------------------------------------------------------------ titles

    pub fn units_of(sh: &Shared, title: u64) -> Res<Vec<Unit>> {
        // ponytail: full scan of the units table per title; an index by title when catalogs get large.
        Ok(sh
            .db
            .all::<Unit>()?
            .into_iter()
            .filter(|u| u.title == title)
            .collect())
    }

    pub fn detail(sh: &Shared, id: u64) -> Res<Option<TitleDetail>> {
        let Some(title) = sh.db.get::<Title>(id)? else {
            return Ok(None);
        };
        Ok(Some(TitleDetail {
            units: Self::units_of(sh, id)?,
            title,
        }))
    }

    async fn fetch_units(&self, t: &Title) -> Res<(nocturne_arr_types::Ids, Vec<Unit>)> {
        if t.kind == MediaKind::Movie {
            return Ok((
                t.ids.clone(),
                vec![Unit {
                    id: 0,
                    title: t.id,
                    name: t.name.clone(),
                    season: None,
                    episode: None,
                    year: t.year,
                    air_date: None,
                    ids: t.ids.clone(),
                    monitored: true,
                    file: None,
                }],
            ));
        }
        self.meta.units(t).await
    }

    pub async fn add_title(self: &Arc<Self>, sh: &Shared, n: NewTitle) -> Res<u64> {
        let lib = sh.db.get::<Library>(n.library)?.ok_or("no such library")?;
        let prof = sh.db.get::<Profile>(n.profile)?.ok_or("no such profile")?;
        if lib.kind != n.kind || prof.kind != n.kind {
            return Err("the library and the profile must be for the same kind of media".into());
        }
        let dup = sh.db.all::<Title>()?.into_iter().find(|t| {
            t.kind == n.kind
                && ((n.ids.imdb.is_some() && t.ids.imdb == n.ids.imdb)
                    || (n.ids.tmdb.is_some() && t.ids.tmdb == n.ids.tmdb)
                    || (decide::norm(&t.name) == decide::norm(&n.name) && t.year == n.year))
        });
        if let Some(d) = dup {
            return Err(format!("already monitored (title {})", d.id));
        }
        let mut t = Title {
            id: 0,
            kind: n.kind,
            name: n.name.trim().into(),
            year: n.year,
            ids: n.ids,
            profile: n.profile,
            library: n.library,
            monitored: n.monitor != Monitor::None,
            added: now(),
        };
        let (ids, mut units) = self.fetch_units(&t).await?;
        t.ids = ids;
        let id = sh.db.put(&mut t)?;
        let today = today();
        for u in &mut units {
            u.id = 0;
            u.title = id;
            u.monitored = match &n.monitor {
                Monitor::All => true,
                Monitor::None => false,
                Monitor::Future => u.air_date.as_deref().is_none_or(|d| d > today.as_str()),
                Monitor::Seasons(s) => u.season.is_some_and(|x| s.contains(&x)),
            };
        }
        sh.db.put_all(&mut units)?;
        sh.publish(Event::Title(id));
        Self::count(sh);
        if n.search {
            let (me, sh) = (self.clone(), sh.clone());
            tokio::spawn(async move {
                let _ = me.search(&sh, id, vec![]).await;
            });
        }
        Ok(id)
    }

    /// New units from the provider are added (monitored like the title), known ones updated; none is deleted.
    pub async fn refresh_title(&self, sh: &Shared, id: u64) -> Res<()> {
        let mut t = sh.db.get::<Title>(id)?.ok_or("no such title")?;
        if t.kind == MediaKind::Movie {
            return Ok(());
        }
        let (ids, fresh) = self.meta.units(&t).await?;
        t.ids = ids;
        sh.db.put(&mut t)?;
        let mut have = Self::units_of(sh, id)?;
        let key = |u: &Unit| {
            (
                u.season,
                u.episode,
                if u.episode.is_some() {
                    String::new()
                } else {
                    decide::norm(&u.name)
                },
            )
        };
        let mut add = vec![];
        for f in fresh {
            match have.iter_mut().find(|h| key(h) == key(&f)) {
                Some(h) => {
                    h.name = f.name;
                    h.air_date = f.air_date;
                    h.year = f.year;
                    h.ids = f.ids;
                }
                None => add.push(Unit {
                    title: id,
                    monitored: t.monitored,
                    ..f
                }),
            }
        }
        sh.db.put_all(&mut have)?;
        sh.db.put_all(&mut add)?;
        sh.publish(Event::Title(id));
        Self::count(sh);
        Ok(())
    }

    /// Forget a title and its units; its downloads go too. Imported files stay where they are.
    pub async fn remove_title(&self, sh: &Shared, id: u64) -> Res<()> {
        let _q = self.queue.lock().await;
        for g in sh
            .db
            .all::<Grab>()?
            .into_iter()
            .filter(|g| g.title == id && active(g))
        {
            if let Ok(dl) = Self::dl(sh) {
                let _ = dl.remove(&g.torrent, true).await;
            }
            sh.db.delete::<Grab>(&[g.id])?;
        }
        let units: Vec<u64> = Self::units_of(sh, id)?.iter().map(|u| u.id).collect();
        sh.db.delete::<Unit>(&units)?;
        sh.db.delete::<Title>(&[id])?;
        sh.publish(Event::Title(id));
        Self::count(sh);
        Ok(())
    }

    pub fn monitor(sh: &Shared, title: u64, units: &[u64], on: bool) -> Res<()> {
        if units.is_empty() {
            let mut t = sh.db.get::<Title>(title)?.ok_or("no such title")?;
            t.monitored = on;
            sh.db.put(&mut t)?;
        } else {
            let mut us: Vec<Unit> = Self::units_of(sh, title)?
                .into_iter()
                .filter(|u| units.contains(&u.id))
                .collect();
            for u in &mut us {
                u.monitored = on;
            }
            sh.db.put_all(&mut us)?;
        }
        sh.publish(Event::Title(title));
        Ok(())
    }

    // ------------------------------------------------------------ search and grab

    /// Title, profile, all units, and the wanted ones among `units` (all wanted when empty).
    fn targets(
        sh: &Shared,
        title: u64,
        units: &[u64],
    ) -> Res<(Title, Profile, Vec<Unit>, Vec<u64>)> {
        let t = sh.db.get::<Title>(title)?.ok_or("no such title")?;
        let p = sh
            .db
            .get::<Profile>(t.profile)?
            .ok_or("the title's profile is gone")?;
        let all = Self::units_of(sh, title)?;
        let today = today();
        let wanted = all
            .iter()
            .filter(|u| units.is_empty() || units.contains(&u.id))
            .filter(|u| decide::wants(u, &p, &today))
            .map(|u| u.id)
            .collect();
        Ok((t, p, all, wanted))
    }

    async fn find(&self, sh: &Shared, t: &Title, all: &[Unit], wanted: &[u64]) -> Vec<Release> {
        let targets: Vec<&Unit> = all.iter().filter(|u| wanted.contains(&u.id)).collect();
        let queries = decide::queries(t, &targets);
        let mut defs: Vec<IndexerDef> = sh
            .db
            .all::<IndexerDef>()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.enabled && d.kinds.contains(&t.kind))
            .collect();
        defs.sort_by_key(|d| d.priority);
        let mut out: Vec<Release> = vec![];
        for d in &defs {
            let key = sh.secrets.get(&indexer_key(d.id));
            for q in &queries {
                match self.ix.search(d, key.as_deref(), q).await {
                    Ok(rs) => {
                        for r in rs {
                            if !out
                                .iter()
                                .any(|o| o.indexer_id == r.indexer_id && o.guid == r.guid)
                            {
                                out.push(r);
                            }
                        }
                    }
                    Err(e) => eprintln!("nocturne-server: indexer {}: {e}", d.name),
                }
            }
        }
        out
    }

    fn evaluate(
        sh: &Shared,
        t: &Title,
        p: &Profile,
        all: &[Unit],
        wanted: &[u64],
        rels: Vec<Release>,
    ) -> Res<Vec<Candidate>> {
        let block = sh.db.all::<BlockEntry>()?;
        let grabs = sh.db.all::<Grab>()?;
        Ok(decide::evaluate(
            t,
            p,
            all,
            wanted,
            rels,
            &block,
            &grabs,
            &today(),
        ))
    }

    /// Interactive search.
    pub async fn releases(&self, sh: &Shared, title: u64, units: &[u64]) -> Res<Vec<Candidate>> {
        let (t, p, all, wanted) = Self::targets(sh, title, units)?;
        // Interactive search also looks for what is not wanted (the user may want it anyway).
        let asked: Vec<u64> = if units.is_empty() {
            all.iter().map(|u| u.id).collect()
        } else {
            units.to_vec()
        };
        let rels = self
            .find(
                sh,
                &t,
                &all,
                if wanted.is_empty() { &asked } else { &wanted },
            )
            .await;
        let mut c = Self::evaluate(sh, &t, &p, &all, &asked, rels)?;
        c.sort_by_key(|c| {
            (
                !c.rejections.is_empty(),
                std::cmp::Reverse(decide::score(&c.release, &p)),
            )
        });
        Ok(c)
    }

    /// Automatic search: grab the best acceptable releases. Returns how many were grabbed.
    pub async fn search(&self, sh: &Shared, title: u64, units: Vec<u64>) -> Res<usize> {
        let _one = self.search.lock().await;
        let (t, p, all, wanted) = Self::targets(sh, title, &units)?;
        if wanted.is_empty() {
            return Ok(0);
        }
        let rels = self.find(sh, &t, &all, &wanted).await;
        self.grab_best(sh, &t, &p, &all, &wanted, rels).await
    }

    async fn grab_best(
        &self,
        sh: &Shared,
        t: &Title,
        p: &Profile,
        all: &[Unit],
        wanted: &[u64],
        rels: Vec<Release>,
    ) -> Res<usize> {
        let ok: Vec<(Release, Vec<u64>)> = Self::evaluate(sh, t, p, all, wanted, rels)?
            .into_iter()
            .filter(|c| c.rejections.is_empty())
            .map(|c| (c.release, c.units))
            .collect();
        let mut n = 0;
        for (rel, units) in decide::choose(ok, p) {
            match self.grab(sh, t, rel, units).await {
                Ok(_) => n += 1,
                Err(e) => eprintln!("nocturne-server: grab for {}: {e}", t.name),
            }
        }
        Ok(n)
    }

    /// The user's pick from the interactive search: only the protocol and the blocklist still refuse it.
    pub async fn grab_manual(&self, sh: &Shared, title: u64, rel: Release) -> Res<u64> {
        let t = sh.db.get::<Title>(title)?.ok_or("no such title")?;
        let all = Self::units_of(sh, title)?;
        let units =
            decide::units_for(&rel, &t, &all).ok_or("this release does not match the title")?;
        if rel.protocol == nocturne_arr_types::Protocol::Usenet {
            return Err(
                "Usenet releases need a Usenet download client, which NOCTURNE does not have yet"
                    .into(),
            );
        }
        if decide::blocked(&rel, t.id, &sh.db.all::<BlockEntry>()?) {
            return Err("this release is on the blocklist".into());
        }
        self.grab(sh, &t, rel, units).await
    }

    async fn grab(&self, sh: &Shared, t: &Title, rel: Release, units: Vec<u64>) -> Res<u64> {
        let dl = Self::dl(sh)?;
        let src = match &rel.link {
            Link::Magnet(m) => TorrentSource::Magnet(m.clone()),
            Link::Url(u) => {
                let def = sh
                    .db
                    .get::<IndexerDef>(rel.indexer_id)?
                    .ok_or("the indexer is gone")?;
                let key = sh.secrets.get(&indexer_key(def.id));
                self.ix
                    .fetch(&def, key.as_deref(), u)
                    .await
                    .map_err(|e| e.to_string())?
            }
        };
        let dir = sh
            .cfg()
            .downloads
            .dir_or_default()
            .unwrap_or_else(|| sh.paths.data.join("downloads"));
        let _q = self.queue.lock().await;
        let torrent = dl
            .add(
                src,
                AddOptions {
                    save_path: dir.join(kind_dir(t.kind)),
                    category: format!("nocturne-{}", kind_tag(t.kind)),
                    paused: false,
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut g = Grab {
            id: 0,
            title: t.id,
            units,
            release: rel,
            torrent,
            state: GrabState::Downloading,
            added: now(),
            done: 0,
            progress_at: now(),
            seeding_since: None,
            message: None,
        };
        let id = sh.db.put(&mut g)?;
        history(sh, &g, HistoryEvent::Grabbed, "")?;
        self.wake.notify_one();
        Ok(id)
    }

    // ------------------------------------------------------------ queue

    pub async fn queue(sh: &Shared, list: Option<&[TorrentStatus]>) -> Res<Vec<QueueItem>> {
        let titles = sh.db.all::<Title>()?;
        Ok(sh
            .db
            .all::<Grab>()?
            .into_iter()
            .filter(|g| {
                matches!(
                    g.state,
                    GrabState::Downloading | GrabState::Seeding | GrabState::ImportBlocked
                )
            })
            .map(|g| QueueItem {
                title_name: titles
                    .iter()
                    .find(|t| t.id == g.title)
                    .map(|t| t.name.clone())
                    .unwrap_or_default(),
                status: list.and_then(|l| l.iter().find(|s| s.id == g.torrent).cloned()),
                grab: g,
            })
            .collect())
    }

    pub async fn queue_live(sh: &Shared) -> Res<Vec<QueueItem>> {
        let list = match Self::dl(sh) {
            Ok(dl) => dl.list().await.ok(),
            Err(_) => None,
        };
        Self::queue(sh, list.as_deref()).await
    }

    /// One pass over the active grabs. Returns whether any is still active.
    pub async fn poll(self: &Arc<Self>, sh: &Shared) -> Res<bool> {
        let grabs: Vec<Grab> = sh.db.all::<Grab>()?.into_iter().filter(active).collect();
        if grabs.is_empty() {
            return Ok(false);
        }
        let dl = Self::dl(sh)?;
        let list = dl.list().await.map_err(|e| e.to_string())?;
        let cfg = sh.cfg().downloads;
        let mut research = vec![];
        {
            let _q = self.queue.lock().await;
            for mut g in grabs {
                // Re-read: a route may have removed it meanwhile.
                if sh.db.get::<Grab>(g.id)?.is_none() {
                    continue;
                }
                let st = list.iter().find(|s| s.id == g.torrent);
                let t = now();
                if let Some(s) = st
                    && s.done > g.done
                {
                    g.done = s.done;
                    g.progress_at = t;
                }
                match decide::check(&g, st, t, cfg.seed_ratio, cfg.seed_hours) {
                    Verdict::Wait => {}
                    Verdict::Completed => {
                        let st = st.cloned().ok_or("no status")?;
                        self.import(sh, &mut g, &st)?;
                    }
                    Verdict::Failed(why) => {
                        let _ = dl.remove(&g.torrent, true).await;
                        block(sh, &g, &why)?;
                        g.state = GrabState::Failed;
                        g.message = Some(why.clone());
                        history(sh, &g, HistoryEvent::DownloadFailed, &why)?;
                        research.push((g.title, g.units.clone()));
                    }
                    Verdict::SeedingDone => {
                        let _ = dl.remove(&g.torrent, true).await;
                        g.state = GrabState::Done;
                    }
                }
                sh.db.put(&mut g)?;
            }
        }
        for (title, units) in research {
            let (me, sh) = (self.clone(), sh.clone());
            tokio::spawn(async move {
                let _ = me.search(&sh, title, units).await;
            });
        }
        let q = Self::queue(sh, Some(&list)).await?;
        let (down, up) = list
            .iter()
            .fold((0, 0), |(d, u), s| (d + s.down_rate, u + s.up_rate));
        {
            let c = sh.ext::<crate::api::Counters>();
            let mut c = c.0.lock().unwrap();
            c.2 = q
                .iter()
                .filter(|i| i.grab.state == GrabState::Downloading)
                .count();
            (c.3, c.4) = (down, up);
        }
        let still = !q.is_empty();
        sh.publish(Event::Queue(q));
        Ok(still)
    }

    /// Import a finished download: plan, place, record. Partial imports keep what worked; the rest stays wanted.
    fn import(&self, sh: &Shared, g: &mut Grab, st: &TorrentStatus) -> Res<()> {
        let t = sh.db.get::<Title>(g.title)?.ok_or("the title is gone")?;
        let lib = sh
            .db
            .get::<Library>(t.library)?
            .ok_or("the library is gone")?;
        let mut units: Vec<Unit> = Self::units_of(sh, t.id)?
            .into_iter()
            .filter(|u| g.units.contains(&u.id))
            .collect();
        let files: Vec<_> = st
            .files
            .iter()
            .map(|f| (st.save_path.join(&f.path), f.size))
            .collect();
        let ix = self.ix.clone();
        let plan = import::plan(&t, &lib, &units, &g.release.parsed, &files, &|n| {
            ix.parse(n)
        });
        let mut problems: Vec<String> = plan
            .missing
            .iter()
            .filter_map(|id| units.iter().find(|u| u.id == *id))
            .map(|u| format!("no file found for {}", u.name))
            .collect();
        let (mut imported, mut upgraded) = (0, false);
        for u in &mut units {
            let moves: Vec<_> = plan.moves.iter().filter(|m| m.unit == u.id).collect();
            if moves.is_empty() {
                continue;
            }
            let old: Vec<_> = u.file.as_ref().map(|f| f.paths.clone()).unwrap_or_default();
            let managed: HashSet<_> = old.iter().cloned().collect();
            let mut placed = vec![];
            let mut size = 0;
            for m in &moves {
                match import::place(&lib.root, m, &managed) {
                    Ok(()) => {
                        placed.push(m.dest.clone());
                        size += std::fs::metadata(&m.dest).map_or(0, |x| x.len());
                    }
                    Err(e) => problems.push(e),
                }
            }
            if placed.len() < moves.len() {
                // Keep the old record; the newly placed files are NOCTURNE's too, so remember them for the next try.
                continue;
            }
            import::retire(&lib.root, &old, &placed);
            upgraded |= u.file.is_some();
            u.file = Some(FileRecord {
                paths: placed,
                quality: g.release.parsed.quality,
                revision: g.release.parsed.revision(),
                size,
                release: g.release.title.clone(),
                imported: now(),
            });
            imported += 1;
        }
        sh.db.put_all(&mut units)?;
        let msg = problems.join("; ");
        if imported == 0 {
            g.state = GrabState::ImportBlocked;
            g.message = Some(if msg.is_empty() {
                "nothing to import".into()
            } else {
                msg.clone()
            });
            history(
                sh,
                g,
                HistoryEvent::ImportFailed,
                g.message.as_deref().unwrap_or(""),
            )?;
        } else {
            g.state = GrabState::Seeding;
            g.seeding_since = Some(now());
            g.message = (!msg.is_empty()).then(|| msg.clone());
            history(
                sh,
                g,
                if upgraded {
                    HistoryEvent::Upgraded
                } else {
                    HistoryEvent::Imported
                },
                "",
            )?;
            if !msg.is_empty() {
                history(sh, g, HistoryEvent::ImportFailed, &msg)?;
            }
        }
        sh.publish(Event::Title(t.id));
        Self::count(sh);
        Ok(())
    }

    pub async fn remove_grab(
        self: &Arc<Self>,
        sh: &Shared,
        id: u64,
        blocklist: bool,
        search: bool,
    ) -> Res<()> {
        let g = {
            let _q = self.queue.lock().await;
            let g = sh.db.get::<Grab>(id)?.ok_or("no such grab")?;
            if let Ok(dl) = Self::dl(sh) {
                let _ = dl.remove(&g.torrent, true).await;
            }
            if blocklist {
                block(sh, &g, "removed by the user")?;
            }
            sh.db.delete::<Grab>(&[id])?;
            history(sh, &g, HistoryEvent::Removed, "")?;
            g
        };
        if search {
            let (me, sh) = (self.clone(), sh.clone());
            tokio::spawn(async move {
                let _ = me.search(&sh, g.title, g.units).await;
            });
        }
        Ok(())
    }

    pub async fn retry_import(&self, sh: &Shared, id: u64) -> Res<()> {
        let _q = self.queue.lock().await;
        let mut g = sh.db.get::<Grab>(id)?.ok_or("no such grab")?;
        if g.state != GrabState::ImportBlocked {
            return Err("only a grab waiting for an import fix can be retried".into());
        }
        let list = Self::dl(sh)?.list().await.map_err(|e| e.to_string())?;
        let st = list
            .into_iter()
            .find(|s| s.id == g.torrent)
            .ok_or("the torrent is gone")?;
        self.import(sh, &mut g, &st)?;
        sh.db.put(&mut g)?;
        Ok(())
    }

    pub async fn pause(sh: &Shared, id: u64, on: bool) -> Res<()> {
        let g = sh.db.get::<Grab>(id)?.ok_or("no such grab")?;
        let dl = Self::dl(sh)?;
        if on {
            dl.pause(&g.torrent).await
        } else {
            dl.resume(&g.torrent).await
        }
        .map_err(|e| e.to_string())
    }

    // ------------------------------------------------------------ jobs

    pub fn jobs(&self, sh: &Shared) -> Vec<JobInfo> {
        let running = self.running.lock().unwrap();
        Job::ALL
            .iter()
            .map(|&j| {
                let last = sh.db.meta(&job_key(j)).ok().flatten();
                JobInfo {
                    job: j,
                    last_run: last,
                    next_run: match last {
                        Some(l) => l + j.interval(),
                        None if j.due(None, sh.started, sh.started) => sh.started,
                        None => sh.started + Job::RssSync.interval(),
                    },
                    running: running.contains(&j),
                }
            })
            .collect()
    }

    pub async fn run_job(self: &Arc<Self>, sh: &Shared, j: Job) -> Res<()> {
        if !self.running.lock().unwrap().insert(j) {
            return Ok(());
        }
        sh.publish(Event::Job(JobEvent {
            job: j,
            running: true,
            message: None,
        }));
        let r = match j {
            Job::RssSync => self.rss_sync(sh).await,
            Job::MissingSearch => self.sweep(sh, false).await,
            Job::UpgradeSearch => self.sweep(sh, true).await,
            Job::RefreshMetadata => self.refresh_all(sh).await,
        };
        self.running.lock().unwrap().remove(&j);
        let _ = sh.db.set_meta(&job_key(j), now());
        *sh.ext::<crate::api::JobSlot>().0.lock().unwrap() = self.jobs(sh);
        sh.publish(Event::Job(JobEvent {
            job: j,
            running: false,
            message: r.as_ref().err().cloned(),
        }));
        r
    }

    async fn rss_sync(&self, sh: &Shared) -> Res<()> {
        let _one = self.search.lock().await;
        let defs: Vec<IndexerDef> = sh
            .db
            .all::<IndexerDef>()?
            .into_iter()
            .filter(|d| d.enabled)
            .collect();
        let titles: Vec<Title> = sh
            .db
            .all::<Title>()?
            .into_iter()
            .filter(|t| t.monitored)
            .collect();
        for d in &defs {
            let key = sh.secrets.get(&indexer_key(d.id));
            for &kind in &d.kinds {
                if !titles.iter().any(|t| t.kind == kind) {
                    continue;
                }
                let rels = match self.ix.rss(d, key.as_deref(), kind).await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("nocturne-server: RSS of {}: {e}", d.name);
                        continue;
                    }
                };
                for t in titles.iter().filter(|t| t.kind == kind) {
                    let (t, p, all, wanted) = Self::targets(sh, t.id, &[])?;
                    if wanted.is_empty() {
                        continue;
                    }
                    let mine: Vec<Release> = rels
                        .iter()
                        .filter(|r| decide::units_for(r, &t, &all).is_some())
                        .cloned()
                        .collect();
                    if !mine.is_empty() {
                        self.grab_best(sh, &t, &p, &all, &wanted, mine).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Missing search (units without a file) or upgrade search (units with one, below the cutoff).
    async fn sweep(&self, sh: &Shared, upgrades: bool) -> Res<()> {
        for t in sh.db.all::<Title>()?.into_iter().filter(|t| t.monitored) {
            let units: Vec<u64> = Self::units_of(sh, t.id)?
                .into_iter()
                .filter(|u| u.file.is_some() == upgrades)
                .map(|u| u.id)
                .collect();
            if !units.is_empty() {
                self.search(sh, t.id, units).await?;
            }
        }
        Ok(())
    }

    async fn refresh_all(&self, sh: &Shared) -> Res<()> {
        for t in sh
            .db
            .all::<Title>()?
            .into_iter()
            .filter(|t| t.kind != MediaKind::Movie)
        {
            if let Err(e) = self.refresh_title(sh, t.id).await {
                eprintln!("nocturne-server: refresh {}: {e}", t.name);
            }
        }
        Ok(())
    }

    /// Title and missing counters for `/status`.
    pub fn count(sh: &Shared) {
        let titles = sh.db.all::<Title>().unwrap_or_default();
        let profiles = sh.db.all::<Profile>().unwrap_or_default();
        let today = today();
        let missing = sh
            .db
            .all::<Unit>()
            .unwrap_or_default()
            .iter()
            .filter(|u| u.file.is_none())
            .filter(|u| {
                titles
                    .iter()
                    .find(|t| t.id == u.title)
                    .and_then(|t| profiles.iter().find(|p| p.id == t.profile))
                    .is_some_and(|p| decide::wants(u, p, &today))
            })
            .count();
        let c = sh.ext::<crate::api::Counters>();
        let mut c = c.0.lock().unwrap();
        (c.0, c.1) = (titles.len(), missing);
    }

    /// The background loops: poll the queue every 2 s while something is active (else sleep until a grab wakes it), run each job when due. End when `stop` turns true or the server stops.
    pub fn spawn_loops(self: &Arc<Self>, sh: &Shared, stop: tokio::sync::watch::Receiver<bool>) {
        let (me, sh1, mut stop1) = (self.clone(), sh.clone(), stop.clone());
        tokio::spawn(async move {
            loop {
                let active = me.poll(&sh1).await.unwrap_or(false);
                let pause = async {
                    if active {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await
                    } else {
                        me.wake.notified().await
                    }
                };
                tokio::select! {
                    () = pause => {}
                    _ = stop1.wait_for(|s| *s) => return,
                    () = sh1.stopped() => return,
                }
            }
        });
        let (me, sh2, mut stop2) = (self.clone(), sh.clone(), stop);
        tokio::spawn(async move {
            loop {
                let t = now();
                for j in Job::ALL {
                    let last = sh2.db.meta(&job_key(j)).ok().flatten();
                    if j.due(last, sh2.started, t) {
                        let _ = me.run_job(&sh2, j).await;
                    }
                }
                Self::count(&sh2);
                let next = me
                    .jobs(&sh2)
                    .iter()
                    .map(|i| i.next_run)
                    .min()
                    .unwrap_or(t + 900);
                let wait = next.saturating_sub(now()).clamp(5, 900);
                tokio::select! {
                    () = tokio::time::sleep(std::time::Duration::from_secs(wait)) => {}
                    _ = stop2.wait_for(|s| *s) => return,
                    () = sh2.stopped() => return,
                }
            }
        });
    }
}

fn active(g: &Grab) -> bool {
    matches!(g.state, GrabState::Downloading | GrabState::Seeding)
}

fn history(sh: &Shared, g: &Grab, event: HistoryEvent, msg: &str) -> Res<()> {
    let mut h = HistoryEntry {
        id: 0,
        at: now(),
        title: g.title,
        units: g.units.clone(),
        event,
        release: g.release.title.clone(),
        quality: g.release.parsed.quality,
        indexer: g.release.indexer.clone(),
        message: msg.into(),
    };
    sh.db.put(&mut h)?;
    sh.publish(Event::History(h));
    Ok(())
}

fn block(sh: &Shared, g: &Grab, reason: &str) -> Res<()> {
    let mut b = BlockEntry {
        id: 0,
        at: now(),
        title: g.title,
        release: g.release.title.clone(),
        indexer_id: g.release.indexer_id,
        guid: g.release.guid.clone(),
        info_hash: g.release.info_hash.clone(),
        reason: reason.into(),
    };
    sh.db.put(&mut b)?;
    Ok(())
}
