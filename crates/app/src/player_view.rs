//! The player page as its own entity: playback backend, controls state, Jellyfin reads (streams,
//! segments, trickplay, queue), favourites hook and desktop integration. Rendering is in
//! `player_ui.rs`; pure rules are in `player_logic.rs` / `video.rs`. Nothing here is ever logged.
use crate::bridge;
use crate::player_logic::{self as pl, Method, Quality, Seg, TrackChoice};
use crate::player_mpris::{Cmd, Mpris, Snap};
use crate::source::{Kind, Media, sniff};
use crate::video::{
    self, Aspect, AutoHide, MenuAct, Playback, PlayerHooks, PlayerPrefs, Pop, Repeat, Settings,
};
use gpui::*;
use nocturne_jellyfin as jf;
use nocturne_player::{EmbeddedPlayer, Event, Options, Player};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::cell::Cell as StdCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Height of the two-row controls bar.
pub const BAR_H: f32 = 96.0;

/// What the caller supplies for one playback session.
#[derive(Clone)]
pub struct PlayerCtx {
    pub client: Arc<jf::Client>,
    pub user: String,
    pub hooks: Option<Arc<dyn PlayerHooks>>,
    pub prefs: PlayerPrefs,
}

pub enum PlayerEvent {
    /// Playback is over; the owner drops the view. `leave_fs`: we entered fullscreen for it.
    Closed { failed: bool, leave_fs: bool },
}

impl EventEmitter<PlayerEvent> for PlayerView {}

pub enum Sheet {
    Loading,
    Ready(Arc<Image>),
    Failed,
}

#[derive(Clone, Copy)]
pub enum Drag {
    Seek(f32),
    Vol(f32),
}

/// Track choices remembered per series/season for the rest of the app session.
static MEMORY: Mutex<Option<HashMap<String, TrackChoice>>> = Mutex::new(None);

fn mem_key(m: &Media) -> String {
    match &m.series_id {
        Some(s) => format!("{s}:{}", m.season_number.unwrap_or(0)),
        None => m.id.clone(),
    }
}

pub struct PlayerView {
    pub ctx: PlayerCtx,
    pub media: Media,
    pub queue: Vec<Media>,
    pub qi: usize,
    pub playback: Option<Playback>,
    /// Embedded player waiting for the next render (needs `&Window` for the raw handles).
    pub pending: Option<(String, String)>,
    pub embed: bool,
    pub video: bool,
    pub vbounds: Option<Bounds<Pixels>>,
    pub hide: AutoHide,
    pub ctl_shown: bool,
    /// Subtitle position last applied for this controls state.
    pub sub_lifted: bool,
    pub volume: f64,
    pub muted_from: Option<f64>,
    pub fs_from_video: bool,
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
    pub track_b: Rc<StdCell<Bounds<Pixels>>>,
    pub vol_b: Rc<StdCell<Bounds<Pixels>>>,
    pub drag: Option<Drag>,
    pub hover_frac: Option<f32>,
    pub bar_hover: bool,
    pub note: Option<(String, Instant)>,
    // per-item state
    pub state: Option<nocturne_player::State>,
    pub loaded: bool,
    pub ended: bool,
    pub started: Instant,
    pub seek_on_load: Option<f64>,
    pub first_load: bool,
    pub play_item: Option<jf::PlayItem>,
    pub segs: Vec<Seg>,
    pub skipped: Vec<usize>,
    pub next_since: Option<Instant>,
    pub next_dismissed: bool,
    pub resume_prompt: Option<f64>,
    pub trick: Option<(String, jf::TrickplayInfo)>,
    pub sheets: HashMap<u32, Sheet>,
    pub ext_added: bool,
    pub applied: bool,
    pub transcoding: bool,
    pub play_session: String,
    // panels
    pub queue_open: bool,
    pub queue_sel: usize,
    pub ctx_menu: Option<Point<Pixels>>,
    pub mini: bool,
    pub restore_size: Option<Size<Pixels>>,
    pub fav: bool,
    // reporting
    pub mpris: Option<Mpris>,
    pub art_url: String,
    pub closed: bool,
}

impl PlayerView {
    pub fn new(ctx: PlayerCtx, media: Media, cx: &mut Context<Self>) -> Self {
        let env = std::env::var("NOCTURNE_VIDEO").ok();
        let wl = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let embed = video::choose(env.as_deref(), wl) == video::Choice::Embedded;
        let now = Instant::now();
        let mut me = Self {
            ctx,
            media: media.clone(),
            queue: vec![media.clone()],
            qi: 0,
            playback: None,
            pending: None,
            embed,
            video: embed,
            vbounds: None,
            hide: AutoHide::new(now),
            ctl_shown: true,
            sub_lifted: false,
            volume: 100.0,
            muted_from: None,
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
            track_b: Rc::default(),
            vol_b: Rc::default(),
            drag: None,
            hover_frac: None,
            bar_hover: false,
            note: None,
            state: None,
            loaded: false,
            ended: false,
            started: now,
            seek_on_load: None,
            first_load: true,
            play_item: None,
            segs: vec![],
            skipped: vec![],
            next_since: None,
            next_dismissed: false,
            resume_prompt: None,
            trick: None,
            sheets: HashMap::new(),
            ext_added: false,
            applied: false,
            transcoding: false,
            play_session: String::new(),
            queue_open: false,
            queue_sel: 0,
            ctx_menu: None,
            mini: false,
            restore_size: None,
            fav: false,
            mpris: Some(Mpris::start()),
            art_url: String::new(),
            closed: false,
        };
        me.begin(media, cx);
        // Poll the player a few times a second.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if this.update(cx, |t, cx| t.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        me
    }

    /// Runs `fut` on the tokio bridge, then applies the result on the UI thread.
    fn run<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        fut: impl std::future::Future<Output = T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let h = bridge::spawn(fut);
        cx.spawn(async move |this, cx| {
            if let Ok(v) = h.await {
                this.update(cx, |t, cx| {
                    done(t, v, cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ---- loading an item ----

    fn header(&self) -> String {
        format!("Authorization: {}", self.ctx.client.auth_header())
    }

    /// Stream URL for the current quality (the item id doubles as media source id until the
    /// server's list arrives; it is the same for single-version items).
    fn url(&self) -> String {
        let src = self.source_id();
        match self.set.quality.transcode(&src, &self.play_session) {
            Some(t) => self.ctx.client.transcode_url(&self.media.id, &t),
            None => self.ctx.client.stream_url(&self.media.id),
        }
    }

    fn source_id(&self) -> String {
        self.play_item
            .as_ref()
            .and_then(|p| p.media_sources.first())
            .map_or_else(|| self.media.id.clone(), |s| s.id.clone())
    }

    /// Starts (or switches to) `m`: resets per-item state, loads, and fetches what the controls need.
    fn begin(&mut self, m: Media, cx: &mut Context<Self>) {
        self.media = m.clone();
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
        self.tracks.clear();
        self.chapters.clear();
        self.art_url.clear();
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
            .is_some_and(|h| h.is_favorite(&m.id));
        self.transcoding = self.set.quality != Quality::Original;
        self.set.method = self.set.quality.method(None, None).label();
        if self.set.quality != Quality::Original {
            self.set.method = Method::Transcode.label();
        }
        self.hide.poke(Instant::now());
        let (url, header) = (self.url(), self.header());
        if let Some(p) = &self.playback {
            if p.load(&url, Some(&header)).is_err() {
                return self.fail(cx);
            }
        } else if self.embed {
            self.pending = Some((url, header)); // created on the next render
        } else if !self.start_external(&url, &header) {
            return self.fail(cx);
        }
        // Jellyfin reads: streams + trickplay, segments, queue (episodes), poster for MPRIS.
        let (c, u, id) = (self.ctx.client.clone(), self.ctx.user.clone(), m.id.clone());
        let id2 = id.clone();
        self.run(
            cx,
            {
                let c = c.clone();
                async move { c.play_item(&u, &id).await.ok() }
            },
            move |t, r, cx| {
                if t.media.id == id2 {
                    t.on_play_item(r, cx);
                }
            },
        );
        let (c2, id) = (c.clone(), m.id.clone());
        let id2 = id.clone();
        self.run(
            cx,
            async move { c2.media_segments(&id).await },
            move |t, r, _| {
                if t.media.id == id2 {
                    t.segs = pl::segments(&r);
                }
            },
        );
        let (c3, id) = (c.clone(), m.id.clone());
        let id2 = id.clone();
        self.run(
            cx,
            async move {
                c3.image_bytes(&id, jf::ImageType::Primary, Some(300), None)
                    .await
                    .ok()
            },
            move |t, r, _| {
                if t.media.id == id2 {
                    t.art_url = r.and_then(|b| write_art(&b)).unwrap_or_default();
                }
            },
        );
        if self.queue.len() <= 1
            && m.kind == Kind::Episode
            && let Some(series) = m.series_id.clone()
        {
            let user = self.ctx.user.clone();
            self.run(
                cx,
                async move { c.episodes(&user, &series, None).await.ok() },
                move |t, r, _| {
                    let Some(items) = r else { return };
                    let q: Vec<Media> = items.items.iter().map(Media::from_item).collect();
                    if let Some(i) = q.iter().position(|e| e.id == t.media.id) {
                        t.queue = q;
                        t.qi = i;
                    }
                },
            );
        }
    }

    fn on_play_item(&mut self, r: Option<jf::PlayItem>, cx: &mut Context<Self>) {
        let Some(p) = r else { return };
        self.trick = p.trickplay_for(400);
        // The server's own source id and size decide what a quality cap really does.
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
        self.set.can_quality = true;
        self.name_and_ext_subs(cx);
    }

    fn start_external(&mut self, url: &str, header: &str) -> bool {
        self.video = false;
        let ok = Player::new(Options::default())
            .ok()
            .filter(|p| p.load(url, Some(header)).is_ok());
        self.playback = ok.map(Playback::External);
        self.playback.is_some()
    }

    /// Render-time: create the embedded player now that we have the window.
    pub fn start_embedded(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some((url, header)) = self.pending.take() else {
            return;
        };
        let handles = match (
            HasDisplayHandle::display_handle(window).map(|h| h.as_raw()),
            HasWindowHandle::window_handle(window).map(|h| h.as_raw()),
        ) {
            (Ok(RawDisplayHandle::Wayland(d)), Ok(RawWindowHandle::Wayland(s))) => {
                Some((d.display.as_ptr(), s.surface.as_ptr()))
            }
            _ => None,
        };
        let p = handles
            .and_then(|(d, s)| EmbeddedPlayer::new(d, s).ok())
            .filter(|p| p.load(&url, Some(&header)).is_ok());
        match p {
            Some(p) => {
                let _ = p.set_volume(self.volume);
                self.vbounds = None; // push bounds to the new surface on the next paint
                self.playback = Some(Playback::Embedded(Box::new(p)));
            }
            None => {
                eprintln!("video: embedded unavailable, using the external player");
                self.embed = false;
                if !self.start_external(&url, &header) {
                    self.fail(cx);
                }
            }
        }
    }

    // ---- state helpers ----

    pub fn osd(&mut self, text: impl Into<String>) {
        self.osd = Some((text.into(), Instant::now()));
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.note = Some((text.into(), Instant::now()));
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

    fn setp(&self, name: &str, val: &str) {
        if let Some(p) = &self.playback {
            let _ = p.set(name, val);
        }
    }

    pub fn controls_visible(&self) -> bool {
        // The external backend has no video under the controls: nothing to get out of the way of.
        !self.video
            || self
                .hide
                .visible(Instant::now(), self.bar_hover || self.drag.is_some())
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
    }

    fn choice(&self) -> TrackChoice {
        MEMORY
            .lock()
            .ok()
            .and_then(|m| m.as_ref()?.get(&mem_key(&self.media)).cloned())
            .unwrap_or_default()
    }

    fn remember(&self, f: impl FnOnce(&mut TrackChoice)) {
        if let Ok(mut m) = MEMORY.lock() {
            f(m.get_or_insert_with(HashMap::new)
                .entry(mem_key(&self.media))
                .or_default());
        }
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
        let audio = self
            .tracks
            .iter()
            .find(|t| t.kind == video::TrackKind::Audio && t.selected)
            .map(|t| t.lang.clone())
            .unwrap_or_default();
        let sid = pl::pick_sub(&self.tracks, &audio, &prefs, &mem);
        self.setp(
            "sid",
            &if sid == 0 {
                "no".into()
            } else {
                sid.to_string()
            },
        );
        self.refresh_meta();
    }

    /// Friendly names, plus the server's external/text subtitles (needed in transcode mode, where the
    /// HLS stream carries none) added once per load.
    fn name_and_ext_subs(&mut self, _cx: &mut Context<Self>) {
        self.refresh_meta();
        if !self.loaded || self.ext_added {
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
        self.ext_added = true;
        let (id, transcoding) = (self.media.id.clone(), self.transcoding);
        for s in src.media_streams.iter().filter(|s| s.kind == "Subtitle") {
            if !(s.is_external || transcoding && s.is_text_subtitle_stream) {
                continue;
            }
            let url = self.ctx.client.subtitle_url(&id, &src.id, s.index);
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

    pub fn set_track(&mut self, prop: &str, id: i64, label: &str) {
        self.setp(prop, &if id == 0 { "no".into() } else { id.to_string() });
        self.refresh_meta();
        let lang = self
            .tracks
            .iter()
            .find(|t| {
                t.selected
                    && t.kind
                        == if prop == "sid" {
                            video::TrackKind::Sub
                        } else {
                            video::TrackKind::Audio
                        }
            })
            .map(|t| t.lang.clone())
            .filter(|l| !l.is_empty());
        if prop == "sid" {
            self.remember(|c| c.sub = Some(lang));
        } else {
            self.remember(|c| c.audio_lang = lang);
        }
        self.osd(label.to_string());
    }

    // ---- settings ----

    pub fn set_speed(&mut self, sp: f64) {
        self.setp("speed", &sp.to_string());
        self.set.speed = sp;
        self.osd(format!("Speed {}", video::fmt_speed(sp)));
    }

    pub fn set_repeat(&mut self, r: Repeat) {
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
        let _ = p.command(&["add", prop, &d.to_string()]);
        let v = self.num(prop).unwrap_or(0.0);
        self.osd(format!("{name} delay {}", video::fmt_delay(v)));
    }

    fn apply_pic(&mut self) {
        let s = self.set.clone();
        for i in 0..4 {
            self.setp(video::PIC_PROPS[i], &s.pic[i].to_string());
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
        let h = self.vbounds.map_or(0.0, |b| f32::from(b.size.height));
        let p = pl::sub_pos(self.set.sub_pos, BAR_H, h, self.ctl_shown && self.video);
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

    pub fn pop_act(&mut self, act: MenuAct, cx: &mut Context<Self>) {
        let back = |t: &mut Self, p: Pop| {
            let from = if p == Pop::Subs {
                Pop::Subs
            } else {
                Pop::Settings
            };
            t.pop = Some(from);
            t.pop_sel = video::settings_index(&t.tracks, &t.set, p);
        };
        match act {
            MenuAct::Sub(id) => {
                self.set_track(
                    "sid",
                    id,
                    if id == 0 {
                        "Subtitles off"
                    } else {
                        "Subtitles on"
                    },
                );
                self.pop = None;
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
                self.set_quality(q, cx);
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
                self.setp(
                    "af",
                    if self.set.normalize {
                        "lavfi=[dynaudnorm=f=250:g=15]"
                    } else {
                        ""
                    },
                );
            }
            MenuAct::Downmix => {
                self.set.downmix = !self.set.downmix;
                self.setp(
                    "audio-channels",
                    if self.set.downmix {
                        "stereo"
                    } else {
                        "auto-safe"
                    },
                );
            }
            MenuAct::Device(i) => {
                if let Some(d) = self.set.devices.get(i).map(|d| d.0.clone()) {
                    self.setp("audio-device", &d);
                    self.set.device = d;
                }
                back(self, Pop::Devices);
            }
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
            MenuAct::Info => {
                self.pop = None;
                self.stats = true;
            }
        }
        cx.notify();
    }

    pub fn toggle_pop(&mut self, p: Pop, cx: &mut Context<Self>) {
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
        self.hide.poke(Instant::now());
        cx.notify();
    }

    /// Switch quality in place: reload at the current position.
    pub fn set_quality(&mut self, q: Quality, cx: &mut Context<Self>) {
        if q == self.set.quality {
            return;
        }
        self.set.quality = q;
        self.reload_at(self.pos(), cx);
        self.osd(format!("Quality: {}", q.label()));
    }

    fn reload_at(&mut self, pos: f64, cx: &mut Context<Self>) {
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
        self.seek_on_load = Some(pos);
        self.loaded = false;
        self.ext_added = false;
        self.applied = false;
        self.started = Instant::now();
        let (url, header) = (self.url(), self.header());
        if self
            .playback
            .as_ref()
            .is_none_or(|p| p.load(&url, Some(&header)).is_err())
        {
            self.fail(cx);
        }
    }

    // ---- actions ----

    pub fn toggle(&mut self) {
        if let Some(p) = &self.playback {
            let _ = p.toggle();
        }
    }

    pub fn seek(&mut self, secs: f64) {
        if let Some(p) = &self.playback {
            let _ = p.seek_by(secs);
        }
    }

    pub fn seek_to(&mut self, secs: f64) {
        if let Some(p) = &self.playback {
            let _ = p.seek_to(secs);
        }
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

    pub fn jump(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(m) = self.queue.get(i).cloned() else {
            return;
        };
        self.qi = i;
        self.queue_open = false;
        self.begin(m, cx);
        cx.notify();
    }

    pub fn step(&mut self, dir: i8, cx: &mut Context<Self>) {
        let t = if dir > 0 {
            self.next_index()
        } else {
            self.prev_index()
        };
        match t {
            Some(i) => self.jump(i, cx),
            None if dir < 0 => self.seek_to(0.0),
            None => {}
        }
    }

    pub fn toggle_favorite(&mut self) {
        let Some(h) = self.ctx.hooks.clone() else {
            return self.toast("Favourites need a signed-in account.");
        };
        self.fav = !self.fav;
        h.set_favorite(&self.media.id, self.fav);
        self.osd(if self.fav {
            "Added to favourites"
        } else {
            "Removed from favourites"
        });
    }

    pub fn toggle_mini(&mut self, window: &mut Window) {
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.mini = !self.mini;
        self.pop = None;
        self.queue_open = false;
        self.ctx_menu = None;
        if self.mini {
            self.restore_size = Some(window.viewport_size());
            window.resize(size(px(640.0), px(360.0)));
        } else if let Some(s) = self.restore_size.take() {
            window.resize(s);
        }
    }

    pub fn toggle_fullscreen(&mut self, window: &mut Window) {
        self.fs_from_video = !window.is_fullscreen();
        window.toggle_fullscreen();
    }

    pub fn screenshot(&mut self) {
        let Some(p) = &self.playback else { return };
        let dir = std::env::var_os("XDG_PICTURES_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Pictures")))
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

    // ---- reporting (hooks are non-blocking by contract) ----

    /// Ends the session: reports, drops the player, tells the owner.
    pub fn close(&mut self, failed: bool, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.playback = None; // dropping closes mpv / the video surface
        self.pending = None;
        self.mpris = None;
        cx.emit(PlayerEvent::Closed {
            failed,
            leave_fs: self.fs_from_video,
        });
        cx.notify();
    }

    /// App exit / window close while playing: report and release the player, no event.
    pub fn shutdown(&mut self) {
        self.closed = true;
        self.playback = None;
        self.mpris = None;
    }

    /// Direct play that never started falls back to a transcode once, calmly.
    fn fail(&mut self, cx: &mut Context<Self>) {
        if self.set.quality == Quality::Original && !self.transcoding {
            self.set.quality = Quality::P1080;
            self.toast(
                "Direct play isn't available for this file. Switching to a transcoded stream.",
            );
            self.reload_at(0.0, cx);
            self.transcoding = true;
            return;
        }
        self.close(true, cx);
    }

    // ---- tick ----

    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        let shown = self.controls_visible();
        if shown != self.ctl_shown {
            self.ctl_shown = shown;
            cx.notify();
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
            cx.notify();
        }
        if self
            .note
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_millis(4000))
        {
            self.note = None;
            cx.notify();
        }
        self.remote(cx);
        let Some(pl_) = &self.playback else { return };
        let (mut failed, mut gone, mut loaded_now) = (false, false, false);
        for ev in pl_.events().try_iter() {
            match ev {
                Event::Loaded => loaded_now = true,
                Event::Failed(_) => failed = true,
                Event::Closed => gone = true,
                _ => {}
            }
        }
        let st = pl_.state();
        self.volume = st.volume;
        failed |= !self.loaded && st.idle && self.started.elapsed() > Duration::from_secs(5);
        self.ended = st.eof;
        if self.state.as_ref() != Some(&st) {
            self.state = Some(st);
            cx.notify();
        }
        if loaded_now {
            self.on_loaded(cx);
        }
        self.poll_extras(cx);
        if self.loaded {
            self.run_rules(cx);
        }
        self.publish();
        if failed {
            self.fail(cx);
        } else if gone {
            self.close(false, cx);
        }
    }

    fn on_loaded(&mut self, cx: &mut Context<Self>) {
        self.loaded = true;
        self.refresh_meta();
        self.apply_tracks();
        self.name_and_ext_subs(cx);
        self.apply_sub_pos();
        if let Some(t) = self.seek_on_load.take() {
            if t > 1.0
                && let Some(p) = &self.playback
            {
                let _ = p.seek_to(t);
            }
        } else {
            let dur = self.dur().or(self.media.duration_s);
            if self.first_load {
                self.resume_prompt = pl::resume_offer(self.media.resume_s, dur, self.media.played);
            }
            // A new file after an end-of-file pause would otherwise start paused.
            self.setp(
                "pause",
                if self.resume_prompt.is_some() {
                    "yes"
                } else {
                    "no"
                },
            );
        }
        self.first_load = false;
        cx.notify();
    }

    /// Per-tick rules: progress reports, auto-skip, next-episode card, end of item.
    fn run_rules(&mut self, cx: &mut Context<Self>) {
        if self.resume_prompt.is_some() {
            return;
        }
        let pos = self.pos();
        let auto = self
            .segs
            .iter()
            .position(|s| pl::auto_skips(s) && pos >= s.start - 0.5 && pos < s.end - 1.0);
        if self.ctx.prefs.auto_skip_intro
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
                cx.notify();
            }
        } else if self.next_since.take().is_some() {
            cx.notify();
        }
        let countdown_done = self.ctx.prefs.autoplay_next
            && !self.next_dismissed
            && self
                .next_since
                .is_some_and(|s| pl::countdown_left(s.elapsed().as_secs_f64()).is_none());
        let at_end = self.ended && self.set.repeat != Repeat::One;
        if countdown_done || at_end {
            let wrap = self.set.repeat == Repeat::All;
            let go =
                countdown_done || wrap || (self.ctx.prefs.autoplay_next && !self.next_dismissed);
            if let (true, Some(i)) = (go, self.next_index()) {
                self.jump(i, cx);
            }
        }
    }

    fn poll_extras(&mut self, cx: &mut Context<Self>) {
        let cache = self.num("demuxer-cache-time");
        if cache != self.cache_end {
            self.cache_end = cache;
            cx.notify();
        }
        if self.stats && self.playback.is_some() {
            self.stats_rows = self.stats_snapshot();
            cx.notify();
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
        let (can_next, can_prev) = (
            pl::step_queue(self.qi, self.queue.len(), 1, self.set.repeat == Repeat::All).is_some(),
            pl::step_queue(
                self.qi,
                self.queue.len(),
                -1,
                self.set.repeat == Repeat::All,
            )
            .is_some(),
        );
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

    fn remote(&mut self, cx: &mut Context<Self>) {
        let cmds: Vec<Cmd> = self
            .mpris
            .as_ref()
            .map(|m| m.cmds.try_iter().collect())
            .unwrap_or_default();
        for c in cmds {
            match c {
                Cmd::PlayPause => self.toggle(),
                Cmd::Play if self.paused() => self.toggle(),
                Cmd::Pause if !self.paused() => self.toggle(),
                Cmd::Stop => return self.close(false, cx),
                Cmd::Next => self.step(1, cx),
                Cmd::Prev => self.step(-1, cx),
                Cmd::SeekBy(s) => self.seek(s),
                Cmd::SeekTo(s) => self.seek_to(s),
                _ => {}
            }
            cx.notify();
        }
    }

    // ---- trickplay ----

    /// Starts fetching the tile sheet covering `secs` (once).
    pub fn want_sheet(&mut self, secs: f64, cx: &mut Context<Self>) {
        let Some((src, info)) = self.trick.clone() else {
            return;
        };
        let Some(tile) = pl::tile_at(&info, secs) else {
            return;
        };
        if self.sheets.contains_key(&tile.sheet) {
            return;
        }
        self.sheets.insert(tile.sheet, Sheet::Loading);
        let (c, id, sheet, item) = (
            self.ctx.client.clone(),
            self.media.id.clone(),
            tile.sheet,
            self.media.id.clone(),
        );
        self.run(
            cx,
            async move { c.trickplay_tile(&id, info.width, sheet, &src).await.ok() },
            move |t, r, _| {
                if t.media.id != item {
                    return;
                }
                let s = match r.and_then(|b| sniff(&b).map(|f| Image::from_bytes(f, b))) {
                    Some(i) => Sheet::Ready(Arc::new(i)),
                    None => Sheet::Failed,
                };
                t.sheets.insert(sheet, s);
            },
        );
    }
}

/// Poster for the desktop's media widgets: a file in the runtime dir (D-Bus clients cannot send our auth header).
fn write_art(bytes: &[u8]) -> Option<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let p = std::path::Path::new(&dir).join(format!("nocturne-art-{}.img", std::process::id()));
    std::fs::write(&p, bytes).ok()?;
    Some(format!("file://{}", p.display()))
}
