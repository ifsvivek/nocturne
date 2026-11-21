//! The player page (port of the GPUI `PlayerView`): playback backend, controls state, the
//! Jellyfin reads it needs (streams, segments, trickplay, queue), progress reporting, favourites,
//! MPRIS and the SyncPlay hooks. A plain struct with its own `Msg`; the root owns one while the
//! page is open and maps `Msg` into its own message type. Rendering is in `view.rs`; the pure
//! rules are in `logic.rs` and in core's `video.rs` / `player_logic.rs`.
//!
//! Teardown order matters (docs/video-embedding.md): [`Player::shutdown`] (or `close`) stops
//! reporting and drops the playback object, which joins the render thread and destroys the
//! subsurface, BEFORE the window or the page goes away.
//!
//! Root hook (the only thing the root has to do):
//! - hold `Option<Player>`; open with `Player::open(ctx)` and run the returned `Task` mapped
//!   into the root message;
//! - route `Msg` to `Player::update`, keys to `Player::key`, include `Player::subscription()`;
//! - after each update read `Player::take_outcome()` (`Outcome::Closed`) and leave the page;
//! - draw `Player::view()` as the whole page (it paints its own backdrop and leaves the video
//!   hole transparent); call `Player::shutdown()` before closing the window.
#[cfg(feature = "theme-engine")]
mod bar_tree;
mod controls;
mod input;
pub mod logic;
mod popovers;
#[cfg(debug_assertions)]
pub mod preview;
mod view;

use crate::keys::Key;
use crate::motion;
use crate::platform::Handles;
use crate::task;
use crate::widget::buffering::{self, Progress};
use crate::widget::track::Ev;
use iced::widget::image::Handle;
use iced::{Point, Rectangle, Size, Subscription, Task, event, mouse, window};
use nocturne_core::backend::{Backend, PlayError, PlayTarget};
use nocturne_core::hdr::{self, HdrMode};
use nocturne_core::lang_memory;
use nocturne_core::player_logic::{
    self as pl, AudioPick, Method, Quality, Seg, SubPick, TrackChoice,
};
use nocturne_core::player_mpris::{Cmd, Mpris, Snap};
use nocturne_core::remote;
use nocturne_core::source::{Art, Kind, Media, ProviderIds, SourceId};
use nocturne_core::sync::{PlayInfo, PlayMethod, Reporter, Tick as SyncTick, ticks};
use nocturne_core::syncplay::GroupReq;
use nocturne_core::video::{
    self, Aspect, AutoHide, MenuAct, Playback, PlayerHooks, PlayerPrefs, Pop, Repeat, Settings,
};
use nocturne_jellyfin as jf;
use nocturne_player::{
    Buffering, EmbeddedPlayer, Event, Options, Player as ExternalPlayer, VlcPlayer,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Height of the two-row controls bar.
pub const BAR_H: f32 = 96.0;

/// What the caller supplies for one playback session.
#[derive(Clone)]
pub struct PlayerCtx {
    pub media: Media,
    /// The queue the item is part of (episodes of the season, a playlist, ...); may be just
    /// `media`. The page finds `media` in it by id.
    pub queue: Vec<Media>,
    /// What to hand the player for `media`.
    pub target: PlayTarget,
    /// The version picked on the detail page (Jellyfin media source id), if any.
    pub source_id: Option<String>,
    /// Resume position (seconds); falls back to the item's own saved position.
    pub resume: Option<f64>,
    /// Backends by source, for item reads and for items later in the queue.
    pub backends: Vec<(SourceId, Backend)>,
    /// Account hooks of the signed-in server (favourite, progress reports). Every method gates
    /// itself on `allow_server_writes` / `sync_progress`; `None` in developer-token mode.
    pub hooks: Option<Arc<dyn PlayerHooks>>,
    pub prefs: PlayerPrefs,
    pub buffering: Buffering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Playback is over; the root leaves the page.
    /// `why`: what to tell the user when it failed, if known (shown after "Couldn't start playback: ").
    Closed {
        failed: bool,
        why: Option<&'static str>,
    },
}

#[derive(Clone)]
pub enum Sheet {
    Loading,
    Ready(Handle),
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub enum Drag {
    Seek(f32),
    Vol(f32),
}

#[derive(Clone, Copy, Debug)]
pub enum CtxAct {
    Key(video::Key),
    Pop(Pop),
}

/// Pointer-driven actions from the view.
#[derive(Clone, Debug)]
pub enum Act {
    Toggle,
    Seek(f64),
    Step(i8),
    Mute,
    Pop(Pop),
    Menu(MenuAct),
    Queue,
    Jump(usize),
    Shuffle,
    Repeat,
    Fav,
    Mini,
    Fullscreen,
    Stop,
    Skip,
    NextPlay,
    NextCancel,
    Resume(bool),
    /// Answer to the HDR prompt (true = pop-out), and its remember box.
    Hdr(bool),
    HdrRemember,
    ShowTotal,
    Ctx(CtxAct),
    BarHover(bool),
    VideoPress,
    VideoDouble,
    VideoRight,
}

#[derive(Clone)]
pub enum Msg {
    Window(Option<window::Id>),
    Scale(f32),
    Size(Size),
    Handles(crate::platform::Handles),
    Hole(Rectangle),
    /// A panel drawn over the picture laid out at these bounds (slot, see [`Player::open_panels`]).
    Cover(usize, Rectangle),
    Tick(Instant),
    Frame(Instant),
    Mouse(Point),
    Key(Key),
    Act(Act),
    Seek(Ev),
    Vol(Ev),
    /// An event of the engine's paint widget (the controls bar).
    #[cfg(feature = "theme-engine")]
    Eng(crate::widget::styled::StyledEvent),
    Target(u64, Result<PlayTarget, PlayError>),
    Item(u64, Option<jf::PlayItem>),
    Segs(u64, Vec<jf::Segment>),
    Art(u64, Option<Handle>, Option<Handle>),
    Poster(u64, Option<Vec<u8>>),
    Episodes(u64, Vec<Media>),
    Sheet(u64, u32, Option<Handle>),
    /// Provider ids of the playing episode's series (for the language memory key).
    Series(u64, Option<(SourceId, String, ProviderIds)>),
}

/// Panels that can sit over the picture: popover, queue, context menu, OSD, stats, help, note, next-episode card, skip chip, scrub tip.
pub const COVERS: usize = 10;

pub struct Player {
    #[cfg(feature = "theme-engine")]
    bar: Option<std::cell::RefCell<bar_tree::BarHost>>,
    /// The Theme Studio's marks, pick state and docked width while it is open.
    #[cfg(feature = "theme-engine")]
    pub studio: Option<crate::widget::styled::Probe>,
    pub ctx: PlayerCtx,
    pub media: Media,
    pub queue: Vec<Media>,
    pub qi: usize,
    pub playback: Option<Playback>,
    /// Windows: the child window mpv draws into (declared after `playback`, so mpv goes first).
    surface: Option<nocturne_player::VideoSurface>,
    /// Stream waiting for the backend to exist (the window handles arrive asynchronously).
    pending: Option<(String, Vec<String>)>,
    /// The target of the current item as the backend resolved it.
    target: Option<PlayTarget>,
    pub embed: bool,
    /// VLC's own window (Settings > Player), driven over its HTTP interface.
    vlc: bool,
    pub video: bool,
    /// No mpv at all: the message to show (install hint).
    pub problem: Option<String>,
    win_id: Option<window::Id>,
    pub win: Size,
    scale: f32,
    pub hole: Rectangle,
    /// Bounds of each open panel (Windows: holes cut in the video window), `None` while closed or not yet laid out.
    covers: [Option<Rectangle>; COVERS],
    pub now: Instant,
    pub epoch: u64,
    pub hide: AutoHide,
    pub ctl_shown: bool,
    sub_lifted: bool,
    pub volume: f64,
    pub muted_from: Option<f64>,
    pub fullscreen: bool,
    fs_from_video: bool,
    pub tracks: Vec<video::Track>,
    pub chapters: Vec<(f64, String)>,
    pub pop: Option<Pop>,
    pub pop_sel: usize,
    pub set: Settings,
    pub show_total: bool,
    pub osd: Option<(String, Instant)>,
    pub stats: bool,
    pub stats_rows: Vec<(&'static str, String)>,
    pub help: bool,
    pub cache_end: Option<f64>,
    pub drag: Option<Drag>,
    pub hover_frac: Option<f32>,
    pub bar_hover: bool,
    pub note: Option<(String, Instant)>,
    pub mouse: Point,
    // per-item state
    pub state: Option<nocturne_player::State>,
    pub loaded: bool,
    pub ended: bool,
    started: Instant,
    seek_on_load: Option<f64>,
    first_load: bool,
    play_item: Option<jf::PlayItem>,
    source_override: Option<String>,
    pub segs: Vec<Seg>,
    skipped: Vec<usize>,
    pub next_since: Option<Instant>,
    pub next_dismissed: bool,
    pub resume_prompt: Option<f64>,
    pub trick: Option<(String, jf::TrickplayInfo)>,
    pub sheets: HashMap<u32, Sheet>,
    ext_added: bool,
    find_subs_req: bool,
    applied: bool,
    transcoding: bool,
    play_session: String,
    hwdec_logged: bool,
    // loading overlay
    pub prog: Progress,
    pub born: Instant,
    pub splash: bool,
    pub leaving: Option<Instant>,
    stall_since: Option<Instant>,
    pub light: bool,
    pub backdrop: Option<Handle>,
    pub logo: Option<Handle>,
    // panels
    pub queue_open: bool,
    pub queue_sel: usize,
    pub shuffle: bool,
    pub(crate) unshuffled: Vec<Media>,
    pub ctx_menu: Option<Point>,
    pub mini: bool,
    restore_size: Option<Size>,
    pub fav: bool,
    // reporting
    mpris: Option<Mpris>,
    art_url: String,
    reporter: Reporter,
    /// What the reporting path saw, for the root (Trakt, watched-state sync); see [`PlayEvent`].
    events: Vec<PlayEvent>,
    closed: bool,
    outcome: Option<Outcome>,
    /// Why the page is failing, when it is more than "could not be played".
    fail_why: Option<&'static str>,
    /// SyncPlay: transport controls become requests to the group (`outbox`) and the group's
    /// commands are applied through the `group_*` methods.
    pub group: bool,
    pub outbox: Vec<GroupReq>,
    pub(crate) ready_due: Option<f64>,
    eof_sent: bool,
    pub group_diff: Option<f64>,
    // HDR (docs/HDR.md)
    /// The window handles, kept so the pop-out can hand the video back to the window.
    handles: Option<Handles>,
    /// The compositor offers the colour-management protocol.
    compositor_cm: bool,
    /// The video plays in the HDR pop-out (an external mpv window) instead of the window.
    pub popout: bool,
    hdr_checked: bool,
    hdr_since: Option<Instant>,
    pub hdr_prompt: Option<hdr::Prompt>,
    /// "Remember my choice" ticked: the root stores it as the setting.
    hdr_choice: Option<HdrMode>,
    /// The playback object was swapped: per-session settings are applied again on load.
    switched: bool,
    target_checked: bool,
    series_ids: Option<(SourceId, String, ProviderIds)>,
    subs_retry: Option<Instant>,
}

impl Player {
    pub fn open(ctx: PlayerCtx) -> (Self, Task<Msg>) {
        Self::build(ctx, None)
    }

    /// A player that follows a SyncPlay group: starts paused at `start` seconds.
    pub fn open_group(ctx: PlayerCtx, start: f64) -> (Self, Task<Msg>) {
        Self::build(ctx, Some(start))
    }

    fn build(ctx: PlayerCtx, group: Option<f64>) -> (Self, Task<Msg>) {
        let env = std::env::var("NOCTURNE_VIDEO").ok();
        let vlc_ok = match &ctx.target {
            PlayTarget::Url { url, headers } => {
                let h: Vec<&str> = headers.iter().map(String::as_str).collect();
                nocturne_player::vlc_url(url, &h).is_ok()
            }
            PlayTarget::File(_) => true,
        };
        let (choice, notice) = video::choose_player(
            ctx.prefs.player,
            env.as_deref(),
            crate::platform::can_embed(),
            video::Installed::detect(),
            vlc_ok,
        );
        let embed = choice == video::Choice::Embedded;
        let vlc = choice == video::Choice::Vlc;
        let now = Instant::now();
        let media = ctx.media.clone();
        let mut queue = ctx.queue.clone();
        let qi = match queue
            .iter()
            .position(|m| m.id == media.id && m.origin == media.origin)
        {
            Some(i) => i,
            None => {
                queue = vec![media.clone()];
                0
            }
        };
        let target = ctx.target.clone();
        let mut me = Self {
            #[cfg(feature = "theme-engine")]
            bar: bar_tree::BarHost::new(),
            #[cfg(feature = "theme-engine")]
            studio: None,
            media: media.clone(),
            queue,
            qi,
            playback: None,
            surface: None,
            pending: None,
            target: Some(target),
            embed,
            vlc,
            video: embed,
            problem: None,
            win_id: None,
            win: Size::new(1280.0, 800.0),
            scale: 1.0,
            hole: Rectangle::new(Point::ORIGIN, Size::new(1.0, 1.0)),
            covers: [None; COVERS],
            now,
            epoch: 0,
            hide: AutoHide::new(now),
            ctl_shown: true,
            sub_lifted: false,
            volume: 100.0,
            muted_from: None,
            fullscreen: false,
            fs_from_video: false,
            tracks: vec![],
            chapters: vec![],
            pop: None,
            pop_sel: 0,
            set: Settings::default(),
            show_total: false,
            osd: None,
            stats: false,
            stats_rows: vec![],
            help: false,
            cache_end: None,
            drag: None,
            hover_frac: None,
            bar_hover: false,
            note: None,
            mouse: Point::ORIGIN,
            state: None,
            loaded: false,
            ended: false,
            started: now,
            seek_on_load: group,
            first_load: true,
            play_item: None,
            source_override: ctx.source_id.clone(),
            segs: vec![],
            skipped: vec![],
            next_since: None,
            next_dismissed: false,
            resume_prompt: None,
            trick: None,
            sheets: HashMap::new(),
            ext_added: false,
            find_subs_req: false,
            applied: false,
            transcoding: false,
            play_session: String::new(),
            hwdec_logged: false,
            prog: Progress::new(now),
            born: now,
            splash: true,
            leaving: None,
            stall_since: None,
            light: false,
            backdrop: None,
            logo: None,
            queue_open: false,
            queue_sel: 0,
            shuffle: false,
            unshuffled: vec![],
            ctx_menu: None,
            mini: false,
            restore_size: None,
            fav: false,
            mpris: Some(Mpris::start()),
            art_url: String::new(),
            reporter: Reporter::default(),
            events: vec![],
            closed: false,
            outcome: None,
            fail_why: None,
            group: group.is_some(),
            outbox: vec![],
            ready_due: None,
            eof_sent: false,
            group_diff: None,
            handles: None,
            compositor_cm: false,
            popout: false,
            hdr_checked: false,
            hdr_since: None,
            hdr_prompt: None,
            hdr_choice: None,
            switched: false,
            target_checked: false,
            series_ids: None,
            subs_retry: None,
            ctx,
        };
        if let Some(n) = notice {
            me.toast(n);
        }
        let first = me.begin(media, true);
        (me, Task::batch([window::oldest().map(Msg::Window), first]))
    }

    /// Reports made since the last call (start / progress / stop of an item), oldest first.
    pub fn take_events(&mut self) -> Vec<PlayEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }

    // ---- subscriptions ----

    pub fn subscription(&self) -> Subscription<Msg> {
        let mut subs = vec![
            iced::time::every(Duration::from_millis(200)).map(Msg::Tick),
            window::resize_events().map(|(_, s)| Msg::Size(s)),
            event::listen_with(|e, _, _| match e {
                iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::Mouse(position))
                }
                _ => None,
            }),
        ];
        if self.animating() {
            subs.push(window::frames().map(Msg::Frame));
        }
        Subscription::batch(subs)
    }

    /// Frames only while the overlay moves (breathing, zoom, wipe, fade).
    pub fn animating(&self) -> bool {
        !self.closed
            && ((self.splash && !motion::reduced())
                || self.leaving.is_some()
                || (self.light && !motion::reduced()))
    }

    // ---- update ----

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        if self.closed {
            return Task::none();
        }
        match msg {
            Msg::Window(Some(id)) => {
                self.win_id = Some(id);
                return Task::batch([
                    window::scale_factor(id).map(Msg::Scale),
                    window::size(id).map(Msg::Size),
                    if self.embed {
                        crate::platform::handles(id).map(Msg::Handles)
                    } else {
                        Task::done(Msg::Handles(crate::platform::Handles::Other))
                    },
                ]);
            }
            Msg::Window(None) => self.problem = Some("No window to play in.".into()),
            Msg::Scale(s) => {
                self.scale = s;
                self.push_bounds();
            }
            Msg::Size(s) => {
                self.win = s;
                if let Some(id) = self.win_id {
                    return window::scale_factor(id).map(Msg::Scale);
                }
            }
            Msg::Handles(h) => self.make_backend(h),
            Msg::Cover(i, r) => {
                if let Some(c) = self.covers.get_mut(i) {
                    *c = Some(r);
                }
                self.sync_surface();
            }
            Msg::Hole(r) => {
                self.hole = r;
                self.push_bounds();
                self.apply_sub_pos();
            }
            Msg::Tick(now) => {
                self.now = now;
                return self.tick();
            }
            Msg::Frame(now) => self.now = now,
            Msg::Mouse(p) => {
                self.mouse = p;
                self.poke();
            }
            Msg::Key(k) => {
                if let Some(t) = self.key(&k) {
                    return t;
                }
            }
            Msg::Act(a) => return self.act(a),
            Msg::Seek(e) => return self.on_seek(e),
            Msg::Vol(e) => self.on_vol(e),
            #[cfg(feature = "theme-engine")]
            Msg::Eng(e) => return self.bar_event(e),
            Msg::Target(epoch, r) if epoch == self.epoch => match r {
                Ok(t) => {
                    self.target = Some(t);
                    return self.load_current();
                }
                Err(e) => {
                    self.toast(e.message());
                    return self.close(true);
                }
            },
            Msg::Item(epoch, r) if epoch == self.epoch => self.on_play_item(r),
            Msg::Segs(epoch, raw) if epoch == self.epoch => self.segs = pl::segments(&raw),
            Msg::Art(epoch, b, l) if epoch == self.epoch => (self.backdrop, self.logo) = (b, l),
            Msg::Poster(epoch, b) if epoch == self.epoch => {
                self.art_url = b
                    .and_then(|b| crate::platform::write_art(&b))
                    .unwrap_or_default();
            }
            Msg::Episodes(epoch, q) if epoch == self.epoch => {
                if let Some(i) = q.iter().position(|e| e.id == self.media.id) {
                    self.queue = q;
                    self.qi = i;
                }
            }
            Msg::Sheet(epoch, sheet, h) if epoch == self.epoch => {
                self.sheets
                    .insert(sheet, h.map_or(Sheet::Failed, Sheet::Ready));
            }
            Msg::Series(epoch, Some(r)) if epoch == self.epoch => self.series_ids = Some(r),
            _ => {} // a stale response: the user moved on
        }
        Task::none()
    }

    // ---- backends ----

    fn make_backend(&mut self, h: Handles) {
        if self.playback.is_some() {
            return;
        }
        self.handles = Some(h);
        if self.vlc {
            match VlcPlayer::new() {
                Ok(p) => self.playback = Some(Playback::Vlc(p)),
                Err(e) => {
                    eprintln!("video: VLC could not start ({e}), using the mpv window");
                    self.vlc = false;
                    self.toast("VLC could not be started, using the mpv window.");
                }
            }
        }
        if let (true, Handles::Win32 { hwnd }) = (self.embed, h) {
            self.make_surface_backend(hwnd);
        } else if let (true, Handles::Wayland { display, surface }) = (self.embed, h) {
            match nocturne_player::libmpv_available() {
                Ok(()) => {
                    self.compositor_cm =
                        nocturne_player::colour_management_available(display as *mut _);
                    match self.new_embedded(display, surface) {
                        Ok(p) => self.playback = Some(Playback::Embedded(p)),
                        Err(e) => eprintln!(
                            "video: embedded unavailable ({e}), using the external player"
                        ),
                    }
                }
                Err(e) => eprintln!("video: libmpv unavailable ({e}), using the external player"),
            }
        } else if self.embed {
            eprintln!("video: no window handle to attach to, using the external player");
        }
        if self.playback.is_none() {
            self.embed = false;
            self.video = false;
            if !nocturne_player::mpv_binary_available() {
                self.problem = Some(format!(
                    "Playback needs mpv, which was not found. {}",
                    nocturne_player::install_hint()
                ));
                self.pending = None;
                return;
            }
            // mpv's own window plays HDR itself (gpu-next); ask the compositor for an HDR output.
            match ExternalPlayer::new(Options {
                buffering: self.ctx.buffering,
                hdr_output: true,
                ..Default::default()
            }) {
                Ok(p) => self.playback = Some(Playback::External(p)),
                Err(e) => {
                    self.problem = Some(format!("Could not start mpv: {e}"));
                    self.pending = None;
                    return;
                }
            }
        }
        if let Some((url, headers)) = self.pending.take() {
            if self.group {
                self.setp("pause", "yes");
            }
            if self.start(&url, &headers).is_err() {
                self.outcome_failed_soon();
            }
        }
    }

    /// Windows: mpv draws into a child window of the app window. If that cannot be set up the
    /// separate mpv window is used (the caller continues with the external path).
    fn make_surface_backend(&mut self, hwnd: isize) {
        if !nocturne_player::mpv_binary_available() {
            return; // the external path reports the missing mpv
        }
        let Some(surface) = nocturne_player::VideoSurface::create(hwnd) else {
            eprintln!("video: could not create the child window, using mpv's own window");
            self.toast("Could not play inside the window, using a separate mpv window.");
            return;
        };
        match ExternalPlayer::new(Options {
            buffering: self.ctx.buffering,
            wid: Some(surface.hwnd()),
            ..Default::default()
        }) {
            Ok(p) => {
                self.surface = Some(surface);
                self.playback = Some(Playback::External(p));
                self.sync_surface();
            }
            Err(e) => eprintln!("video: in-window mpv failed to start ({e})"),
        }
    }

    /// The picture could not be shown inside the window (mpv left before the first frame):
    /// continue in mpv's own window and say so.
    fn fall_back_to_window(&mut self) -> Task<Msg> {
        eprintln!("video: in-window playback failed, using mpv's own window");
        self.playback = None;
        self.surface = None;
        self.embed = false;
        self.video = false;
        match self.new_popout_window() {
            Ok(p) => self.playback = Some(p),
            Err(e) => {
                self.problem = Some(format!("Could not start mpv: {e}"));
                return Task::none();
            }
        }
        self.switched = true;
        self.toast("Could not play inside the window, using a separate mpv window.");
        self.reload_at(0.0)
    }

    /// mpv's own window (no HDR pop-out behaviour, not fullscreen).
    fn new_popout_window(&self) -> Result<Playback, String> {
        ExternalPlayer::new(Options {
            buffering: self.ctx.buffering,
            hdr_output: true,
            ..Default::default()
        })
        .map(Playback::External)
        .map_err(|e| e.to_string())
    }

    /// Which of the [`COVERS`] panels the view draws right now.
    fn open_panels(&self) -> [bool; COVERS] {
        let bar = self.controls_visible();
        [
            bar && self.pop.is_some(),
            self.queue_open,
            self.ctx_menu.is_some(),
            self.osd.is_some(),
            self.stats,
            self.help,
            self.note.is_some(),
            self.next_card().is_some(),
            self.active_seg().is_some(),
            bar && self.hover_frac.is_some() && self.dur().is_some(),
        ]
    }

    /// Windows: places the child window and cuts holes in it where panels are drawn; hidden
    /// while a full-screen overlay needs the pixels.
    fn sync_surface(&mut self) {
        if self.surface.is_none() {
            return;
        }
        let covered = self.splash
            || self.popout
            || self.light
            || self.problem.is_some()
            || self.resume_prompt.is_some()
            || self.hdr_prompt.is_some();
        let open = self.open_panels();
        for (c, o) in self.covers.iter_mut().zip(open) {
            if !o {
                *c = None;
            }
        }
        let cuts: Vec<_> = self
            .covers
            .iter()
            .flatten()
            .map(|r| (r.x, r.y, r.width, r.height))
            .collect();
        let r = logic::video_rect(self.hole, BAR_H, self.controls_visible(), covered);
        let scale = self.scale;
        if let Some(s) = self.surface.as_mut() {
            match r {
                Some(r) => {
                    s.set_cutouts(&cuts, scale);
                    s.set_bounds(r.x, r.y, r.width, r.height, scale);
                }
                None => s.set_visible(false),
            }
        }
    }

    /// The in-window player on the window's video surface, sized to the hole.
    fn new_embedded(
        &self,
        display: usize,
        surface: usize,
    ) -> nocturne_player::Result<Box<EmbeddedPlayer>> {
        let mut p = EmbeddedPlayer::with_buffering(
            display as *mut _,
            surface as *mut _,
            self.ctx.buffering,
        )?;
        let _ = p.set_volume(self.volume);
        let r = self.hole;
        p.set_bounds(r.x, r.y, r.width, r.height, self.scale);
        Ok(Box::new(p))
    }

    /// A load error right at the start: handled in the next tick like any other failure.
    fn outcome_failed_soon(&mut self) {
        self.started = Instant::now() - Duration::from_secs(10);
    }

    fn push_bounds(&mut self) {
        let (r, s) = (self.hole, self.scale);
        if let Some(Playback::Embedded(p)) = self.playback.as_mut() {
            p.set_bounds(r.x, r.y, r.width, r.height, s);
        }
        self.sync_surface();
    }

    fn start(&self, url: &str, headers: &[String]) -> Result<(), ()> {
        match &self.playback {
            Some(p) => p.load(url, headers).map_err(|_| ()),
            None => Err(()),
        }
    }

    // ---- loading an item ----

    fn backend_of(&self, m: &Media) -> Option<Backend> {
        self.ctx
            .backends
            .iter()
            .find(|(id, _)| *id == m.origin)
            .map(|(_, b)| b.clone())
    }

    fn jf(&self) -> Option<(Arc<jf::Client>, String)> {
        self.backend_of(&self.media)?.jellyfin().map(|s| s.client())
    }

    fn source_id(&self) -> String {
        self.source_override
            .clone()
            .or_else(|| {
                self.play_item
                    .as_ref()
                    .and_then(|p| p.media_sources.first())
                    .map(|s| s.id.clone())
            })
            .unwrap_or_else(|| self.media.id.clone())
    }

    /// (url, header) of the current item at the current quality.
    fn stream(&self) -> Option<(String, Vec<String>)> {
        if let Some((c, _)) = self.jf()
            && let Some(t) = self
                .set
                .quality
                .transcode(&self.source_id(), &self.play_session)
        {
            // Same headers as the direct stream: the token first, then the server's extra one.
            let mut headers = vec![format!("Authorization: {}", c.auth_header())];
            headers.extend(c.extra_header_line());
            return Some((c.transcode_url(&self.media.id, &t), headers));
        }
        match self.target.as_ref()? {
            PlayTarget::Url { url, headers } => Some((url.clone(), headers.clone())),
            PlayTarget::File(p) => Some((p.to_string_lossy().into_owned(), vec![])),
        }
    }

    /// Hands the current item's stream to the backend (or parks it until the backend exists).
    fn load_current(&mut self) -> Task<Msg> {
        let Some((url, headers)) = self.stream() else {
            return self.close(true);
        };
        if self.playback.is_none() {
            self.pending = Some((url, headers));
            return Task::none();
        }
        if self.group {
            self.setp("pause", "yes"); // held until the group says go
        }
        if self.start(&url, &headers).is_err() {
            return self.fail();
        }
        Task::none()
    }

    /// Starts (or switches to) `m`: resets per-item state, loads it, and fetches what the controls
    /// need. `first`: the context's target is used instead of asking the backend.
    fn begin(&mut self, m: Media, first: bool) -> Task<Msg> {
        self.epoch += 1;
        let epoch = self.epoch;
        self.media = m.clone();
        self.set.find_subs = self.ctx.hooks.as_ref().is_some_and(|h| h.can_find_subs())
            && self.backend_of(&m).is_some_and(|b| b.local().is_some());
        self.state = None;
        self.loaded = false;
        self.ended = false;
        self.started = Instant::now();
        self.play_item = None;
        self.segs.clear();
        self.skipped.clear();
        self.next_since = None;
        self.next_dismissed = false;
        self.resume_prompt = None;
        self.trick = None;
        self.sheets.clear();
        self.ext_added = false;
        self.applied = false;
        self.subs_retry = None;
        self.tracks.clear();
        self.chapters.clear();
        self.art_url.clear();
        self.backdrop = None;
        self.logo = None;
        self.hwdec_logged = false;
        self.hdr_checked = false;
        self.hdr_since = None;
        self.hdr_prompt = None;
        self.target_checked = false;
        self.set.hdr_popout = None;
        self.splash = true;
        self.leaving = None;
        self.light = false;
        self.stall_since = None;
        self.born = Instant::now();
        self.prog = Progress::new(self.born);
        self.play_session = format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        self.fav = self
            .ctx
            .hooks
            .as_ref()
            .is_some_and(|h| h.is_favorite(&m.id))
            || m.favorite;
        self.transcoding = self.set.quality != Quality::Original;
        self.set.method = if self.transcoding {
            Method::Transcode.label()
        } else {
            Method::DirectPlay.label()
        };
        self.poke();
        if !first {
            self.source_override = None;
            self.target = None;
        }
        self.report_info();
        let mut tasks = vec![];
        let backend = self.backend_of(&m);
        if first {
            tasks.push(self.load_current());
        } else if let Some(b) = backend.clone() {
            let m2 = m.clone();
            tasks.push(task::run(
                async move { b.play_target(&m2).await },
                move |r| Msg::Target(epoch, r),
            ));
        } else {
            return self.close(true);
        }
        let Some(b) = backend else {
            return Task::batch(tasks);
        };
        // The overlay's art and the desktop widgets' poster (both read-only, any source).
        let (b2, m2) = (b.clone(), m.clone());
        tasks.push(task::run(
            async move {
                let bd = b2.image(&m2, Art::Backdrop, 1280).await;
                let lg = b2.image(&m2, Art::Logo, 600).await;
                (bd, lg)
            },
            move |(bd, lg)| {
                Msg::Art(
                    epoch,
                    bd.map(Handle::from_bytes),
                    lg.map(Handle::from_bytes),
                )
            },
        ));
        let (b2, m2) = (b.clone(), m.clone());
        tasks.push(task::run(
            async move { b2.image(&m2, Art::Poster, 300).await },
            move |r| Msg::Poster(epoch, r),
        ));
        if let Some(series) = m.series_id.clone()
            && self
                .series_ids
                .as_ref()
                .is_none_or(|(o, id, _)| (*o, id) != (m.origin, &series))
        {
            // Read-only; the ids only widen the language memory's key (same show on two sources).
            let (b2, origin) = (b.clone(), m.origin);
            tasks.push(task::run(
                async move {
                    let (s, _) = b2.full(&series).await.ok()?;
                    (!s.ids.is_empty()).then_some((origin, series, s.ids))
                },
                move |r| Msg::Series(epoch, r),
            ));
        }
        if let Some(s) = b.jellyfin() {
            let (c, user) = s.client();
            let id = m.id.clone();
            let (c1, u1, id1) = (c.clone(), user.clone(), id.clone());
            tasks.push(task::run(
                async move { c1.play_item(&u1, &id1).await.ok() },
                move |r| Msg::Item(epoch, r),
            ));
            let (c2, id2) = (c.clone(), id.clone());
            tasks.push(task::run(
                async move { c2.media_segments(&id2).await },
                move |r| Msg::Segs(epoch, r),
            ));
            if self.queue.len() <= 1
                && m.kind == Kind::Episode
                && let Some(series) = m.series_id.clone()
            {
                let origin = m.origin;
                tasks.push(task::run(
                    async move {
                        c.episodes(&user, &series, None)
                            .await
                            .ok()
                            .map(|i| {
                                i.items
                                    .iter()
                                    .map(|e| Media::from_item_in(origin, e))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    },
                    move |q| Msg::Episodes(epoch, q),
                ));
            }
        }
        Task::batch(tasks)
    }

    fn on_play_item(&mut self, r: Option<jf::PlayItem>) {
        let Some(p) = r else { return };
        self.trick = p.trickplay_for(400);
        if let Some(s) = p.media_sources.first() {
            let h = s
                .media_streams
                .iter()
                .find(|x| x.kind == "Video")
                .and_then(|x| x.height);
            self.set.method = if self.transcoding {
                self.set.quality.method(h, s.bitrate).label()
            } else {
                Method::DirectPlay.label()
            };
        }
        self.play_item = Some(p);
        self.report_info();
        self.set.can_quality = true;
        self.name_and_ext_subs();
    }

    // ---- state helpers ----

    /// Tells the account hooks how this item is delivered, so progress reports are truthful.
    fn report_info(&self) {
        let Some(h) = &self.ctx.hooks else { return };
        let method = match self.set.method {
            m if m == Method::DirectPlay.label() => PlayMethod::DirectPlay,
            m if m == Method::DirectStream.label() => PlayMethod::DirectStream,
            _ => PlayMethod::Transcode,
        };
        h.play_info(
            &self.media.id,
            PlayInfo {
                method,
                media_source_id: Some(self.source_id()),
                play_session_id: self.transcoding.then(|| self.play_session.clone()),
            },
        );
    }

    pub fn osd(&mut self, text: impl Into<String>) {
        self.osd = Some((text.into(), Instant::now()));
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.note = Some((text.into(), Instant::now()));
    }

    fn poke(&mut self) {
        self.hide.poke(Instant::now());
        self.ctl_shown = true;
    }

    pub fn pos(&self) -> f64 {
        self.state.as_ref().and_then(|s| s.position).unwrap_or(0.0)
    }

    pub fn dur(&self) -> Option<f64> {
        self.state.as_ref().and_then(|s| s.duration)
    }

    pub fn paused(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.paused)
    }

    pub fn frac(&self) -> f32 {
        match (self.pos(), self.dur()) {
            (p, Some(d)) if d > 0.0 => (p / d).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        }
    }

    pub fn num(&self, name: &str) -> Option<f64> {
        self.playback.as_ref()?.prop(name)?.as_f64()
    }

    fn flag(&self, name: &str) -> bool {
        self.playback
            .as_ref()
            .and_then(|p| p.prop(name))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    fn setp(&self, name: &str, val: &str) {
        if let Some(p) = &self.playback {
            let _ = p.set(name, val);
        }
    }

    /// The Theme Studio is docked: the bar stays up so it can be picked.
    fn studio_open(&self) -> bool {
        #[cfg(feature = "theme-engine")]
        return self.studio.is_some();
        #[cfg(not(feature = "theme-engine"))]
        false
    }

    pub fn controls_visible(&self) -> bool {
        // The external backend has no video under the controls: nothing to get out of the way of.
        !self.video
            || self.problem.is_some()
            || self.studio_open()
            || self.hide.visible(
                Instant::now(),
                // Anything open keeps the bar (and the menus anchored to it) up.
                self.bar_hover
                    || self.drag.is_some()
                    || self.pop.is_some()
                    || self.queue_open
                    || self.ctx_menu.is_some()
                    || self.resume_prompt.is_some()
                    || self.hdr_prompt.is_some(),
            )
    }

    pub fn next_index(&self) -> Option<usize> {
        pl::step_queue(self.qi, self.queue.len(), 1, self.set.repeat == Repeat::All)
    }

    pub fn prev_index(&self) -> Option<usize> {
        pl::step_queue(
            self.qi,
            self.queue.len(),
            -1,
            self.set.repeat == Repeat::All,
        )
    }

    /// An overlay (prompt, menu, skip chip, next card) is waiting for Enter; otherwise A plays / pauses.
    pub fn wants_enter(&self) -> bool {
        self.problem.is_some()
            || self.hdr_prompt.is_some()
            || self.resume_prompt.is_some()
            || self.ctx_menu.is_some()
            || self.queue_open
            || self.pop.is_some()
            || self.active_seg().is_some()
            || self.next_card().is_some()
    }

    pub fn active_seg(&self) -> Option<Seg> {
        if !self.loaded || self.resume_prompt.is_some() {
            return None;
        }
        pl::active_seg(&self.segs, self.pos())
    }

    /// Next-episode card: due, something to play, not cancelled. (title, seconds left if autoplaying)
    pub fn next_card(&self) -> Option<(String, Option<u32>)> {
        let n = self.queue.get(self.next_index()?)?;
        let since = self.next_since?;
        let left = self
            .ctx
            .prefs
            .autoplay_next
            .then(|| pl::countdown_left(since.elapsed().as_secs_f64()))
            .flatten();
        Some((n.title.clone(), left))
    }

    /// Can this item's favourite flag be changed (a server item, writes on)?
    pub fn can_favorite(&self) -> bool {
        self.ctx.hooks.as_ref().is_some_and(|h| h.can_write())
            && self
                .backend_of(&self.media)
                .is_some_and(|b| b.jellyfin().is_some())
    }

    // ---- track selection, names, external subtitles ----

    pub fn refresh_meta(&mut self) {
        let Some(p) = &self.playback else { return };
        self.tracks = p
            .prop("track-list")
            .map(|v| video::parse_tracks(&v))
            .unwrap_or_default();
        self.chapters = p
            .prop("chapter-list")
            .map(|v| video::parse_chapters(&v))
            .unwrap_or_default();
        if let Some(s) = self
            .play_item
            .as_ref()
            .and_then(|p| p.media_sources.first())
        {
            pl::name_tracks(&mut self.tracks, &s.media_streams);
        }
        let second = p.prop("secondary-sid").and_then(|v| v.as_i64());
        for t in &mut self.tracks {
            if second == Some(t.id) && t.kind == video::TrackKind::Sub {
                (t.secondary, t.selected) = (true, false);
            }
        }
    }

    /// Keys of this item's show (or movie) in the language memory.
    fn lang_keys(&self) -> Vec<String> {
        let m = &self.media;
        let ids = self
            .series_ids
            .as_ref()
            .filter(|(o, id, _)| *o == m.origin && Some(id) == m.series_id.as_ref())
            .map(|(_, _, ids)| ids);
        lang_memory::keys(m, ids)
    }

    fn choice(&self) -> TrackChoice {
        lang_memory::recall(&self.lang_keys())
    }

    fn remember(&self, f: impl FnOnce(&mut TrackChoice)) {
        lang_memory::remember(&self.lang_keys(), f);
    }

    /// Once per item, after the file is loaded: preferred audio/subtitle tracks.
    fn apply_tracks(&mut self) {
        if std::mem::replace(&mut self.applied, true) {
            return;
        }
        self.refresh_meta();
        let (prefs, mem) = (self.ctx.prefs.clone(), self.choice());
        if let Some(a) = pl::pick_audio(&self.tracks, &prefs, &mem) {
            self.setp("aid", &a.to_string());
            self.refresh_meta();
        }
        self.set.audio_original = mem.audio == Some(AudioPick::Original);
        self.apply_sub2_pos();
        self.subs_retry = (!self.apply_subs()).then(|| Instant::now() + Duration::from_secs(5));
    }

    /// Main and secondary subtitle from the memory and preferences. `false`: the memory names a
    /// language no track carries yet (external subtitles can arrive after the file loads), so
    /// the tick tries again for a few seconds.
    fn apply_subs(&mut self) -> bool {
        let (prefs, mem) = (self.ctx.prefs.clone(), self.choice());
        let audio = self
            .tracks
            .iter()
            .find(|t| t.kind == video::TrackKind::Audio && t.selected)
            .map(|t| t.lang.clone())
            .unwrap_or_default();
        let sid = pl::pick_sub(&self.tracks, &audio, &prefs, &mem);
        self.setp("sid", &track_arg(sid));
        let sid2 = pl::pick_sub2(&self.tracks, sid, &mem);
        self.setp("secondary-sid", &track_arg(sid2));
        self.refresh_meta();
        let missing = |p: &Option<SubPick>, id: i64| matches!(p, Some(SubPick::Lang(_))) && id == 0;
        !(missing(&mem.sub, sid) || missing(&mem.sub2, sid2))
    }

    /// Friendly names, plus the server's external/text subtitles (needed in transcode mode, where
    /// the HLS stream carries none) added once per load.
    fn name_and_ext_subs(&mut self) {
        self.refresh_meta();
        if !self.loaded || self.ext_added {
            return;
        }
        // A local file's subtitles are the ones downloaded for it (none come with the file here).
        if self
            .backend_of(&self.media)
            .is_some_and(|b| b.local().is_some())
        {
            self.ext_added = true;
            let paths = self
                .ctx
                .hooks
                .as_ref()
                .map(|h| h.subtitles(&self.media.id))
                .unwrap_or_default();
            for path in paths {
                self.add_sub_file(&path);
            }
            return;
        }
        let Some(src) = self
            .play_item
            .as_ref()
            .and_then(|p| p.media_sources.first())
            .cloned()
        else {
            return;
        };
        let Some((client, _)) = self.jf() else { return };
        self.ext_added = true;
        let (id, transcoding) = (self.media.id.clone(), self.transcoding);
        for s in src.media_streams.iter().filter(|s| s.kind == "Subtitle") {
            if !(s.is_external || transcoding && s.is_text_subtitle_stream) {
                continue;
            }
            let url = client.subtitle_url(&id, &src.id, s.index);
            let title = s
                .display_title
                .clone()
                .or_else(|| s.title.clone())
                .unwrap_or_else(|| "Subtitles".into());
            let lang = s.language.clone().unwrap_or_default();
            if let Some(p) = &self.playback {
                let _ = p.command(&["sub-add", &url, "auto", &title, &lang]);
            }
        }
        self.refresh_meta();
    }

    /// Add a downloaded subtitle (`en-123.srt`: language, then id) as an external track.
    pub fn add_sub_file(&mut self, path: &std::path::Path) {
        let lang = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split_once('-'))
            .map_or("", |(l, _)| l);
        if let (Some(p), Some(f)) = (&self.playback, path.to_str()) {
            let title = if lang.is_empty() {
                "Subtitles".to_string()
            } else {
                lang.to_uppercase()
            };
            let _ = p.command(&["sub-add", f, "auto", &title, lang]);
        }
        self.refresh_meta();
    }

    pub fn take_find_subs(&mut self) -> Option<Media> {
        std::mem::take(&mut self.find_subs_req).then(|| self.media.clone())
    }

    /// Select a track of `prop` (`sid`, `secondary-sid` or `aid`; 0 = off) and remember its language
    /// for this show. A track with no language tag changes the selection only.
    pub fn set_track(&mut self, prop: &str, id: i64, label: &str) {
        self.subs_retry = None; // the viewer chose: stop applying the memory
        self.setp(prop, &track_arg(id));
        self.refresh_meta();
        let (kind, second) = match prop {
            "aid" => (video::TrackKind::Audio, false),
            "secondary-sid" => (video::TrackKind::Sub, true),
            _ => (video::TrackKind::Sub, false),
        };
        let lang = self
            .tracks
            .iter()
            .find(|t| t.kind == kind && if second { t.secondary } else { t.selected })
            .map(|t| t.lang.clone())
            .filter(|l| !l.is_empty());
        match prop {
            "aid" => {
                self.set.audio_original = false;
                if let Some(l) = lang {
                    self.remember(|c| c.audio = Some(AudioPick::Lang(l)));
                }
            }
            _ if id == 0 || lang.is_some() => {
                let pick = if id == 0 {
                    SubPick::Off
                } else {
                    SubPick::of(lang)
                };
                self.remember(|c| {
                    if second {
                        c.sub2 = Some(pick)
                    } else {
                        c.sub = Some(pick)
                    }
                });
            }
            _ => {}
        }
        self.osd(label.to_string());
    }

    /// The secondary subtitle stays where the viewer put it (top by default).
    pub fn apply_sub2_pos(&self) {
        self.setp("secondary-sub-pos", &self.set.sub2_pos.to_string());
    }

    /// Id of the track shown as the main (`secondary` false) or secondary subtitle, 0 if none.
    fn shown_sub(&self, secondary: bool) -> i64 {
        self.tracks
            .iter()
            .find(|t| {
                t.kind == video::TrackKind::Sub && if secondary { t.secondary } else { t.selected }
            })
            .map_or(0, |t| t.id)
    }

    // ---- settings ----

    pub fn set_speed(&mut self, sp: f64) {
        if self.group {
            return self.osd("Speed follows the group");
        }
        self.setp("speed", &sp.to_string());
        self.set.speed = sp;
        self.osd(format!("Speed {}", video::fmt_speed(sp)));
    }

    pub fn set_repeat(&mut self, r: Repeat) {
        if self.group {
            return self.osd("Not available in a group");
        }
        self.setp("loop-file", if r == Repeat::One { "inf" } else { "no" });
        self.set.repeat = r;
        self.osd(r.label());
    }

    fn set_aspect(&mut self, a: Aspect) {
        for (k, v) in a.props() {
            self.setp(k, v);
        }
        self.set.aspect = a;
        self.osd(format!("Aspect {}", a.label()));
    }

    pub fn add_delay(&mut self, prop: &str, name: &str, d: f64) {
        let Some(p) = &self.playback else { return };
        let v = self.num(prop).unwrap_or(0.0) + d;
        let _ = p.command(&["add", prop, &d.to_string()]);
        self.osd(format!("{name} delay {}", video::fmt_delay(v)));
    }

    fn apply_pic(&mut self) {
        let s = self.set.clone();
        for (prop, v) in video::PIC_PROPS.iter().zip(s.pic.iter()) {
            self.setp(prop, &v.to_string());
        }
        self.setp("video-zoom", &s.zoom_log2().to_string());
        self.setp("video-pan-x", &(s.pan[0] as f64 * 0.05).to_string());
        self.setp("video-pan-y", &(s.pan[1] as f64 * 0.05).to_string());
        self.setp("video-rotate", &s.rotate.to_string());
    }

    fn apply_sub_style(&mut self) {
        for (k, v) in pl::SUB_STYLES[self.set.sub_style].props() {
            self.setp(k, &v);
        }
        self.apply_sub_pos();
    }

    /// Subtitles stay above the controls while those are showing.
    pub fn apply_sub_pos(&self) {
        let p = pl::sub_pos(
            self.set.sub_pos,
            BAR_H,
            self.hole.height,
            self.ctl_shown && self.video,
        );
        self.setp("sub-pos", &p.to_string());
    }

    pub fn refresh_devices(&mut self) {
        let Some(p) = &self.playback else { return };
        self.set.devices = p
            .prop("audio-device-list")
            .and_then(|v| v.as_array().cloned())
            .map(|a| {
                a.iter()
                    .filter_map(|d| {
                        Some((
                            d["name"].as_str()?.to_string(),
                            d["description"].as_str().unwrap_or("").to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.set.device = p
            .prop("audio-device")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
    }

    pub fn pop_act(&mut self, act: MenuAct) -> Task<Msg> {
        let back = |t: &mut Self, p: Pop| {
            let from = if p == Pop::Subs {
                Pop::Subs
            } else {
                Pop::Settings
            };
            t.pop = Some(from);
            t.pop_sel = video::settings_index(&t.tracks, &t.set, p);
        };
        let mut task = Task::none();
        match act {
            MenuAct::Sub(id) => {
                if id != 0 && id == self.shown_sub(true) {
                    self.set_track("secondary-sid", 0, "Secondary subtitles off");
                }
                let l = if id == 0 {
                    "Subtitles off"
                } else {
                    "Subtitles on"
                };
                self.set_track("sid", id, l);
                self.pop = None;
            }
            MenuAct::Sub2(id) => {
                if id != 0 && id == self.shown_sub(false) {
                    self.set_track("sid", 0, "Subtitles off");
                }
                let l = if id == 0 {
                    "Secondary subtitles off"
                } else {
                    "Secondary subtitles on"
                };
                self.set_track("secondary-sid", id, l);
                self.pop = None;
            }
            MenuAct::Sub2Pos(d) => {
                self.set.sub2_pos = (self.set.sub2_pos + d).clamp(0, 100);
                self.apply_sub2_pos();
            }
            MenuAct::AudioOriginal => {
                self.refresh_meta();
                if let Some(id) = pl::original_audio(&self.tracks) {
                    self.setp("aid", &id.to_string());
                    self.refresh_meta();
                }
                self.set.audio_original = true;
                self.remember(|c| c.audio = Some(AudioPick::Original));
                self.osd("Original audio");
                self.pop = None;
            }
            MenuAct::Nop => {}
            MenuAct::HdrSwitch => {
                self.pop = None;
                task = self.switch_playback(!self.popout, self.paused());
            }
            MenuAct::Audio(id) => {
                self.set_track("aid", id, "Audio track changed");
                self.pop = None;
            }
            MenuAct::Speed(s) => {
                self.set_speed(s);
                back(self, Pop::Speed);
            }
            MenuAct::Aspect(a) => {
                self.set_aspect(a);
                back(self, Pop::Aspect);
            }
            MenuAct::Repeat(r) => {
                self.set_repeat(r);
                back(self, Pop::Repeat);
            }
            MenuAct::Quality(q) => {
                task = self.set_quality(q);
                back(self, Pop::Quality);
            }
            MenuAct::SubStyle(i) => {
                self.set.sub_style = i;
                self.apply_sub_style();
                self.osd(format!("Subtitle style: {}", pl::SUB_STYLES[i].name));
            }
            MenuAct::SubPos(d) => {
                self.set.sub_pos = (self.set.sub_pos + d).clamp(0, 100);
                self.apply_sub_pos();
            }
            MenuAct::Pic(i, d) => {
                self.set.pic[i] = (self.set.pic[i] + d as i64 * 5).clamp(-100, 100);
                self.apply_pic();
            }
            MenuAct::Zoom(d) => {
                self.set.zoom = (self.set.zoom + d as i64).clamp(-5, 30);
                self.apply_pic();
            }
            MenuAct::Pan(i, d) => {
                self.set.pan[i] = (self.set.pan[i] + d as i64).clamp(-20, 20);
                self.apply_pic();
            }
            MenuAct::Rotate => {
                self.set.rotate = (self.set.rotate + 90) % 360;
                self.apply_pic();
            }
            MenuAct::Deint => {
                self.set.deint = !self.set.deint;
                self.setp("deinterlace", if self.set.deint { "yes" } else { "no" });
            }
            MenuAct::Tone(i) => {
                self.set.tone = i;
                self.setp("tone-mapping", video::TONES[i]);
                back(self, Pop::Tone);
            }
            MenuAct::Normalize => {
                self.set.normalize = !self.set.normalize;
                let v = if self.set.normalize {
                    "lavfi=[dynaudnorm=f=250:g=15]"
                } else {
                    ""
                };
                self.setp("af", v);
            }
            MenuAct::Downmix => {
                self.set.downmix = !self.set.downmix;
                let v = if self.set.downmix {
                    "stereo"
                } else {
                    "auto-safe"
                };
                self.setp("audio-channels", v);
            }
            MenuAct::Device(i) => {
                if let Some(d) = self.set.devices.get(i).map(|d| d.0.clone()) {
                    self.setp("audio-device", &d);
                    self.set.device = d;
                }
                back(self, Pop::Devices);
            }
            MenuAct::AbLoop if self.group => self.osd("Not available in a group"),
            MenuAct::AbLoop => {
                if let Some(p) = &self.playback {
                    let _ = p.command(&["ab-loop"]);
                }
                self.set.ab = (self.set.ab + 1) % 3;
                self.osd(["A-B loop off", "Loop start set", "Looping A-B"][self.set.ab as usize]);
            }
            MenuAct::ResetPicture => {
                self.set.pic = [0; 4];
                (self.set.zoom, self.set.pan, self.set.rotate) = (0, [0; 2], 0);
                self.apply_pic();
            }
            MenuAct::Open(p) => {
                if matches!(p, Pop::AudioFx | Pop::Devices) {
                    self.refresh_devices();
                }
                self.pop = Some(p);
                self.pop_sel = 0;
            }
            MenuAct::FindSubs => {
                self.pop = None;
                self.find_subs_req = true;
            }
            MenuAct::Info => {
                self.pop = None;
                self.stats = true;
            }
        }
        task
    }

    pub fn toggle_pop(&mut self, p: Pop) {
        if self.pop == Some(p) {
            self.pop = None;
        } else {
            self.refresh_meta();
            let items = video::items(p, &self.tracks, &self.set);
            self.pop_sel = items.iter().position(|i| i.on).unwrap_or(0);
            self.pop = Some(p);
        }
        self.queue_open = false;
        self.ctx_menu = None;
        self.poke();
    }

    /// Switch quality in place: reload at the current position.
    pub fn set_quality(&mut self, q: Quality) -> Task<Msg> {
        if q == self.set.quality {
            return Task::none();
        }
        self.set.quality = q;
        self.osd(format!("Quality: {}", q.label()));
        self.reload_at(self.pos())
    }

    fn reload_at(&mut self, pos: f64) -> Task<Msg> {
        self.transcoding = self.set.quality != Quality::Original;
        let h = self
            .play_item
            .as_ref()
            .and_then(|p| p.media_sources.first())
            .and_then(|s| {
                s.media_streams
                    .iter()
                    .find(|x| x.kind == "Video")
                    .and_then(|x| x.height)
                    .zip(Some(s.bitrate))
            });
        self.set.method = match h {
            Some((h, b)) => self.set.quality.method(Some(h), b),
            None => self.set.quality.method(None, None),
        }
        .label();
        self.report_info();
        self.seek_on_load = Some(pos);
        self.loaded = false;
        self.ext_added = false;
        self.applied = false;
        self.started = Instant::now();
        self.load_current()
    }

    // ---- actions ----

    pub fn toggle(&mut self) {
        if self.group {
            let r = if self.paused() {
                GroupReq::Unpause
            } else {
                GroupReq::Pause
            };
            return self.outbox.push(r);
        }
        if let Some(p) = &self.playback {
            let _ = p.toggle();
        }
    }

    pub fn seek(&mut self, secs: f64) {
        if self.group {
            return self.seek_to((self.pos() + secs).max(0.0));
        }
        if let Some(p) = &self.playback {
            let _ = p.seek_by(secs);
        }
    }

    pub fn seek_to(&mut self, secs: f64) {
        if self.group {
            let to = self.dur().map_or(secs, |d| secs.min(d)).max(0.0);
            return self.outbox.push(GroupReq::Seek(to));
        }
        if let Some(p) = &self.playback {
            let _ = p.seek_to(secs);
        }
    }

    /// The Stop key: ends playback for the whole group, or just closes when alone.
    pub fn stop(&mut self) -> Task<Msg> {
        if self.group {
            self.outbox.push(GroupReq::Stop);
            Task::none()
        } else {
            self.close(false)
        }
    }

    // ---- SyncPlay: what the group tells this player (never routed back to the group) ----

    pub fn take_outbox(&mut self) -> Vec<GroupReq> {
        std::mem::take(&mut self.outbox)
    }

    pub fn group_snap(&self) -> Option<(f64, bool)> {
        let st = self.state.as_ref().filter(|_| self.loaded)?;
        Some((st.position?, !st.paused))
    }

    pub fn group_pause(&mut self, at: f64) {
        self.setp("pause", "yes");
        self.group_seek(at);
    }

    pub fn group_unpause(&mut self) {
        self.setp("pause", "no");
    }

    pub fn group_seek(&mut self, to: f64) {
        // Never past the end: a position beyond the runtime makes the server schedule nonsense.
        let to = self.dur().map_or(to, |d| to.min(d)).max(0.0);
        if let Some(p) = &self.playback {
            let _ = p.seek_to(to);
        }
    }

    /// Playback speed nudge for drift correction (1.0 = normal); not the user's speed setting.
    pub fn group_speed(&mut self, f: f64) {
        self.setp("speed", &format!("{f:.3}"));
    }

    /// (mpv is seeking, mpv is waiting for cache data).
    pub fn group_flags(&self) -> (bool, bool) {
        (self.flag("seeking"), self.flag("paused-for-cache"))
    }

    /// Replaces the queue with the group's and loads `idx` at `start` seconds, paused.
    pub fn group_load(&mut self, queue: Vec<Media>, idx: usize, start: f64) -> Task<Msg> {
        let Some(m) = queue.get(idx).cloned() else {
            return Task::none();
        };
        self.queue = queue;
        self.qi = idx;
        self.group = true;
        self.seek_on_load = Some(start);
        self.queue_open = false;
        self.begin(m, false)
    }

    /// Same queue, new order or additions (the playing item stays where it is).
    pub fn group_queue(&mut self, queue: Vec<Media>, idx: usize) {
        self.queue = queue;
        self.qi = idx;
    }

    pub fn skip(&mut self, s: Seg) {
        let to = s.end.min(self.dur().map_or(s.end, |d| d - 0.5)).max(0.0);
        self.seek_to(to);
        self.osd(s.label().replace("Skip", "Skipped"));
    }

    pub fn resume_choice(&mut self, resume: bool) {
        if let Some(r) = self.resume_prompt.take() {
            if resume {
                self.seek_to(r);
            }
            self.setp("pause", "no");
        }
    }

    // ---- remote control (another Jellyfin app; docs/WRITES.md) ----

    /// Replace the queue and load `idx` from `start` seconds.
    pub fn remote_load(&mut self, queue: Vec<Media>, idx: usize, start: f64) -> Task<Msg> {
        let Some(m) = queue.get(idx).cloned() else {
            return Task::none();
        };
        self.report_stop();
        self.queue = queue;
        self.qi = idx;
        self.queue_open = false;
        self.seek_on_load = Some(start);
        self.setp("pause", "no"); // the last file may have ended paused
        self.begin(m, false)
    }

    /// Start position for a player that was just opened by a remote Play.
    pub fn remote_start(&mut self, start: f64) {
        self.seek_on_load = Some(start);
    }

    pub fn remote_pause(&mut self, want_paused: bool) {
        if self.paused() != want_paused {
            self.toggle();
        }
    }

    pub fn remote_volume(&mut self, pct: f64) {
        self.muted_from = None;
        if let Some(p) = &self.playback {
            let _ = p.set_volume(pct);
        }
        self.osd(format!("Volume {}%", pct.round()));
    }

    /// `Some(b)`: set muted to `b`; `None`: toggle.
    pub fn remote_mute(&mut self, want: Option<bool>) {
        let muted = self.state.as_ref().is_some_and(|s| s.volume <= 0.0);
        if want.is_none_or(|w| w != muted) {
            self.mute();
            let muted = !muted;
            self.osd(if muted { "Muted" } else { "Unmuted" });
        }
    }

    /// A Jellyfin stream index from the other app, mapped onto mpv's track list (when it can be).
    pub fn remote_stream(&mut self, index: i64, kind: video::TrackKind) {
        let streams = self
            .play_item
            .as_ref()
            .and_then(|p| p.media_sources.first())
            .map(|s| s.media_streams.as_slice())
            .unwrap_or_default();
        let Some(id) = remote::mpv_track(index, kind, streams, &self.tracks) else {
            return;
        };
        let prop = if kind == video::TrackKind::Audio {
            "aid"
        } else {
            "sid"
        };
        self.set_track(prop, id, "Changed from another device");
    }

    pub fn mute(&mut self) {
        let v = self.state.as_ref().map_or(100.0, |s| s.volume);
        let Some(p) = &self.playback else { return };
        match self.muted_from.take() {
            Some(prev) => drop(p.set_volume(prev)),
            None if v > 0.0 => {
                self.muted_from = Some(v);
                let _ = p.set_volume(0.0);
            }
            None => drop(p.set_volume(100.0)),
        }
    }

    pub fn jump(&mut self, i: usize) -> Task<Msg> {
        if self.group {
            self.outbox.push(GroupReq::SetIndex(i));
            return Task::none();
        }
        let Some(m) = self.queue.get(i).cloned() else {
            return Task::none();
        };
        self.report_stop();
        self.qi = i;
        self.queue_open = false;
        self.begin(m, false)
    }

    pub fn queue_shift(&mut self, i: usize, up: bool) {
        if self.group {
            return self.osd("The group's queue is shared");
        }
        if let Some(j) = pl::queue_move(&mut self.queue, &mut self.qi, i, up) {
            self.queue_sel = j;
        } else {
            self.osd("Already at the end");
        }
    }

    pub fn queue_drop(&mut self, i: usize) {
        if self.group {
            return self.osd("The group's queue is shared");
        }
        if pl::queue_remove(&mut self.queue, &mut self.qi, i) {
            self.queue_sel = self.queue_sel.min(self.queue.len().saturating_sub(1));
            self.osd("Removed from the queue");
        } else {
            self.osd("That one is playing");
        }
    }

    pub fn toggle_shuffle(&mut self) {
        if self.group {
            return self.osd("The group's queue is shared");
        }
        if self.shuffle {
            let (q, at) = pl::unshuffle(
                &self.queue,
                &self.unshuffled,
                |m| m.id.as_str(),
                &self.media.id,
            );
            (self.queue, self.qi) = (q, at);
            self.unshuffled.clear();
        } else {
            self.unshuffled = self.queue.clone();
            let mut s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
                | 1;
            pl::shuffle_after(&mut self.queue, self.qi, &mut |n| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s % n as u64) as usize
            });
        }
        self.shuffle = !self.shuffle;
        self.queue_sel = self.qi;
        self.osd(if self.shuffle {
            "Shuffle on"
        } else {
            "Shuffle off"
        });
    }

    /// Off, then all, then one.
    pub fn cycle_repeat(&mut self) {
        let r = match self.set.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        self.set_repeat(r);
    }

    pub fn step(&mut self, dir: i8) -> Task<Msg> {
        if self.group {
            let r = match (dir > 0, self.prev_index()) {
                (true, _) => Some(GroupReq::Next),
                (false, Some(_)) => Some(GroupReq::Prev),
                (false, None) => None,
            };
            match r {
                Some(r) => self.outbox.push(r),
                None => self.seek_to(0.0),
            }
            return Task::none();
        }
        let t = if dir > 0 {
            self.next_index()
        } else {
            self.prev_index()
        };
        match t {
            Some(i) => self.jump(i),
            None if dir < 0 => {
                self.seek_to(0.0);
                Task::none()
            }
            None => Task::none(),
        }
    }

    pub fn toggle_favorite(&mut self) {
        if !self.can_favorite() {
            return self.toast(if self.ctx.hooks.as_ref().is_some_and(|h| h.can_write()) {
                "Favourites are kept by the server only.".to_string()
            } else {
                format!("{}.", nocturne_core::sync::NEED_WRITES)
            });
        }
        let Some(h) = self.ctx.hooks.clone() else {
            return;
        };
        self.fav = !self.fav;
        h.set_favorite(&self.media.id, self.fav);
        self.osd(if self.fav {
            "Added to favourites"
        } else {
            "Removed from favourites"
        });
    }

    pub fn toggle_mini(&mut self) -> Task<Msg> {
        let Some(id) = self.win_id else {
            return Task::none();
        };
        let mut tasks = vec![];
        if self.fullscreen {
            self.fullscreen = false;
            tasks.push(window::set_mode(id, window::Mode::Windowed));
        }
        self.mini = !self.mini;
        self.pop = None;
        self.queue_open = false;
        self.ctx_menu = None;
        if self.mini {
            self.restore_size = Some(self.win);
            tasks.push(window::resize(id, Size::new(640.0, 360.0)));
        } else if let Some(s) = self.restore_size.take() {
            tasks.push(window::resize(id, s));
        }
        Task::batch(tasks)
    }

    pub fn toggle_fullscreen(&mut self) -> Task<Msg> {
        if self.popout {
            // The window with the picture is mpv's.
            if let Some(p) = &self.playback {
                let _ = p.command(&["cycle", "fullscreen"]);
            }
            return Task::none();
        }
        let Some(id) = self.win_id else {
            return Task::none();
        };
        self.fullscreen = !self.fullscreen;
        self.fs_from_video = self.fullscreen;
        window::set_mode(
            id,
            if self.fullscreen {
                window::Mode::Fullscreen
            } else {
                window::Mode::Windowed
            },
        )
    }

    pub fn screenshot(&mut self) {
        let Some(p) = &self.playback else { return };
        let dir = std::env::var_os("XDG_PICTURES_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| nocturne_session::paths::home().map(|h| h.join("Pictures")))
            .filter(|d| d.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let file = dir.join(format!("nocturne-{secs}.png"));
        let ok = p
            .command(&["screenshot-to-file", &file.to_string_lossy(), "video"])
            .is_ok();
        self.osd(if ok {
            "Screenshot saved"
        } else {
            "Screenshot failed"
        });
    }

    // ---- HDR (docs/HDR.md) ----

    pub fn take_hdr_choice(&mut self) -> Option<HdrMode> {
        self.hdr_choice.take()
    }

    fn prop_str(&self, name: &str) -> Option<String> {
        self.playback
            .as_ref()?
            .prop(name)?
            .as_str()
            .map(str::to_string)
    }

    /// Can the display path show HDR: the compositor's colour management plus an mpv that uses it.
    fn hdr_capable(&self) -> bool {
        // Debug builds: NOCTURNE_HDR_DISPLAY=yes|no stands in for the display (pop-out testing on an SDR display).
        #[cfg(debug_assertions)]
        if let Ok(v) = std::env::var("NOCTURNE_HDR_DISPLAY") {
            return v == "yes";
        }
        let ver = self.prop_str("mpv-version").unwrap_or_default();
        hdr::display_ok(self.compositor_cm, &ver)
    }

    /// Once per item, when mpv knows the picture's colour parameters: is it HDR, and what now.
    /// Only the in-window path (and its pop-out) is asked; the plain mpv window plays HDR itself.
    fn check_hdr(&mut self) -> Task<Msg> {
        if !self.embed || self.playback.is_none() || self.resume_prompt.is_some() {
            return Task::none();
        }
        if self.popout && !self.target_checked && self.pos() > 2.0 {
            self.target_checked = true;
            let target = self
                .prop_str("video-target-params/gamma")
                .unwrap_or_default();
            if self.hdr_checked && self.set.hdr_popout == Some(true) && !hdr::target_is_hdr(&target)
            {
                self.toast(
                    "The display did not report HDR, so the pop-out is showing SDR. Settings, HDR switches back to the window.",
                );
            }
        }
        if self.hdr_checked {
            return Task::none();
        }
        let gamma = self.prop_str("video-params/gamma");
        let prim = self.prop_str("video-params/primaries");
        if gamma.is_none() && prim.is_none() {
            // mpv has not decoded a frame yet; give up after a few seconds (treated as SDR).
            if self.hdr_since.get_or_insert(Instant::now()).elapsed() > Duration::from_secs(5) {
                self.hdr_checked = true;
            }
            return Task::none();
        }
        self.hdr_checked = true;
        if !hdr::is_hdr(
            gamma.as_deref().unwrap_or_default(),
            prim.as_deref().unwrap_or_default(),
        ) {
            // An SDR item after an HDR one: back to the window.
            return if self.popout {
                self.switch_playback(false, self.paused())
            } else {
                Task::none()
            };
        }
        self.set.hdr_popout = Some(self.popout);
        if self.popout {
            return Task::none();
        }
        let capable = self.hdr_capable();
        match hdr::decide(self.ctx.prefs.hdr, self.group, capable) {
            hdr::Decision::Window => Task::none(),
            hdr::Decision::Popout => self.switch_playback(true, self.paused()),
            hdr::Decision::Prompt { capable } => {
                let was_paused = self.paused();
                self.setp("pause", "yes");
                self.hdr_prompt = Some(hdr::Prompt::new(capable, was_paused));
                self.pop = None;
                self.queue_open = false;
                self.ctx_menu = None;
                Task::none()
            }
        }
    }

    /// The answer to the HDR prompt.
    pub fn hdr_choose(&mut self, popout: bool) -> Task<Msg> {
        let Some(p) = self.hdr_prompt.take() else {
            return Task::none();
        };
        if let Some(m) = p.remembered(popout) {
            self.ctx.prefs.hdr = m;
            self.hdr_choice = Some(m);
        }
        if popout {
            return self.switch_playback(true, p.was_paused);
        }
        if !p.was_paused {
            self.setp("pause", "no");
        }
        Task::none()
    }

    /// Moves the video between the window and the HDR pop-out (a separate mpv process driven over
    /// IPC), keeping position, volume and per-session settings. The same item and play session:
    /// progress reporting carries on untouched.
    fn switch_playback(&mut self, popout: bool, keep_paused: bool) -> Task<Msg> {
        if self.popout == popout || self.closed {
            return Task::none();
        }
        let pos = self.pos();
        self.hdr_prompt = None;
        // Release the old one first: the embedded player owns the window's video surface, the
        // external one a child process (and the pop-out's fullscreen window).
        self.playback = None;
        let made = if popout {
            self.new_popout()
        } else {
            self.new_window()
        };
        let pb = match made {
            Ok(p) => p,
            Err(e) => {
                // Put the other kind back so playback is not lost.
                eprintln!("video: switching failed ({e})");
                match if popout {
                    self.new_window()
                } else {
                    self.new_popout()
                } {
                    Ok(p) => {
                        self.playback = Some(p);
                        self.toast(format!("Could not switch: {e}"));
                        return self.reload_at(pos);
                    }
                    Err(_) => {
                        self.problem = Some(format!("Could not continue playback: {e}"));
                        return Task::none();
                    }
                }
            }
        };
        self.popout = popout;
        self.video = !popout;
        self.playback = Some(pb);
        self.switched = true;
        self.target_checked = false;
        self.set.hdr_popout = Some(popout);
        let vol = self.state.as_ref().map_or(self.volume, |s| s.volume);
        if let Some(p) = &self.playback {
            let _ = p.set_volume(vol);
        }
        if keep_paused {
            self.setp("pause", "yes");
        }
        self.osd(if popout {
            "HDR pop-out window"
        } else {
            "Back in the window (SDR)"
        });
        self.reload_at(pos)
    }

    fn new_popout(&self) -> Result<Playback, String> {
        if !nocturne_player::mpv_binary_available() {
            return Err(format!(
                "the HDR pop-out needs the mpv program. {}",
                nocturne_player::install_hint()
            ));
        }
        ExternalPlayer::new(Options {
            buffering: self.ctx.buffering,
            hdr_output: true,
            fullscreen: true,
            ..Default::default()
        })
        .map(Playback::External)
        .map_err(|e| e.to_string())
    }

    fn new_window(&self) -> Result<Playback, String> {
        if let Some(sf) = &self.surface {
            return ExternalPlayer::new(Options {
                buffering: self.ctx.buffering,
                wid: Some(sf.hwnd()),
                ..Default::default()
            })
            .map(Playback::External)
            .map_err(|e| e.to_string());
        }
        let Some(Handles::Wayland { display, surface }) = self.handles else {
            return Err("no window to play in".into());
        };
        self.new_embedded(display, surface)
            .map(Playback::Embedded)
            .map_err(|e| e.to_string())
    }

    /// Per-session settings a fresh playback object lacks (it starts from mpv's defaults).
    fn reapply(&mut self) {
        let s = self.set.clone();
        if (s.speed - 1.0).abs() > f64::EPSILON {
            self.setp("speed", &s.speed.to_string());
        }
        for (k, v) in s.aspect.props() {
            self.setp(k, v);
        }
        self.setp("tone-mapping", video::TONES[s.tone % video::TONES.len()]);
        if s.deint {
            self.setp("deinterlace", "yes");
        }
        if s.normalize {
            self.setp("af", "lavfi=[dynaudnorm=f=250:g=15]");
        }
        if s.downmix {
            self.setp("audio-channels", "stereo");
        }
        if s.repeat == Repeat::One {
            self.setp("loop-file", "inf");
        }
        self.apply_pic();
        self.apply_sub_style();
    }

    // ---- reporting ----

    /// The item reported as started is over: tell its source (server hooks gate themselves on the
    /// write settings; a local item's progress goes to the local library's own state).
    fn report_stop(&mut self) {
        if let Some((id, pos)) = self.reporter.stop() {
            self.report(Reported::Stopped, &id, pos, false);
        }
    }

    fn report(&mut self, what: Reported, id: &str, pos: f64, paused: bool) {
        let Some(m) = self
            .queue
            .iter()
            .chain([&self.media])
            .find(|m| m.id == id)
            .cloned()
        else {
            return;
        };
        let dur = if m.id == self.media.id {
            self.dur()
        } else {
            None
        };
        self.events.push(PlayEvent {
            dur: dur.or(m.duration_s),
            media: m.clone(),
            what,
            pos,
            paused,
        });
        match self.backend_of(&m) {
            Some(b @ Backend::Local(_)) => {
                bridge_spawn(async move {
                    let _ = b.set_progress_local(&m, pos.max(0.0) as u64).await;
                });
            }
            Some(Backend::Jellyfin(_)) => {
                if let Some(h) = &self.ctx.hooks {
                    match what {
                        Reported::Started => h.started(id, ticks(pos)),
                        Reported::Progress => h.progress(id, ticks(pos), paused),
                        Reported::Stopped => h.stopped(id, ticks(pos)),
                    }
                }
            }
            None => {}
        }
    }

    /// Ends the session: reports, drops the player, tells the owner.
    pub fn close(&mut self, failed: bool) -> Task<Msg> {
        if self.closed {
            return Task::none();
        }
        self.shutdown();
        let why = self.fail_why.take();
        self.outcome = Some(Outcome::Closed { failed, why });
        match (self.win_id, self.fs_from_video && self.fullscreen) {
            (Some(id), true) => window::set_mode(id, window::Mode::Windowed),
            _ => Task::none(),
        }
    }

    /// App exit / window close while playing: report and release the player, no outcome.
    pub fn shutdown(&mut self) {
        if !self.closed {
            self.report_stop();
        }
        self.closed = true;
        self.pending = None;
        self.playback = None; // dropping stops mpv / destroys the video surface
        self.mpris = None;
    }

    /// Direct play that never started falls back to a transcode once, calmly.
    fn fail(&mut self) -> Task<Msg> {
        if self.set.quality == Quality::Original && !self.transcoding && self.jf().is_some() {
            self.set.quality = Quality::P1080;
            self.toast(
                "Direct play isn't available for this file. Switching to a transcoded stream.",
            );
            self.transcoding = true;
            return self.reload_at(0.0);
        }
        self.close(true)
    }

    // ---- tick ----

    fn tick(&mut self) -> Task<Msg> {
        let shown = self.controls_visible();
        let mut tasks = vec![];
        if shown != self.ctl_shown {
            self.ctl_shown = shown;
        }
        if shown != self.sub_lifted {
            self.sub_lifted = shown;
            self.apply_sub_pos();
        }
        if self
            .osd
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(1600))
        {
            self.osd = None;
        }
        if self
            .note
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(4000))
        {
            self.note = None;
        }
        tasks.push(self.remote());
        let Some(p) = &self.playback else {
            return Task::batch(tasks);
        };
        let (mut failed, mut gone, mut loaded_now) = (false, false, false);
        let (mut tracks, mut input) = (false, vec![]);
        for ev in p.events().try_iter() {
            match ev {
                Event::Loaded => loaded_now = true,
                Event::Failed(m) => {
                    failed = true;
                    if m.contains("stopped responding") {
                        self.fail_why = Some("the video engine stopped responding");
                    }
                }
                Event::Closed => gone = true,
                Event::Tracks => tracks = true,
                Event::Input(s) => input.push(s),
                _ => {}
            }
        }
        if (failed || gone) && !self.loaded && self.surface.is_some() {
            tasks.push(self.fall_back_to_window());
            return Task::batch(tasks);
        }
        let st = p.state();
        self.volume = st.volume;
        failed |= !self.loaded && st.idle && self.started.elapsed() > Duration::from_secs(5);
        if logic::open_timed_out(self.loaded, self.started.elapsed()) {
            failed = true;
            self.fail_why = Some("no answer after 30 seconds");
        }
        self.ended = st.eof;
        self.state = Some(st);
        for s in input {
            tasks.push(self.surface_input(&s));
        }
        if loaded_now {
            self.on_loaded();
        } else if tracks && self.loaded {
            self.refresh_meta();
        }
        self.poll_extras();
        self.overlay_tick();
        if self.loaded {
            self.report_tick();
            tasks.push(self.run_rules());
            tasks.push(self.check_hdr());
        }
        self.publish();
        self.sync_surface();
        if failed {
            tasks.push(self.fail());
        } else if gone {
            tasks.push(self.close(false));
        }
        Task::batch(tasks)
    }

    /// Loading overlay: the fill follows mpv's cache, it leaves once playback runs, and a stall
    /// during playback brings the light variant back.
    fn overlay_tick(&mut self) {
        let now = self.now;
        let cache_wait = self.flag("paused-for-cache");
        // Debug builds only: `NOCTURNE_HOLD_SPLASH=0.55` pins the loading overlay at that fill (screenshots).
        #[cfg(debug_assertions)]
        if self.splash
            && let Some(f) = std::env::var("NOCTURNE_HOLD_SPLASH")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
        {
            self.prog.set(f, now);
            return;
        }
        if self.splash {
            if self.leaving.is_none() {
                let buffered = self.state.as_ref().map_or(0.0, |s| s.buffered);
                self.prog.set(logic::open_fill(self.loaded, buffered), now);
                let has_pos = self.state.as_ref().is_some_and(|s| s.position.is_some());
                if logic::started(
                    self.loaded,
                    has_pos,
                    cache_wait,
                    self.resume_prompt.is_some(),
                ) {
                    self.leaving = Some(now);
                }
            }
            if let Some(t) = self.leaving
                && buffering::fade_out(t, now, motion::reduced()) <= 0.0
            {
                self.splash = false;
                self.leaving = None;
            }
            return;
        }
        let stalled = self.loaded && cache_wait;
        match (stalled, self.stall_since) {
            (true, None) => {
                self.stall_since = Some(now);
                self.prog = Progress::new(now);
                self.born = now;
            }
            (false, Some(_)) => {
                self.stall_since = None;
                self.light = false;
            }
            _ => {}
        }
        if let Some(since) = self.stall_since {
            let pct = self.num("cache-buffering-state");
            self.prog.set(logic::stall_fill(pct), now);
            self.light = logic::stall_visible(Some(now.saturating_duration_since(since)));
        }
    }

    fn on_loaded(&mut self) {
        self.loaded = true;
        if std::mem::take(&mut self.switched) {
            self.reapply();
        }
        self.refresh_meta();
        self.apply_tracks();
        self.name_and_ext_subs();
        self.apply_sub_pos();
        if self.group {
            let t = self.seek_on_load.take().unwrap_or(0.0);
            if t > 0.5
                && let Some(p) = &self.playback
            {
                let _ = p.seek_to(t);
            }
            self.setp("pause", "yes");
            self.ready_due = Some(t);
        } else if let Some(t) = self.seek_on_load.take() {
            if t > 1.0
                && let Some(p) = &self.playback
            {
                let _ = p.seek_to(t);
            }
        } else {
            let dur = self.dur().or(self.media.duration_s);
            if self.first_load {
                let saved = self.ctx.resume.or(self.media.resume_s);
                self.resume_prompt = pl::resume_offer(saved, dur, self.media.played);
            }
            // A new file after an end-of-file pause would otherwise start paused.
            let hold = self.resume_prompt.is_some();
            self.setp("pause", if hold { "yes" } else { "no" });
        }
        self.first_load = false;
    }

    /// Progress reports: started once, every 10 s, and on pause/resume/seek.
    fn report_tick(&mut self) {
        // Nothing is reported until the resume question is answered: a "started at 0:00" report
        // would overwrite the saved position before the viewer chose to resume it.
        if self.resume_prompt.is_some() {
            return;
        }
        let Some(pos) = self.state.as_ref().and_then(|s| s.position) else {
            return;
        };
        let (id, paused) = (self.media.id.clone(), self.paused());
        if self.reporter.id().is_some_and(|r| r != id) {
            self.report_stop();
        }
        match self.reporter.tick(Instant::now(), &id, pos, paused) {
            Some(SyncTick::Started) => self.report(Reported::Started, &id, pos, false),
            Some(SyncTick::Progress) => self.report(Reported::Progress, &id, pos, paused),
            None => {}
        }
    }

    /// Per-tick rules: auto-skip, next-episode card, end of item.
    fn run_rules(&mut self) -> Task<Msg> {
        if self.resume_prompt.is_some() {
            return Task::none();
        }
        let pos = self.pos();
        let auto = self
            .segs
            .iter()
            .position(|s| pl::auto_skips(s) && pos >= s.start - 0.5 && pos < s.end - 1.0);
        if self.ctx.prefs.auto_skip_intro
            && !self.group
            && let Some(i) = auto
            && !self.skipped.contains(&i)
        {
            self.skipped.push(i);
            let s = self.segs[i];
            self.skip(s);
        }
        let credits = self
            .segs
            .iter()
            .find(|s| s.kind == pl::SegKind::Credits)
            .copied();
        let due =
            pl::next_card_due(pos, self.dur(), credits.as_ref()) && self.next_index().is_some();
        if due && !self.next_dismissed {
            if self.next_since.is_none() {
                self.next_since = Some(Instant::now());
            }
        } else {
            self.next_since = None;
        }
        let countdown_done = self.ctx.prefs.autoplay_next
            && !self.next_dismissed
            && self
                .next_since
                .is_some_and(|s| pl::countdown_left(s.elapsed().as_secs_f64()).is_none());
        let at_end = self.ended && self.set.repeat != Repeat::One;
        if self.group {
            // Everyone reaches the end; NextItem names the item, so the group advances once.
            if (countdown_done || at_end) && !self.eof_sent && self.next_index().is_some() {
                self.eof_sent = true;
                self.outbox.push(GroupReq::Next);
            }
            return Task::none();
        }
        if countdown_done || at_end {
            let wrap = self.set.repeat == Repeat::All;
            let go =
                countdown_done || wrap || (self.ctx.prefs.autoplay_next && !self.next_dismissed);
            if let (true, Some(i)) = (go, self.next_index()) {
                return self.jump(i);
            }
        }
        Task::none()
    }

    /// Pointer input mpv forwarded from the child window, handled like the UI's own events.
    fn surface_input(&mut self, s: &str) -> Task<Msg> {
        let Some((kind, p)) = logic::parse_input(s) else {
            return Task::none();
        };
        let (scale, origin) = (self.scale.max(0.1), self.hole.position());
        self.mouse = Point::new(origin.x + p.x / scale, origin.y + p.y / scale);
        self.poke();
        if kind != "move" {
            // The picture's window may have taken the keyboard focus with the click.
            if let Some(sf) = &self.surface {
                sf.focus_parent();
            }
        }
        match kind {
            "click" => self.act(Act::VideoPress),
            "dbl" => self.act(Act::VideoDouble),
            "right" => self.act(Act::VideoRight),
            _ => Task::none(),
        }
    }

    fn poll_extras(&mut self) {
        if let Some(t) = self.subs_retry {
            self.refresh_meta();
            if Instant::now() > t || self.apply_subs() {
                self.subs_retry = None;
            }
        }
        // End of the buffered range: position plus the demuxer's buffered-ahead seconds.
        self.cache_end = self
            .state
            .as_ref()
            .filter(|s| s.buffered > 0.0)
            .map(|s| s.position.unwrap_or(0.0) + s.buffered);
        // Hardware decoding check (QA): what mpv actually picked, logged once per item.
        if !self.hwdec_logged && self.loaded && self.pos() > 0.5 {
            self.hwdec_logged = true;
            if crate::timing::enabled() {
                let v = self
                    .playback
                    .as_ref()
                    .and_then(|p| p.prop("hwdec-current"))
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| "?".into());
                eprintln!("timing: hwdec-current  {v}");
            }
        }
        if self.stats && self.playback.is_some() {
            self.stats_rows = self.stats_snapshot();
            if let Some(d) = self.group_diff {
                self.stats_rows.push(("Group drift", format!("{d:+.0} ms")));
            }
        }
    }

    fn stats_snapshot(&self) -> Vec<(&'static str, String)> {
        let Some(p) = &self.playback else {
            return vec![];
        };
        let g = |n: &str| {
            p.prop(n)
                .map(|v| match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Number(n) => n.as_f64().map_or(n.to_string(), |f| {
                        if f.fract() == 0.0 {
                            format!("{f:.0}")
                        } else {
                            format!("{f:.2}")
                        }
                    }),
                    v => v.to_string(),
                })
                .unwrap_or_else(|| "-".into())
        };
        let kbps = |n: &str| {
            p.prop(n)
                .and_then(|v| v.as_f64())
                .map_or("-".into(), |b| format!("{:.0} kbps", b / 1000.0))
        };
        vec![
            ("Play method", self.set.method.into()),
            ("Container", g("file-format")),
            (
                "Video",
                format!(
                    "{} {}x{} @ {} fps",
                    g("video-format"),
                    g("width"),
                    g("height"),
                    g("container-fps")
                ),
            ),
            ("Video bitrate", kbps("video-bitrate")),
            (
                "Audio",
                format!(
                    "{} {}",
                    g("audio-codec-name"),
                    g("audio-params/hr-channels")
                ),
            ),
            ("Audio bitrate", kbps("audio-bitrate")),
            ("Hardware decode", g("hwdec-current")),
            (
                "Dropped frames",
                format!(
                    "{} (decoder {})",
                    g("frame-drop-count"),
                    g("decoder-frame-drop-count")
                ),
            ),
            ("Buffered", format!("{} s", g("demuxer-cache-duration"))),
        ]
    }

    // ---- MPRIS ----

    fn publish(&mut self) {
        let Some(m) = &mut self.mpris else { return };
        let st = self.state.clone().unwrap_or_default();
        let wrap = self.set.repeat == Repeat::All;
        let can_next = pl::step_queue(self.qi, self.queue.len(), 1, wrap).is_some();
        let can_prev = pl::step_queue(self.qi, self.queue.len(), -1, wrap).is_some();
        m.update(Snap {
            status: pl::mpris_status(!self.loaded || st.idle, st.paused),
            title: self.media.title.clone(),
            artist: self.media.series_name.clone().unwrap_or_default(),
            length_us: pl::micros(st.duration.unwrap_or(0.0)),
            art_url: self.art_url.clone(),
            volume: st.volume / 100.0,
            can_next,
            can_prev,
            pos_us: pl::micros(st.position.unwrap_or(0.0)),
        });
    }

    fn remote(&mut self) -> Task<Msg> {
        let cmds: Vec<Cmd> = self
            .mpris
            .as_ref()
            .map(|m| m.cmds.try_iter().collect())
            .unwrap_or_default();
        let mut tasks = vec![];
        for c in cmds {
            match c {
                Cmd::PlayPause => self.toggle(),
                Cmd::Play if self.paused() => self.toggle(),
                Cmd::Pause if !self.paused() => self.toggle(),
                Cmd::Stop => return self.stop(),
                Cmd::Next => tasks.push(self.step(1)),
                Cmd::Prev => tasks.push(self.step(-1)),
                Cmd::SeekBy(s) => self.seek(s),
                Cmd::SeekTo(s) => self.seek_to(s),
                _ => {}
            }
        }
        Task::batch(tasks)
    }

    // ---- trickplay ----

    /// Starts fetching the tile sheet covering `secs` (once).
    fn want_sheet(&mut self, secs: f64) -> Task<Msg> {
        let Some((src, info)) = self.trick.clone() else {
            return Task::none();
        };
        let Some(tile) = pl::tile_at(&info, secs) else {
            return Task::none();
        };
        let Some((c, _)) = self.jf() else {
            return Task::none();
        };
        if self.sheets.contains_key(&tile.sheet) {
            return Task::none();
        }
        self.sheets.insert(tile.sheet, Sheet::Loading);
        let (id, sheet, epoch) = (self.media.id.clone(), tile.sheet, self.epoch);
        task::run(
            async move { c.trickplay_tile(&id, info.width, sheet, &src).await.ok() },
            move |b| Msg::Sheet(epoch, sheet, b.map(Handle::from_bytes)),
        )
    }

    // ---- pointer input ----

    fn on_seek(&mut self, e: Ev) -> Task<Msg> {
        self.poke();
        match e {
            Ev::Hover(f) => {
                self.hover_frac = f;
                if let (Some(f), Some(d)) = (f, self.dur()) {
                    return self.want_sheet(f as f64 * d);
                }
            }
            Ev::Down(f) | Ev::Drag(f) => {
                self.drag = Some(Drag::Seek(f));
                self.hover_frac = Some(f);
                if let Some(d) = self.dur() {
                    return self.want_sheet(f as f64 * d);
                }
            }
            Ev::Up(f) => {
                if let (Some(Drag::Seek(_)), Some(d)) = (self.drag.take(), self.dur()) {
                    self.seek_to(f as f64 * d);
                }
            }
        }
        Task::none()
    }

    fn on_vol(&mut self, e: Ev) {
        self.poke();
        match e {
            Ev::Down(f) | Ev::Drag(f) => {
                self.drag = Some(Drag::Vol(f));
                self.muted_from = None;
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(f as f64 * 130.0);
                }
            }
            Ev::Up(_) => self.drag = None,
            Ev::Hover(_) => {}
        }
    }

    fn act(&mut self, a: Act) -> Task<Msg> {
        self.poke();
        match a {
            Act::Toggle => self.toggle(),
            Act::Seek(s) => self.seek(s),
            Act::Step(d) => return self.step(d),
            Act::Mute => self.mute(),
            Act::Pop(p) => self.toggle_pop(p),
            Act::Menu(m) => return self.pop_act(m),
            Act::Queue => {
                self.queue_open = !self.queue_open;
                self.queue_sel = self.qi;
                self.pop = None;
            }
            Act::Jump(i) => return self.jump(i),
            Act::Shuffle => self.toggle_shuffle(),
            Act::Repeat => self.cycle_repeat(),
            Act::Fav => self.toggle_favorite(),
            Act::Mini => return self.toggle_mini(),
            Act::Fullscreen => return self.toggle_fullscreen(),
            Act::Stop => return self.close(false),
            Act::Skip => {
                if let Some(s) = self.active_seg() {
                    self.skip(s);
                }
            }
            Act::NextPlay => {
                if let Some(i) = self.next_index() {
                    return self.jump(i);
                }
            }
            Act::NextCancel => {
                self.next_dismissed = true;
                self.next_since = None;
            }
            Act::Resume(r) => self.resume_choice(r),
            Act::Hdr(popout) => return self.hdr_choose(popout),
            Act::HdrRemember => {
                if let Some(p) = self.hdr_prompt.as_mut() {
                    p.remember ^= true;
                }
            }
            Act::ShowTotal => self.show_total = !self.show_total,
            Act::Ctx(c) => {
                self.ctx_menu = None;
                return match c {
                    CtxAct::Key(k) => self.do_key(k),
                    CtxAct::Pop(p) => {
                        self.toggle_pop(p);
                        Task::none()
                    }
                };
            }
            Act::BarHover(h) => {
                self.bar_hover = h;
                if !h {
                    self.hover_frac = None;
                }
            }
            Act::VideoPress => {
                self.ctx_menu = None;
                self.pop = None;
                self.queue_open = false;
                if self.resume_prompt.is_none() && self.hdr_prompt.is_none() {
                    self.toggle();
                }
            }
            Act::VideoDouble => {
                if self.resume_prompt.is_none() {
                    // The press already toggled pause; the double-click's second press toggled it
                    // back, so only the fullscreen is left to do.
                    return self.toggle_fullscreen();
                }
            }
            Act::VideoRight => {
                self.pop = None;
                self.queue_open = false;
                self.pop_sel = 0;
                self.ctx_menu = Some(self.mouse);
            }
        }
        Task::none()
    }
}

/// One report of the playing item as the player made it, handed to the root unchanged.
pub struct PlayEvent {
    pub media: Media,
    pub what: Reported,
    pub pos: f64,
    pub dur: Option<f64>,
    pub paused: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reported {
    Started,
    Progress,
    Stopped,
}

/// mpv's value for a track property: the id, or `no` for 0.
fn track_arg(id: i64) -> String {
    if id == 0 { "no".into() } else { id.to_string() }
}

fn bridge_spawn(fut: impl std::future::Future<Output = ()> + Send + 'static) {
    nocturne_core::bridge::spawn(fut);
}

impl Drop for Player {
    fn drop(&mut self) {
        // Belt and braces: the owner calls `shutdown` before the window goes, but a dropped page
        // must never leave a subsurface or an mpv child behind.
        self.shutdown();
    }
}
