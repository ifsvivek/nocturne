//! `EmbeddedPlayer`: libmpv (OpenGL render API, EGL) drawing into a `wl_subsurface` placed BELOW the UI
//! window's `wl_surface`. The window stays a normal transparent window and the UI draws controls on top.
//! Same method names / `State` / `Event` as the child-process [`crate::Player`].
//!
//! Threading: the UI (main) thread owns the Wayland proxies and calls `set_bounds`; one render
//! thread owns the EGL context + mpv render context; one event thread feeds `State`/`Event`.
//! Teardown order (see `Drop`) is load-bearing; see docs/video-embedding.md.

use crate::mpv_ffi::{Ev, Mpv, PropData, RenderCtx, UPDATE_FRAME};
use crate::{Buffering, Error, Event, Result, State, e};
use std::ffi::{c_char, c_int, c_void};
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::*};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
    wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_egl::WlEglSurface;
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

// ---- minimal EGL (links libEGL via glvnd; avoids a binding crate) ----
type Ptr = *mut c_void;
#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetPlatformDisplay(platform: u32, native: Ptr, attrs: *const isize) -> Ptr;
    fn eglInitialize(d: Ptr, maj: *mut c_int, min: *mut c_int) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglChooseConfig(
        d: Ptr,
        attrs: *const c_int,
        out: *mut Ptr,
        n: c_int,
        num: *mut c_int,
    ) -> u32;
    fn eglCreateContext(d: Ptr, cfg: Ptr, share: Ptr, attrs: *const c_int) -> Ptr;
    fn eglCreatePlatformWindowSurface(d: Ptr, cfg: Ptr, win: Ptr, attrs: *const isize) -> Ptr;
    fn eglMakeCurrent(d: Ptr, draw: Ptr, read: Ptr, ctx: Ptr) -> u32;
    fn eglSwapBuffers(d: Ptr, s: Ptr) -> u32;
    fn eglSwapInterval(d: Ptr, i: c_int) -> u32;
    fn eglDestroySurface(d: Ptr, s: Ptr) -> u32;
    fn eglDestroyContext(d: Ptr, c: Ptr) -> u32;
    fn eglGetProcAddress(name: *const c_char) -> Ptr;
    fn eglGetError() -> c_int;
}

struct St;
impl Dispatch<WlRegistry, GlobalListContents> for St {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(St: ignore WlCompositor);
delegate_noop!(St: ignore WlSubcompositor);
delegate_noop!(St: ignore WlSurface);
delegate_noop!(St: ignore WlSubsurface);
delegate_noop!(St: ignore WlRegion);
delegate_noop!(St: ignore WpViewporter);
delegate_noop!(St: ignore WpViewport);

/// How long the UI thread waits for one mpv call. A healthy core answers in microseconds; the core
/// thread can block for good inside a driver (a PipeWire `pw_thread_loop_stop` that never returns when
/// the audio server is gone), and every synchronous libmpv call then waits for it.
const CALL_WAIT: Duration = Duration::from_millis(300);
/// A call that has not returned after this long means the core is stuck: the page is told so.
const STUCK_AFTER: Duration = Duration::from_secs(15);
/// Teardown waits at most this long for each thread and for `mpv_terminate_destroy`; whatever is
/// still stuck is left behind (see [`crate::left_behind`]).
const TEARDOWN_WAIT: Duration = Duration::from_millis(1200);

type Job = Box<dyn FnOnce(&Mpv) + Send>;

struct Shared {
    stop: AtomicBool,
    /// Start of the mpv call the worker is running now (`None` when idle).
    since: Mutex<Option<Instant>>,
    stuck: AtomicBool,
    dirty: Mutex<bool>,
    cv: Condvar,
    w: AtomicI32, // physical pixels
    h: AtomicI32,
}
impl Shared {
    fn poke(&self) {
        *self.dirty.lock().unwrap() = true;
        self.cv.notify_one();
    }
}

struct SendBox<T>(T);
unsafe impl<T> Send for SendBox<T> {}

/// Does the compositor offer the colour-management protocol (`wp_color_manager_v1`)? Without it mpv
/// cannot ask for an HDR output (docs/HDR.md). `display`: the UI window's raw `wl_display*`; call on
/// the UI thread, like [`EmbeddedPlayer::new`]. Only the global list is read, nothing is bound.
pub fn colour_management_available(display: *mut c_void) -> bool {
    unsafe {
        let backend = Backend::from_foreign_display(display as *mut _);
        let conn = Connection::from_backend(backend);
        registry_queue_init::<St>(&conn).is_ok_and(|(globals, _)| {
            globals
                .contents()
                .with_list(|l| l.iter().any(|g| g.interface == "wp_color_manager_v1"))
        })
    }
}

/// Video rect in the parent window: logical position/size for the subsurface + viewport, and the
/// physical pixel size of the EGL buffer. Pure, so it is unit-tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub pw: i32,
    pub ph: i32,
}

pub fn geometry(x: f32, y: f32, w: f32, h: f32, scale: f32) -> Geometry {
    let (lw, lh) = (w.round().max(1.0) as i32, h.round().max(1.0) as i32);
    // Physical size derives from the rounded logical size so the viewport never stretches by a fraction.
    let phys = |l: i32| (l as f32 * scale).round().max(1.0) as i32;
    Geometry {
        x: x.round() as i32,
        y: y.round() as i32,
        w: lw,
        h: lh,
        pw: phys(lw),
        ph: phys(lh),
    }
}

pub struct EmbeddedPlayer {
    // Drop order is explicit in `Drop`; field order only matters for the leftovers.
    mpv: ManuallyDrop<Arc<Mpv>>,
    /// Every mpv call from the UI thread goes through this worker so a stuck core never blocks the UI.
    calls: Option<Sender<Job>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    events_thread: Option<JoinHandle<()>>,
    state: Arc<Mutex<State>>,
    rx: Receiver<Event>,
    conn: Connection,
    queue: EventQueue<St>,
    surface: WlSurface,
    sub: WlSubsurface,
    viewport: WpViewport,
    compositor: WlCompositor,
    qh: QueueHandle<St>,
}

unsafe extern "C" fn gpa(_: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { eglGetProcAddress(name) }
}

impl EmbeddedPlayer {
    /// `display`/`parent`: raw `wl_display*` / `wl_surface*` from the UI window's `HasDisplayHandle` /
    /// `HasWindowHandle`. Must be called on the UI (main) thread. Errors mean "embedding is not
    /// available here" (no viewporter/subcompositor, EGL failure, ...) and callers fall back to
    /// [`crate::Player`].
    pub fn new(display: *mut c_void, parent: *mut c_void) -> Result<Self> {
        Self::with_buffering(display, parent, Buffering::default())
    }

    /// Like [`Self::new`] with an explicit read-ahead policy.
    pub fn with_buffering(
        display: *mut c_void,
        parent: *mut c_void,
        buffering: Buffering,
    ) -> Result<Self> {
        // Before touching Wayland: without libmpv there is nothing to embed.
        crate::mpv_ffi::available().map_err(Error::Mpv)?;
        unsafe {
            // Wrap the UI toolkit's connection (foreign display: we never own/close it) with our own queue.
            let backend = Backend::from_foreign_display(display as *mut _);
            let conn = Connection::from_backend(backend);
            let (globals, queue) = registry_queue_init::<St>(&conn).map_err(e)?;
            let qh = queue.handle();
            let compositor: WlCompositor = globals.bind(&qh, 1..=6, ()).map_err(e)?;
            let subcomp: WlSubcompositor = globals.bind(&qh, 1..=1, ()).map_err(e)?;
            let viewporter: WpViewporter = globals.bind(&qh, 1..=1, ()).map_err(e)?;

            let pid = ObjectId::from_ptr(WlSurface::interface(), parent as *mut _).map_err(e)?;
            let parent_surface = WlSurface::from_id(&conn, pid).map_err(e)?;

            let surface = compositor.create_surface(&qh, ());
            let sub = subcomp.get_subsurface(&surface, &parent_surface, &qh, ());
            sub.place_below(&parent_surface);
            sub.set_desync(); // video updates must not wait for the UI's commits
            // Clicks fall through to the UI: empty input region.
            let empty = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&empty));
            empty.destroy();
            let viewport = viewporter.get_viewport(&surface, &qh, ());
            surface.commit();
            conn.flush().map_err(e)?;

            let (w0, h0) = (640, 360);
            let egl_win = WlEglSurface::new(surface.id(), w0, h0).map_err(e)?;

            let shared = Arc::new(Shared {
                stop: AtomicBool::new(false),
                since: Mutex::new(None),
                stuck: AtomicBool::new(false),
                dirty: Mutex::new(true),
                cv: Condvar::new(),
                w: AtomicI32::new(w0),
                h: AtomicI32::new(h0),
            });

            // RULE: in-process mpv may ONLY use the OpenGL render API (`vo=libmpv`). Never
            // `gpu`/`gpu-next` (Vulkan context) and never `hwdec=vulkan` here: a second VkInstance
            // next to the UI's Vulkan renderer SIGSEGVs on NVIDIA at stop/quit. See docs/video-embedding.md.
            let mut opts: Vec<(&str, String)> = vec![
                ("vo", "libmpv".into()),
                ("hwdec", "auto-safe".into()),
                ("keep-open", "yes".into()),
                ("terminal", "no".into()),
                ("input-default-bindings", "no".into()),
                ("osc", "no".into()),
            ];
            opts.extend(buffering.mpv_options());
            let opts: Vec<(&str, &str)> = opts.iter().map(|(k, v)| (*k, v.as_str())).collect();
            // Opaque black bars (the default would be alpha 0). Names differ by libmpv version:
            // >= 0.40 `background=color` + `background-color`; older takes the colour as the
            // value of `background`, and has no `background-color`. Rejections are not fatal.
            let background: [&[(&str, &str)]; 2] = [
                &[("background", "color"), ("background", "#FF000000")],
                &[("background-color", "#FF000000")],
            ];
            let mpv = Mpv::new_with(&opts, &background)
                .map_err(|er| Error::Mpv(format!("init: {er}")))?;
            let mpv = Arc::new(mpv);
            for (i, (n, flag)) in [
                ("pause", true),
                ("time-pos", false),
                ("duration", false),
                ("volume", false),
                ("eof-reached", true),
                ("idle-active", true),
                ("demuxer-cache-duration", false),
            ]
            .into_iter()
            .enumerate()
            {
                let id = i as u64 + 1;
                let r = if flag {
                    mpv.observe_flag(n, id)
                } else {
                    mpv.observe_double(n, id)
                };
                r.map_err(e)?;
            }

            let (tx, rx) = channel::<std::result::Result<(), String>>();
            let (mpv2, sh2) = (mpv.clone(), shared.clone());
            let payload = SendBox((display, egl_win));
            let thread = std::thread::Builder::new()
                .name("video-render".into())
                .spawn(move || {
                    let payload = payload; // capture the whole Send wrapper, not its raw-pointer fields
                    let SendBox((display, egl_win)) = payload;
                    render_thread(display, egl_win, mpv2, sh2, tx)
                })
                .map_err(e)?;
            // Ready only once the render context exists: `loadfile` before that is fatal for vo=libmpv.
            if let Err(m) = rx.recv().map_err(e)? {
                let _ = thread.join();
                return Err(Error::Mpv(m));
            }

            let state = Arc::new(Mutex::new(State::default()));
            let (etx, erx) = channel();
            let (mpv3, st3, sh3) = (mpv.clone(), state.clone(), shared.clone());
            let events_thread = std::thread::Builder::new()
                .name("video-events".into())
                .spawn(move || event_loop(mpv3, st3, etx, sh3))
                .map_err(e)?;
            let (ctx, crx) = channel::<Job>();
            let (mpv4, sh4) = (mpv.clone(), shared.clone());
            // Debug builds only (QA): `NOCTURNE_MPV_STALL_AFTER_MS=n` makes the core look stuck from n ms on.
            #[cfg(debug_assertions)]
            let stall = std::env::var("NOCTURNE_MPV_STALL_AFTER_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|ms| Instant::now() + Duration::from_millis(ms));
            std::thread::Builder::new()
                .name("video-calls".into())
                .spawn(move || {
                    for job in crx {
                        #[cfg(debug_assertions)]
                        if stall.is_some_and(|t| Instant::now() >= t) {
                            loop {
                                std::thread::park();
                            }
                        }
                        job(&mpv4);
                        *sh4.since.lock().unwrap() = None;
                    }
                })
                .map_err(e)?;
            Ok(Self {
                mpv: ManuallyDrop::new(mpv),
                calls: Some(ctx),
                shared,
                thread: Some(thread),
                events_thread: Some(events_thread),
                state,
                rx: erx,
                conn,
                queue,
                surface,
                sub,
                viewport,
                compositor,
                qh,
            })
        }
    }

    /// Logical position/size inside the parent window (logical pixels as f32) plus scale factor.
    /// Position applies at the parent's next commit, so ask the UI for a redraw after calling.
    pub fn set_bounds(&mut self, x: f32, y: f32, w: f32, h: f32, scale: f32) {
        let g = geometry(x, y, w, h, scale);
        self.sub.set_position(g.x, g.y);
        self.viewport.set_destination(g.w, g.h);
        // The whole video surface is opaque: lets the compositor skip blending/blur behind it.
        let region = self.compositor.create_region(&self.qh, ());
        region.add(0, 0, g.w, g.h);
        self.surface.set_opaque_region(Some(&region));
        region.destroy();
        let (ow, oh) = (
            self.shared.w.swap(g.pw, SeqCst),
            self.shared.h.swap(g.ph, SeqCst),
        );
        if (ow, oh) != (g.pw, g.ph) {
            self.shared.poke();
        }
        let _ = self.conn.flush();
        let mut st = St;
        let _ = self.queue.dispatch_pending(&mut st); // drain our (ignored) events
    }

    /// Non-blocking event source: `try_recv` / `try_iter`.
    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }

    /// Runs `f` on the call worker and waits up to [`CALL_WAIT`]. While an earlier call is still
    /// running (a busy or stuck core) this fails at once instead of queueing behind it.
    fn call<T: Send + 'static>(&self, f: impl FnOnce(&Mpv) -> T + Send + 'static) -> Result<T> {
        let busy = || e("the video engine is busy");
        let Some(tx) = &self.calls else {
            return Err(busy());
        };
        {
            let mut since = self.shared.since.lock().unwrap();
            if since.is_some() {
                return Err(busy());
            }
            *since = Some(Instant::now());
        }
        let (rtx, rrx) = channel();
        if tx
            .send(Box::new(move |m| {
                let _ = rtx.send(f(m));
            }))
            .is_err()
        {
            return Err(busy());
        }
        rrx.recv_timeout(CALL_WAIT).map_err(|_| busy())
    }

    fn cmd(&self, name: &str, args: &[&str]) -> Result<()> {
        let (name, args): (String, Vec<String>) =
            (name.into(), args.iter().map(|a| a.to_string()).collect());
        self.call(move |m| {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            m.command(&name, &a)
        })?
        .map_err(e)
    }

    /// Same contract as [`crate::Player::load`]: each of `headers` is a full header line, appended to
    /// the `http-header-fields` list (`change-list` appends verbatim, no splitting on ',').
    pub fn load(&self, url: &str, headers: &[&str]) -> Result<()> {
        let (url, headers): (String, Vec<String>) =
            (url.into(), headers.iter().map(|h| h.to_string()).collect());
        self.call(move |m| {
            m.command("change-list", &["http-header-fields", "clr", ""])?;
            for h in &headers {
                m.command("change-list", &["http-header-fields", "append", h])?;
            }
            m.command("loadfile", &[&url, "replace"])
        })?
        .map_err(e)
    }

    pub fn play(&self) -> Result<()> {
        self.set_paused(false)
    }

    pub fn pause(&self) -> Result<()> {
        self.set_paused(true)
    }

    pub fn toggle(&self) -> Result<()> {
        self.set_paused(!self.state().paused)
    }

    pub fn seek_to(&self, secs: f64) -> Result<()> {
        self.cmd("seek", &[&secs.to_string(), "absolute"])
    }

    pub fn seek_by(&self, secs: f64) -> Result<()> {
        self.cmd("seek", &[&secs.to_string(), "relative"])
    }

    /// 0..=130 (mpv's range); values outside are clamped.
    pub fn set_volume(&self, vol: f64) -> Result<()> {
        let v = vol.clamp(0.0, 130.0);
        self.call(move |m| m.set_double("volume", v))?.map_err(e)
    }

    pub fn stop(&self) -> Result<()> {
        self.cmd("stop", &[])
    }

    /// Any mpv command, e.g. `command(&["set", "speed", "1.5"])`.
    pub fn command(&self, args: &[&str]) -> Result<()> {
        let (name, rest) = args.split_first().ok_or_else(|| e("empty command"))?;
        self.cmd(name, rest)
    }

    /// Reads one mpv property as JSON, same shape as the child-process player. `None` when unavailable
    /// (or when the core is busy: the caller asks again on its next tick).
    pub fn prop(&self, name: &str) -> Option<serde_json::Value> {
        let name = name.to_string();
        let s = self.call(move |m| m.get_string(&name)).ok()?.ok()?;
        Some(match s.as_str() {
            "yes" => true.into(),
            "no" => false.into(),
            _ => serde_json::from_str(&s).unwrap_or(s.into()),
        })
    }

    /// Cheap, non-blocking: a snapshot maintained by the event thread.
    pub fn state(&self) -> State {
        crate::lock(&self.state).clone()
    }

    fn set_paused(&self, p: bool) -> Result<()> {
        self.call(move |m| m.set_flag("pause", p))?.map_err(e)
    }
}

impl Drop for EmbeddedPlayer {
    fn drop(&mut self) {
        // Nothing here may wait on mpv for long: a stuck core would freeze the app and make it ignore
        // SIGTERM. Whatever does not finish in time is left behind and the process ends without it.
        // `stop` first: frees the decoder/hwdec while the GL context and window are still alive. Without
        // it, freeing the render context hangs intermittently when the compositor is closing the window.
        let stopped = self.cmd("stop", &[]).is_ok();
        self.shared.stop.store(true, SeqCst);
        self.shared.poke();
        self.calls = None; // the worker ends after its current call
        let wait = if stopped {
            TEARDOWN_WAIT
        } else {
            TEARDOWN_WAIT / 4
        };
        // 1. the render thread frees the mpv render ctx, EGL surface/ctx and wl_egl_window (GL context
        //    still current while mpv frees its objects); the event thread just ends.
        let mut clean = true;
        let deadline = Instant::now() + wait;
        for t in [self.thread.take(), self.events_thread.take()]
            .into_iter()
            .flatten()
        {
            while !t.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if t.is_finished() {
                let _ = t.join();
            } else {
                clean = false;
            }
        }
        // 2. only now may the Wayland objects go (viewport, subsurface, surface), parent untouched.
        //    A render thread still stuck in mpv may still use them: leave them alone then.
        if clean {
            self.viewport.destroy();
            self.sub.destroy();
            self.surface.destroy();
            let _ = self.conn.flush();
        }
        // 3. the last `Arc<Mpv>` runs `mpv_terminate_destroy`, which waits for the core: do it on a
        //    thread of its own and wait for it only a little.
        let mpv = unsafe { ManuallyDrop::take(&mut self.mpv) };
        let (done_tx, done_rx) = channel::<()>();
        let reaper = std::thread::Builder::new()
            .name("mpv-reaper".into())
            .spawn(move || {
                drop(mpv);
                let _ = done_tx.send(());
            });
        let reaped = reaper.is_ok() && done_rx.recv_timeout(wait).is_ok();
        if !(clean && reaped) || self.shared.since.lock().unwrap().is_some() {
            crate::mark_left_behind();
        }
    }
}

/// Mirrors `Player`'s IPC reader: property changes -> `State`, edges -> `Event`.
fn event_loop(mpv: Arc<Mpv>, state: Arc<Mutex<State>>, tx: Sender<Event>, sh: Arc<Shared>) {
    while !sh.stop.load(SeqCst) {
        let since = *sh.since.lock().unwrap();
        if since.is_some_and(|t| t.elapsed() > STUCK_AFTER) && !sh.stuck.swap(true, SeqCst) {
            let _ = tx.send(Event::Failed("the video engine stopped responding".into()));
        }
        let out = match mpv.wait_event(0.05) {
            Ev::None => continue,
            Ev::Failed(er) => Some(Event::Failed(er)),
            Ev::FileLoaded => Some(Event::Loaded),
            Ev::Shutdown => break,
            Ev::Property(name, change) => {
                let mut s = crate::lock(&state);
                let flag = |d: &PropData, def| match d {
                    PropData::Flag(b) => *b,
                    _ => def,
                };
                let num = |d: &PropData| match d {
                    PropData::Double(v) => Some(*v),
                    _ => None,
                };
                match name.as_str() {
                    "pause" => {
                        let p = flag(&change, false);
                        (std::mem::replace(&mut s.paused, p) != p).then_some(Event::Paused(p))
                    }
                    "eof-reached" => {
                        let b = flag(&change, false);
                        let rose = b && !s.eof;
                        s.eof = b;
                        rose.then_some(Event::Eof)
                    }
                    "idle-active" => {
                        s.idle = flag(&change, true);
                        None
                    }
                    "time-pos" => {
                        s.position = num(&change);
                        None
                    }
                    "duration" => {
                        s.duration = num(&change);
                        None
                    }
                    "volume" => {
                        s.volume = num(&change).unwrap_or(100.0);
                        None
                    }
                    "demuxer-cache-duration" => {
                        s.buffered = num(&change).unwrap_or(0.0);
                        None
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if out.is_some_and(|ev| tx.send(ev).is_err()) {
            return;
        }
    }
    crate::lock(&state).idle = true;
    let _ = tx.send(Event::Closed);
}

fn render_thread(
    display: *mut c_void,
    egl_win: WlEglSurface,
    mpv: Arc<Mpv>,
    sh: Arc<Shared>,
    ready: Sender<std::result::Result<(), String>>,
) {
    unsafe {
        let dpy = eglGetPlatformDisplay(0x31D8, display, null());
        let (mut ma, mut mi) = (0, 0);
        if dpy.is_null() || eglInitialize(dpy, &mut ma, &mut mi) == 0 {
            return drop(ready.send(Err(format!("eglInitialize failed 0x{:x}", eglGetError()))));
        }
        eglBindAPI(0x30A2); // OPENGL_API
        let cfg_attr = [
            0x3033, 4, 0x3040, 8, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3038,
        ];
        let (mut cfg, mut n) = (null_mut(), 0);
        if eglChooseConfig(dpy, cfg_attr.as_ptr(), &mut cfg, 1, &mut n) == 0 || n == 0 {
            return drop(ready.send(Err("no EGL config".into())));
        }
        let ctx_attr = [0x3098, 3, 0x30FB, 3, 0x30FD, 1, 0x3038];
        let ctx = eglCreateContext(dpy, cfg, null_mut(), ctx_attr.as_ptr());
        let surf = eglCreatePlatformWindowSurface(dpy, cfg, egl_win.ptr() as Ptr, null());
        if ctx.is_null() || surf.is_null() || eglMakeCurrent(dpy, surf, surf, ctx) == 0 {
            return drop(ready.send(Err(format!("EGL ctx/surface failed 0x{:x}", eglGetError()))));
        }
        eglSwapInterval(dpy, 0);
        {
            let mut rc = match RenderCtx::new(&mpv, gpa, display as *const c_void) {
                Ok(rc) => rc,
                Err(e) => return drop(ready.send(Err(format!("mpv render ctx: {e}")))),
            };
            // Ready only now: `loadfile` before a render context exists is fatal for vo=libmpv.
            let _ = ready.send(Ok(()));
            let sh2 = sh.clone();
            rc.set_update_callback(move || sh2.poke());
            let clear_color: unsafe extern "C" fn(f32, f32, f32, f32) =
                std::mem::transmute(eglGetProcAddress(c"glClearColor".as_ptr()));
            let clear: unsafe extern "C" fn(u32) =
                std::mem::transmute(eglGetProcAddress(c"glClear".as_ptr()));
            let (mut cw, mut ch) = (0, 0);
            while !sh.stop.load(SeqCst) {
                {
                    let mut d = sh.dirty.lock().unwrap();
                    if !*d {
                        d = sh.cv.wait_timeout(d, Duration::from_millis(250)).unwrap().0;
                    }
                    *d = false;
                }
                if sh.stop.load(SeqCst) {
                    break;
                }
                let flags = rc.update();
                let (w, h) = (sh.w.load(SeqCst), sh.h.load(SeqCst));
                let resized = (w, h) != (cw, ch);
                if resized {
                    egl_win.resize(w, h, 0, 0);
                    (cw, ch) = (w, h);
                }
                if flags & UPDATE_FRAME != 0 || resized {
                    clear_color(0.0, 0.0, 0.0, 1.0); // alpha 1: the video surface must be opaque
                    clear(0x4000);
                    let _ = rc.render(0, w, h, true);
                    eglSwapBuffers(dpy, surf);
                    rc.report_swap();
                }
            }
            // `rc` dropped here with the GL context still current (mpv frees GL objects).
        }
        eglMakeCurrent(dpy, null_mut(), null_mut(), null_mut());
        eglDestroySurface(dpy, surf);
        drop(egl_win); // wl_egl_window after its EGLSurface
        eglDestroyContext(dpy, ctx);
        // eglTerminate deliberately NOT called: shared process-wide display, see docs.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_rounds_and_scales() {
        let g = geometry(0.4, 10.6, 800.0, 450.0, 1.0);
        assert_eq!(
            g,
            Geometry {
                x: 0,
                y: 11,
                w: 800,
                h: 450,
                pw: 800,
                ph: 450
            }
        );
        let g = geometry(0.0, 0.0, 801.5, 450.4, 1.25);
        assert_eq!((g.w, g.h, g.pw, g.ph), (802, 450, 1003, 563));
        let g = geometry(0.0, 0.0, 960.0, 540.0, 2.0);
        assert_eq!((g.pw, g.ph), (1920, 1080));
        let g = geometry(0.0, 0.0, 0.0, -3.0, 1.0);
        assert_eq!((g.w, g.h, g.pw, g.ph), (1, 1, 1, 1), "never zero-sized");
    }
}
