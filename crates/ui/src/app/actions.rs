//! What can be done to a cell: open, play, hover-focus, mark played, favourite. Writes go to the
//! item's own source only (`Catalog::write_slot` / `local_for`): a server needs the master switch
//! and its own login's hooks, the local library keeps its own state and needs neither. Optimistic
//! patch everywhere the item is held, reverted when the write fails.
use super::message::{At, CellAct, Message};
use super::{App, Load};
use crate::task::run;
use iced::Task;
use nocturne_core::backend::Backend;
use nocturne_core::catalog::WriteError;
use nocturne_core::source::{Kind, Media, SourceId};
use nocturne_core::sync::SyncHooks;
use nocturne_core::video::PlayerHooks;
use std::sync::Arc;

impl App {
    /// The item behind a cell of the page in view, and how many sources have a copy.
    pub(super) fn media_at(&self, at: At) -> Option<(Media, usize)> {
        let p = self.stack.last()?;
        match at {
            At::Hero => p.home.as_deref()?.hero().map(|m| (m.clone(), 1)),
            At::Home(row, col) => {
                let h = p.home.as_deref()?;
                let k = *h.layout().get(row)?;
                h.items(k).get(col).map(|(m, c)| ((*m).clone(), *c))
            }
            At::Grid(i) => p.br.as_deref()?.item(i).map(|(m, c)| (m.clone(), c)),
        }
    }

    pub(super) fn update_cell(&mut self, at: At, act: CellAct) -> Task<Message> {
        if act == CellAct::Hover {
            return match at {
                At::Home(r, c) => self.hover_home(r, c),
                At::Grid(i) => {
                    if let Some(p) = self.stack.last_mut() {
                        p.sel = i;
                    }
                    Task::none()
                }
                At::Hero => Task::none(),
            };
        }
        let Some((m, _)) = self.media_at(at) else {
            return Task::none();
        };
        match act {
            CellAct::Open => self.open_any(m),
            CellAct::Play => self.play_item(m),
            CellAct::Fav => self.toggle_fav(&m),
            CellAct::Played => self.toggle_played(&m),
            CellAct::Hover => Task::none(),
        }
    }

    /// Open whatever an item is: a library or folder as a grid, a genre as a grid of its titles,
    /// anything else as its detail page.
    pub(super) fn open_any(&mut self, m: Media) -> Task<Message> {
        self.close_admin();
        let origin = m.origin;
        match m.kind {
            Kind::Library => self.open_library(m),
            Kind::Folder => self.open_browse(super::browse::scope_folder(&m), Some(origin), None),
            Kind::Genre => self.open_browse(
                super::browse::scope_tag("genre", &m.title, &m.title),
                Some(origin),
                None,
            ),
            Kind::Studio => self.open_browse(
                super::browse::scope_tag("studio", &m.id, &m.title),
                Some(origin),
                None,
            ),
            _ => self.open_detail(m),
        }
    }

    /// Play for a focused thing. A series plays its first unwatched (else first) episode.
    pub(super) fn play_item(&mut self, m: Media) -> Task<Message> {
        match m.kind {
            Kind::Library | Kind::Season => Task::none(),
            Kind::Photo => self.open_detail(m),
            Kind::Book | Kind::Folder | Kind::Person | Kind::Other => {
                self.toast("Not playable in NOCTURNE yet.");
                Task::none()
            }
            Kind::Series => {
                let Some(b) = self.catalog.backend(m.origin).cloned() else {
                    return Task::none();
                };
                run(series_target(b, m.id), |r| match r {
                    Some(e) => Message::Play(e, vec![]),
                    None => Message::Toast("Nothing to play here yet.".into()),
                })
            }
            _ => Task::done(Message::Play(m, vec![])),
        }
    }

    // ---- writes ----

    /// Hooks (played / favourite / progress) of the server `origin`: the active login's, or the one
    /// loaded for that source.
    pub(super) fn hooks_for(&self, origin: SourceId) -> Option<Arc<SyncHooks>> {
        if self.active == Some(origin) {
            Some(self.hooks.clone())
        } else {
            self.hooks_by.get(&origin).cloned()
        }
    }

    pub(super) fn toggle_played(&mut self, m: &Media) -> Task<Message> {
        let v = !m.played;
        let done = if v {
            "Marked as watched."
        } else {
            "Marked as unwatched."
        };
        let set = move |x: &mut Media| {
            x.played = v;
            if v {
                x.resume_s = None;
            }
        };
        let backend = match self
            .catalog
            .write_slot(m.origin, self.settings.allow_server_writes)
        {
            Ok(s) => s.backend.clone(),
            Err(e) => {
                self.toast(self.write_refused(e));
                return Task::none();
            }
        };
        match backend {
            Some(Backend::Local(l)) => {
                self.patch(m.origin, &m.id, set);
                self.toast(done);
                let (m2, origin, id) = (m.clone(), m.origin, m.id.clone());
                run(
                    async move { l.set_played(&m2, v).await.is_ok() },
                    move |ok| Message::LocalWrite(origin, id, v, ok),
                )
            }
            _ => {
                let Some(h) = self.hooks_for(m.origin).filter(|h| h.can_write()) else {
                    self.toast(self.writes_off());
                    return Task::none();
                };
                self.patch(m.origin, &m.id, set);
                h.set_played(&m.id, v);
                self.toast(done);
                Task::none()
            }
        }
    }

    pub(super) fn toggle_fav(&mut self, m: &Media) -> Task<Message> {
        let v = !m.favorite;
        let backend = match self
            .catalog
            .write_slot(m.origin, self.settings.allow_server_writes)
        {
            Ok(s) => s.backend.clone(),
            Err(e) => {
                self.toast(self.write_refused(e));
                return Task::none();
            }
        };
        if matches!(backend, Some(Backend::Local(_))) {
            self.toast("Favourites are kept by servers, not for local files.");
            return Task::none();
        }
        let Some(h) = self.hooks_for(m.origin).filter(|h| h.can_write()) else {
            self.toast(self.writes_off());
            return Task::none();
        };
        h.seed_favorite(&m.id, m.favorite);
        h.set_favorite(&m.id, v);
        self.patch(m.origin, &m.id, move |x| x.favorite = v);
        self.toast(if v {
            "Added to favourites."
        } else {
            "Removed from favourites."
        });
        Task::none()
    }

    fn write_refused(&self, e: WriteError) -> &'static str {
        match e {
            WriteError::WritesOff => self.writes_off(),
            _ => "That source isn't available right now.",
        }
    }

    /// A local write came back: undo the optimistic change if it failed.
    pub(super) fn local_write(&mut self, origin: SourceId, id: String, v: bool, ok: bool) {
        if !ok {
            self.patch(origin, &id, move |m| m.played = !v);
            self.toast("Couldn't save that change, so it was undone.");
        }
    }

    /// Failed server writes come back from the bridge: undo them and say so calmly.
    pub(super) fn drain_reverts(&mut self) {
        let mut all: Vec<(SourceId, Vec<nocturne_core::sync::Revert>)> = vec![];
        if let Some(a) = self.active {
            all.push((a, self.hooks.drain()));
        }
        for (o, h) in &self.hooks_by {
            all.push((*o, h.drain()));
        }
        for (origin, rs) in all {
            for r in rs {
                self.patch(origin, &r.id, |m| {
                    m.played = r.played.unwrap_or(m.played);
                    m.favorite = r.fav.unwrap_or(m.favorite);
                });
                self.toast("Couldn't save that change, so it was undone.");
            }
        }
    }

    /// Apply `f` to every copy of the item the UI holds, so an optimistic change shows everywhere.
    pub(super) fn patch(&mut self, origin: SourceId, id: &str, f: impl Fn(&mut Media)) {
        let hit = |m: &mut Media| {
            if m.origin == origin && m.id == id {
                f(m)
            }
        };
        let entries = |v: &mut [nocturne_core::merge::Entry]| {
            for e in v {
                std::iter::once(&mut e.primary)
                    .chain(e.alternates.iter_mut())
                    .for_each(&hit);
            }
        };
        for p in &mut self.stack {
            p.media.iter_mut().for_each(&hit);
            if let Some(d) = p.det.as_deref_mut() {
                d.patch(id, |m| {
                    if m.origin == origin {
                        f(m)
                    }
                });
            }
            if let Some(h) = p.home.as_deref_mut() {
                h.resume.for_each_mut(&hit);
                h.nextup.for_each_mut(&hit);
                entries(&mut h.resume_e);
                entries(&mut h.nextup_e);
                for r in &mut h.latest {
                    if let Load::Ready(v) = &mut r.items {
                        v.iter_mut().for_each(&hit);
                    }
                }
            }
            if let Some(br) = p.br.as_deref_mut() {
                br.slots.iter_mut().flatten().for_each(&hit);
                if let Some(m) = br.merged.as_mut() {
                    m.for_each_mut(&hit);
                }
                entries(&mut br.list);
            }
        }
    }

    /// Read a server's stored login to build its hooks (played / favourite / progress).
    pub(super) fn load_hooks(&self, id: SourceId) -> Task<Message> {
        use nocturne_core::profile::{Profile, ProfileKind};
        let Some(Profile {
            kind: ProfileKind::Jellyfin { url, user_id, .. },
            ..
        }) = self.sources.get(id).cloned()
        else {
            return Task::none();
        };
        run(
            async move {
                tokio::task::spawn_blocking(move || {
                    nocturne_session::load_account(&url, &user_id)
                        .ok()
                        .flatten()
                        .map(|s| (url, s))
                })
                .await
                .ok()
                .flatten()
            },
            move |r| Message::HooksLoaded(id, r),
        )
    }

    pub(super) fn hooks_loaded(
        &mut self,
        id: SourceId,
        r: Option<(String, nocturne_jellyfin_user::Session)>,
    ) {
        let Some((url, s)) = r else { return };
        self.hooks_by.insert(
            id,
            Arc::new(SyncHooks::new(
                nocturne_jellyfin_user::UserClient::from_session(&url, &s).ok(),
                self.settings.allow_server_writes,
                self.settings.sync_progress,
            )),
        );
    }
}

/// The episode a series plays: its next up, else the server's pick.
async fn series_target(b: Backend, id: String) -> Option<Media> {
    match &b {
        Backend::Jellyfin(s) => s.series_target(&id).await.ok(),
        Backend::Local(_) => b.series_next_up(&id).await.ok()?.into_iter().next(),
    }
}
