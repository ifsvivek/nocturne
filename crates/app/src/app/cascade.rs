//! One watched state across sources (opt-in, `Settings::sync_watched`): after the user marks a title
//! watched, or finishes it, the same state goes to its other copies. Which copies and which writes is
//! `nocturne_core::watched` (pure, tested); here the writes are made, each to the copy's own source
//! (`Catalog::write_slot` rules: servers need the master switch, the local library does not). The
//! writes call the low-level setters, never `toggle_played`, so they cannot cascade again.
use super::App;
use super::message::Message;
use crate::player::{PlayEvent, Reported};
use crate::task::run;
use iced::Task;
use nocturne_core::backend::Backend;
use nocturne_core::source::{Media, SourceId};
use nocturne_core::sync::ticks;
use nocturne_core::video::PlayerHooks;
use nocturne_core::watched::{self as w, Cause, Change, Gate, Via};

#[derive(Clone)]
pub enum Msg {
    /// The other copies of `from` were found.
    Found(Box<Media>, Change, Vec<Media>),
}

impl App {
    /// How a source is written to, `None` when it cannot be right now. (The master switch is checked
    /// by `watched::plan`, so this asks for the slot as if writes were allowed.)
    pub(super) fn via_of(&self, o: SourceId) -> Option<Via> {
        match self.catalog.write_slot(o, true).ok()?.backend.as_ref()? {
            Backend::Local(_) => Some(Via::Local),
            Backend::Jellyfin(_) => Some(Via::Server),
        }
    }

    /// Start applying `change` (made on `m` by the user) to the other copies, when the setting is on.
    pub(super) fn cascade(&self, m: &Media, change: Change) -> Task<Message> {
        if !self.settings.sync_watched {
            return Task::none();
        }
        let backends: Vec<_> = self
            .catalog
            .online()
            .filter_map(|s| Some((s.id, s.backend.clone()?)))
            .collect();
        let from = m.clone();
        run(
            async move {
                let copies = w::find_copies(&from, &backends).await;
                (from, copies)
            },
            move |(from, copies)| Message::Cascade(Msg::Found(Box::new(from), change, copies)),
        )
    }

    pub(super) fn cascade_msg(&mut self, m: Msg) -> Task<Message> {
        let Msg::Found(from, change, copies) = m;
        let gate = Gate {
            enabled: self.settings.sync_watched,
            allow_server_writes: self.settings.allow_server_writes,
            dev_token: self.dev,
            sync_progress: self.settings.sync_progress,
        };
        let steps = w::plan(
            (from.origin, &from.id),
            &copies,
            change,
            Cause::User,
            gate,
            |o| self.via_of(o),
        );
        let mut tasks = vec![];
        for s in steps {
            let Some(copy) = copies
                .iter()
                .find(|c| (c.origin, &c.id) == (s.origin, &s.id))
            else {
                continue;
            };
            match (s.via, s.change) {
                (Via::Local, c) => {
                    let Some(l) = self
                        .catalog
                        .backend(s.origin)
                        .and_then(|b| b.local())
                        .cloned()
                    else {
                        continue;
                    };
                    let copy = copy.clone();
                    match c {
                        Change::Played(v) => {
                            self.patch(s.origin, &s.id, move |x| x.played = v);
                            let (origin, id) = (s.origin, s.id.clone());
                            tasks.push(run(
                                async move { l.set_played(&copy, v).await.is_ok() },
                                move |ok| Message::LocalWrite(origin, id, v, ok),
                            ));
                        }
                        Change::Resume { secs, .. } => {
                            tasks.push(run(
                                async move {
                                    let _ = l.set_progress(&copy, secs as u64).await;
                                },
                                |_| Message::Noop,
                            ));
                        }
                    }
                }
                (Via::Server, c) => {
                    let Some(h) = self.hooks_for(s.origin).filter(|h| h.can_write()) else {
                        continue;
                    };
                    match c {
                        Change::Played(v) => {
                            self.patch(s.origin, &s.id, move |x| {
                                x.played = v;
                                if v {
                                    x.resume_s = None;
                                }
                            });
                            h.set_played(&s.id, v);
                        }
                        // The hooks gate this on `sync_progress` themselves; a stop report stores the position.
                        Change::Resume { secs, .. } => h.stopped(&s.id, ticks(secs)),
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    /// What the player reported since the last look: Trakt scrobbles, and a finished item (or the
    /// stop position, with progress sync) carried to the other copies.
    pub(super) fn handle_play_events(&mut self, evs: Vec<PlayEvent>) -> Task<Message> {
        let mut tasks = vec![];
        for ev in evs {
            self.trakt_event(&ev);
            if ev.what != Reported::Stopped {
                continue;
            }
            let Some(dur) = ev.dur else { continue };
            if w::finished(ev.pos, dur) {
                tasks.push(self.cascade(&ev.media, Change::Played(true)));
            } else if ev.pos >= 5.0 {
                tasks.push(self.cascade(&ev.media, Change::Resume { secs: ev.pos, dur }));
            }
        }
        Task::batch(tasks)
    }
}
