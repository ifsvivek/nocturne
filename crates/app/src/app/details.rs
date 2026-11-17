//! Detail pages on the root: opening one from any cell, forwarding its messages and keys, and
//! carrying out the [`detail::Outcome`]s it queues (play, browse, writes, queue, links). The page
//! owns its data and images; the root only routes (hook notes: `crate::detail`).
use super::message::Message;
use super::{App, View};
use crate::detail::{self, Mark, Outcome, Write};
use crate::keys::Key;
use crate::task::run;
use crate::widget::source::{mark_text, status_color};
use iced::Task;
use nocturne_core::backend::Backend;
use nocturne_core::catalog::WriteError;
use nocturne_core::merge::Entry;
use nocturne_core::source::{Kind, Media};
use nocturne_core::versions;
use nocturne_core::video::PlayerHooks;

impl App {
    fn marks(&self) -> Vec<Mark> {
        self.catalog
            .slots
            .iter()
            .map(|s| Mark {
                origin: s.id,
                label: mark_text(s),
                color: status_color(&s.status),
            })
            .collect()
    }

    /// The other sources' copies of `m`, when the page in view lists it as a merged entry.
    fn alternates_of(&self, m: &Media) -> Vec<Media> {
        let find = |es: &[Entry]| {
            es.iter()
                .find(|e| e.primary.origin == m.origin && e.primary.id == m.id)
                .map(|e| e.alternates.clone())
        };
        let Some(p) = self.stack.last() else {
            return vec![];
        };
        p.home
            .as_deref()
            .and_then(|h| find(&h.resume_e).or_else(|| find(&h.nextup_e)))
            .or_else(|| p.br.as_deref().and_then(|b| find(&b.list)))
            .unwrap_or_default()
    }

    /// The photos of the grid in view, for the viewer's arrows.
    fn grid_photos(&self) -> Vec<Media> {
        self.stack
            .last()
            .and_then(|p| p.br.as_deref())
            .map(|b| {
                b.list
                    .iter()
                    .map(|e| e.primary.clone())
                    .filter(|m| m.kind == Kind::Photo)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Push the detail page of `m` (a photo opens its viewer straight away).
    pub(super) fn open_detail(&mut self, m: Media) -> Task<Message> {
        if self.catalog.slots.is_empty() {
            return Task::none();
        }
        let (alts, photos) = (self.alternates_of(&m), self.grid_photos());
        let backends = self
            .catalog
            .online()
            .filter_map(|s| Some((s.id, s.backend.clone()?)))
            .collect();
        let photo = m.kind == Kind::Photo;
        let (mut d, t0) = detail::Detail::new(
            m.clone(),
            alts,
            backends,
            self.win,
            self.settings.allow_server_writes && !self.dev,
            self.marks(),
        );
        let mut tasks = vec![t0];
        if photo {
            d.set_siblings(photos);
            tasks.push(d.open_photo());
        }
        d.subs_ok = self.subtitles_on(m.origin);
        let pid = self.push_page(View::Detail, Some(m));
        let Some(p) = self.page_mut(pid) else {
            return Task::none();
        };
        p.det = Some(Box::new(d));
        Task::batch(tasks).map(move |m| Message::Detail(pid, m))
    }

    pub(super) fn detail_msg(&mut self, pid: u64, msg: detail::Msg) -> Task<Message> {
        let Some(d) = self.page_mut(pid).and_then(|p| p.det.as_deref_mut()) else {
            return Task::none();
        };
        let t = d.update(msg).map(move |m| Message::Detail(pid, m));
        Task::batch([t, self.drain_detail(pid)])
    }

    /// Detail page keys. `None`: not the detail page's key (Esc / Backspace go back).
    pub(super) fn detail_key(&mut self, k: &Key) -> Option<Task<Message>> {
        let p = self.stack.last_mut().filter(|p| p.view == View::Detail)?;
        let pid = p.pid;
        let t = p
            .det
            .as_deref_mut()?
            .key(k)?
            .map(move |m| Message::Detail(pid, m));
        Some(Task::batch([t, self.drain_detail(pid)]))
    }

    /// A text field or overlay of the page in view owns the keyboard.
    pub(super) fn detail_capturing(&self) -> bool {
        self.stack
            .last()
            .and_then(|p| p.det.as_deref())
            .is_some_and(|d| d.capturing())
    }

    /// The detail page in view, when its viewer covers the window (drawn above the header).
    pub(super) fn detail_fullscreen(&self) -> Option<&detail::Detail> {
        self.stack
            .last()
            .and_then(|p| p.det.as_deref())
            .filter(|d| d.fullscreen())
    }

    /// Keep the page's copies of window size, master switch and source marks current.
    pub(super) fn sync_detail(&mut self) {
        let (win, writes, marks) = (
            self.win,
            self.settings.allow_server_writes && !self.dev,
            self.marks(),
        );
        if let Some(d) = self.stack.last_mut().and_then(|p| p.det.as_deref_mut()) {
            d.set_win(win);
            d.set_writes(writes);
            d.set_marks(marks);
        }
    }

    pub(super) fn drain_detail(&mut self, pid: u64) -> Task<Message> {
        let outs: Vec<Outcome> = match self.page_mut(pid).and_then(|p| p.det.as_deref_mut()) {
            Some(d) => std::mem::take(&mut d.outcomes),
            None => return Task::none(),
        };
        Task::batch(outs.into_iter().map(|o| self.detail_outcome(pid, o)))
    }

    fn detail_outcome(&mut self, pid: u64, o: Outcome) -> Task<Message> {
        match o {
            Outcome::Open(m) => self.open_any(m),
            Outcome::Play {
                media,
                rest,
                source_id,
            } => self.start_play(media, rest, source_id),
            Outcome::Browse { kind, key, label } => {
                let origin = self
                    .page_mut(pid)
                    .and_then(|p| p.media.as_ref().map(|m| m.origin));
                self.open_browse(super::browse::scope_tag(kind, &key, &label), origin, None)
            }
            Outcome::Toast(s) => {
                self.toast(s);
                Task::none()
            }
            Outcome::Write(w) => self.detail_write(pid, w),
            Outcome::Prefer(m) => {
                let ok = versions::prefer(
                    &mut self.prefs,
                    &self.series_ids,
                    &m,
                    nocturne_core::merge::Preferred::default_path().as_deref(),
                );
                self.toast(if ok {
                    "This source will be shown first for this title."
                } else {
                    "This title can't be matched across sources."
                });
                Task::none()
            }
            Outcome::Queue { tracks, next } => {
                match self.queue.insert(tracks, next) {
                    Ok(m) | Err(m) => self.toast(m),
                }
                Task::none()
            }
            Outcome::FindSubs(m) => {
                Task::done(Message::Online(super::online::Msg::Find(Box::new(m))))
            }
            Outcome::OpenUrl(u) => {
                if u.starts_with("http://") || u.starts_with("https://") {
                    let _ = std::process::Command::new("xdg-open").arg(u).spawn();
                }
                Task::none()
            }
            Outcome::Back => {
                self.back();
                self.page_wants()
            }
        }
    }

    /// Carry out a write the page asked for (it already shows the change) and answer it.
    fn detail_write(&mut self, pid: u64, w: Write) -> Task<Message> {
        let answer = move |w: Write, r| Task::done(Message::DetailWrote(pid, w, r));
        let (m, played) = match &w {
            Write::Played(m, v) => (m.clone(), Some(*v)),
            Write::Favourite(m, _) => (m.clone(), None),
            Write::Playlist(op) => {
                let allow =
                    self.settings.allow_server_writes && !self.dev && self.hooks.can_write();
                let Some(c) = self.client().filter(|_| allow) else {
                    return answer(w, Err(self.writes_off()));
                };
                let op = op.clone();
                return run(detail::perform_playlist(c, op), move |r| {
                    Message::DetailWrote(pid, w.clone(), r)
                });
            }
        };
        let backend = match self
            .catalog
            .write_slot(m.origin, self.settings.allow_server_writes)
        {
            Ok(s) => s.backend.clone(),
            Err(WriteError::WritesOff) => return answer(w, Err(self.writes_off())),
            Err(_) => return answer(w, Err("That source isn't available right now.")),
        };
        if let Some(Backend::Local(l)) = backend {
            let Some(v) = played else {
                return answer(
                    w,
                    Err("Favourites are kept by servers, not for local files."),
                );
            };
            return run(
                async move { l.set_played(&m, v).await.is_ok() },
                move |ok| Message::DetailWrote(pid, w.clone(), if ok { Ok(()) } else { Err("") }),
            );
        }
        let Some(h) = self.hooks_for(m.origin).filter(|h| h.can_write()) else {
            return answer(w, Err(self.writes_off()));
        };
        match &w {
            Write::Played(_, v) => h.set_played(&m.id, *v),
            Write::Favourite(_, v) => {
                h.seed_favorite(&m.id, m.favorite);
                h.set_favorite(&m.id, *v);
            }
            Write::Playlist(_) => {}
        }
        // A server refusing it comes back through `drain_reverts`, which patches the page too.
        answer(w, Ok(()))
    }

    pub(super) fn detail_wrote(
        &mut self,
        pid: u64,
        w: Write,
        r: Result<(), &'static str>,
    ) -> Task<Message> {
        self.detail_msg(pid, detail::Msg::Wrote(w, r))
    }
}
