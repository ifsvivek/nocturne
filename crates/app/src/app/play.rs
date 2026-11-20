//! Starting and ending playback: `Message::Play` resolves what to hand the player (a stream URL or a
//! local file) from the item's own source, opens the player page and leaves it when it reports it
//! closed. The page itself is `crate::player`.
use super::message::Message;
use super::{App, Phase};
use crate::player::{self, Outcome, Player, PlayerCtx};
use crate::task::{after, run};
use iced::Task;
use nocturne_core::backend::{Backend, PlayError, PlayTarget};
use nocturne_core::bridge;
use nocturne_core::local::LocalBackend;
use nocturne_core::source::{Media, SourceId};
use nocturne_core::video::PlayerHooks;
use std::sync::Arc;
use std::time::Duration;

/// The local library keeps watch progress itself (no server involved, so no setting gates it).
struct LocalHooks {
    backend: LocalBackend,
    origin: SourceId,
    find_subs: bool,
}

impl LocalHooks {
    fn save(&self, id: &str, ticks: i64) {
        let b = self.backend.clone();
        let m = Media {
            origin: self.origin,
            id: id.to_string(),
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        };
        let secs = (ticks.max(0) / 10_000_000) as u64;
        bridge::spawn(async move {
            let _ = b.set_progress(&m, secs).await;
        });
    }
}

impl PlayerHooks for LocalHooks {
    fn started(&self, _: &str, _: i64) {}
    fn progress(&self, id: &str, ticks: i64, _: bool) {
        self.save(id, ticks);
    }
    fn stopped(&self, id: &str, ticks: i64) {
        self.save(id, ticks);
    }
    fn set_favorite(&self, _: &str, _: bool) {}
    fn is_favorite(&self, _: &str) -> bool {
        false
    }
    fn can_write(&self) -> bool {
        false
    }
    fn subtitles(&self, id: &str) -> Vec<std::path::PathBuf> {
        self.backend.subtitles_of(id)
    }
    fn can_find_subs(&self) -> bool {
        self.find_subs
    }
}

impl App {
    /// `Message::Play`: find out what to open, then open the page.
    pub(super) fn start_play(
        &mut self,
        m: Media,
        rest: Vec<Media>,
        source_id: Option<String>,
    ) -> Task<Message> {
        if self.player.is_some() {
            return Task::none();
        }
        if self.sp.in_group() {
            // In a group the queue is everyone's: the server decides what plays.
            let t = self.sp.play(m, rest).map(Message::SyncPlay);
            return Task::batch([t, self.drain_sp()]);
        }
        let Some(b) = self.catalog.backend(m.origin).cloned() else {
            self.toast("That source isn't available right now.");
            return Task::none();
        };
        let m2 = m.clone();
        let sid = source_id.clone();
        run(
            async move { b.play_target_for(&m2, sid.as_deref()).await },
            move |r| Message::PlayTarget(m, rest, source_id, r),
        )
    }

    pub(super) fn play_target(
        &mut self,
        m: Media,
        rest: Vec<Media>,
        source_id: Option<String>,
        r: Result<PlayTarget, PlayError>,
    ) -> Task<Message> {
        let target = match r {
            Ok(t) => t,
            Err(e) => {
                self.toast(e.message());
                return Task::none();
            }
        };
        if self.player.is_some() || !matches!(self.phase, Phase::Ready) {
            return Task::none();
        }
        let ctx = self.player_ctx(m, rest, source_id, target);
        self.menu = false;
        let (mut p, open) = Player::open(ctx);
        if std::mem::take(&mut self.restore_q) {
            p.restore_music(&self.queue);
        }
        self.player = Some(p);
        open.map(Message::Player)
    }

    /// Everything the player page is opened with for `m` and the queue after it.
    pub(super) fn player_ctx(
        &self,
        m: Media,
        rest: Vec<Media>,
        source_id: Option<String>,
        target: PlayTarget,
    ) -> PlayerCtx {
        let hooks: Option<Arc<dyn PlayerHooks>> = match self.catalog.backend(m.origin) {
            Some(Backend::Local(l)) => Some(Arc::new(LocalHooks {
                backend: l.clone(),
                origin: m.origin,
                find_subs: self.subtitles_on(m.origin),
            })),
            _ => self.hooks_for(m.origin).map(|h| {
                h.seed_favorite(&m.id, m.favorite);
                h as Arc<dyn PlayerHooks>
            }),
        };
        let queue = std::iter::once(m.clone()).chain(rest).collect();
        PlayerCtx {
            media: m,
            queue,
            target,
            source_id,
            resume: None,
            backends: self
                .catalog
                .online()
                .filter_map(|s| Some((s.id, s.backend.clone()?)))
                .collect(),
            hooks,
            prefs: self.settings.prefs(),
            buffering: self.settings.buffering(),
        }
    }

    /// Route a player message (and keys) and leave the page when it says it is over.
    pub(super) fn player_msg(&mut self, msg: player::Msg) -> Task<Message> {
        // The Theme Studio's pick mode takes the bar's pointer events first.
        #[cfg(feature = "theme-engine")]
        if let player::Msg::Eng(e) = &msg
            && let Some(t) = self.studio_event(super::message::Which::Bar, e)
        {
            return t;
        }
        let Some(p) = self.player.as_mut() else {
            return Task::none();
        };
        let t = p.update(msg);
        self.after_player(t)
    }

    pub(super) fn after_player(&mut self, t: Task<player::Msg>) -> Task<Message> {
        let mut tasks = vec![t.map(Message::Player)];
        if let Some(p) = self.player.as_mut() {
            let evs = p.take_events();
            if !evs.is_empty() {
                tasks.push(self.handle_play_events(evs));
            }
        }
        // "Remember my choice" on the HDR prompt is the HDR video setting from now on.
        if let Some(m) = self.player.as_mut().and_then(|p| p.take_hdr_choice()) {
            self.settings.hdr = m;
            self.settings.save();
        }
        if let Some(Outcome::Closed { failed, why }) =
            self.player.as_mut().and_then(|p| p.take_outcome())
        {
            if let Some(q) = self.player.as_ref().and_then(|p| p.music_snapshot()) {
                self.queue = q;
            }
            self.player = None;
            self.sp.player_closed();
            if failed {
                match why {
                    Some(w) => self.toast(format!("Couldn't start playback: {w}.")),
                    None => self.toast("This couldn't be played."),
                }
            }
            // The server has the final report shortly after; then Home's rows have moved.
            tasks.push(after(Duration::from_millis(900), Message::RefreshResume));
            // The page under the player shows resume / watched state: read it again.
            let pid = self.stack.last().map(|p| p.pid);
            if let Some((pid, d)) =
                pid.zip(self.stack.last_mut().and_then(|p| p.det.as_deref_mut()))
            {
                tasks.push(d.reload().map(move |m| Message::Detail(pid, m)));
            }
        }
        if let Some(m) = self.player.as_mut().and_then(|p| p.take_find_subs()) {
            tasks.push(Task::done(Message::Online(super::online::Msg::Find(
                Box::new(m),
            ))));
        }
        Task::batch(tasks)
    }

    /// Look again at Continue watching and Next up after playback.
    pub(super) fn refresh_resume(&mut self) -> Task<Message> {
        let Some(pid) = self
            .stack
            .first()
            .filter(|p| p.home.is_some())
            .map(|p| p.pid)
        else {
            return Task::none();
        };
        let (rf, t2) = self.fan(pid, super::message::FanKind::Resume, |b| async move {
            b.resume().await
        });
        let (nf, t3) = self.fan(pid, super::message::FanKind::NextUp, |b| async move {
            b.next_up().await
        });
        if let Some(h) = self.page_mut(pid).and_then(|p| p.home.as_deref_mut()) {
            (h.resume, h.nextup) = (rf, nf);
        }
        Task::batch([t2, t3])
    }

    /// Quit: stop playback first (reports, destroys the video surface), then the process.
    pub(super) fn quit(&mut self) -> Task<Message> {
        self.sp.shutdown(self.player.as_mut());
        if let Some(p) = self.player.as_mut() {
            p.shutdown();
            // The last "stopped" still reaches Trakt; the cascade needs the UI and is skipped on exit.
            for ev in p.take_events() {
                self.trakt_event(&ev);
            }
        }
        self.player = None;
        crate::timing::report();
        self.settings.save();
        crate::platform::remove_art_file();
        iced::exit()
    }
}
