//! `VideoSurface`: libmpv (OpenGL render API, EGL) drawing into a `wl_subsurface` placed BELOW a
//! GPUI window's `wl_surface`. GPUI stays a normal transparent window and draws controls on top.
//!
//! Threading: the main (GPUI) thread owns the Wayland proxies and calls `set_bounds`; one render
//! thread owns the EGL context + mpv render context. Teardown order (see `Drop`) is load-bearing.

use libmpv2::Mpv;
use libmpv2::render::{OpenGLInitParams, RenderParam, RenderParamApiType, mpv_render_update};
use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
    wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_egl::WlEglSurface;
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};

// ---- minimal EGL (links libEGL via glvnd; avoids a binding crate) ----
type Ptr = *mut c_void;
#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetPlatformDisplay(platform: u32, native: Ptr, attrs: *const isize) -> Ptr;
    fn eglInitialize(d: Ptr, maj: *mut c_int, min: *mut c_int) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglChooseConfig(d: Ptr, attrs: *const c_int, out: *mut Ptr, n: c_int, num: *mut c_int) -> u32;
    fn eglCreateContext(d: Ptr, cfg: Ptr, share: Ptr, attrs: *const c_int) -> Ptr;
    fn eglCreatePlatformWindowSurface(d: Ptr, cfg: Ptr, win: Ptr, attrs: *const isize) -> Ptr;
    fn eglMakeCurrent(d: Ptr, draw: Ptr, read: Ptr, ctx: Ptr) -> u32;
    fn eglSwapBuffers(d: Ptr, s: Ptr) -> u32;
    fn eglSwapInterval(d: Ptr, i: c_int) -> u32;
    fn eglDestroySurface(d: Ptr, s: Ptr) -> u32;
    fn eglDestroyContext(d: Ptr, c: Ptr) -> u32;
    fn eglGetProcAddress(name: *const c_char) -> Ptr;
    fn eglGetError() -> c_int;
    fn eglQueryString(d: Ptr, name: c_int) -> *const c_char;
}

struct St;
impl Dispatch<WlRegistry, GlobalListContents> for St {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(St: ignore WlCompositor);
delegate_noop!(St: ignore WlSubcompositor);
delegate_noop!(St: ignore WlSurface);
delegate_noop!(St: ignore WlSubsurface);
delegate_noop!(St: ignore WlRegion);
delegate_noop!(St: ignore WpViewporter);
delegate_noop!(St: ignore WpViewport);

struct Shared {
    stop: AtomicBool,
    dirty: Mutex<bool>,
    cv: Condvar,
    w: AtomicI32, // physical pixels
    h: AtomicI32,
    frames: std::sync::atomic::AtomicU64,
}
impl Shared {
    fn poke(&self) {
        *self.dirty.lock().unwrap() = true;
        self.cv.notify_one();
    }
}

struct SendBox<T>(T);
unsafe impl<T> Send for SendBox<T> {}

pub struct VideoSurface {
    // Drop order is explicit in `Drop`; field order only matters for the leftovers.
    mpv: Arc<Mpv>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    conn: Connection,
    queue: EventQueue<St>,
    surface: WlSurface,
    sub: WlSubsurface,
    viewport: WpViewport,
    compositor: WlCompositor,
    qh: QueueHandle<St>,
    pub gl_renderer: String,
}

fn gpa(_: &(), name: &str) -> *mut c_void {
    let c = CString::new(name).unwrap();
    unsafe { eglGetProcAddress(c.as_ptr()) }
}

impl VideoSurface {
    /// `display`/`parent`: raw `wl_display*` / `wl_surface*` from GPUI's `HasDisplayHandle` /
    /// `HasWindowHandle`. Must be called on the GPUI main thread. `hwdec` e.g. "auto-safe".
    pub fn new(display: *mut c_void, parent: *mut c_void, hwdec: &str) -> Result<Self, String> {
        unsafe {
            // Wrap GPUI's connection (foreign display: we never own/close it) with our own queue.
            let backend = Backend::from_foreign_display(display as *mut _);
            let conn = Connection::from_backend(backend);
            let (globals, queue) = registry_queue_init::<St>(&conn).map_err(|e| e.to_string())?;
            let qh = queue.handle();
            let compositor: WlCompositor = globals.bind(&qh, 1..=6, ()).map_err(|e| e.to_string())?;
            let subcomp: WlSubcompositor = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
            let viewporter: WpViewporter = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;

            let pid = ObjectId::from_ptr(WlSurface::interface(), parent as *mut _).map_err(|e| e.to_string())?;
            let parent_surface = WlSurface::from_id(&conn, pid).map_err(|e| e.to_string())?;

            let surface = compositor.create_surface(&qh, ());
            let sub = subcomp.get_subsurface(&surface, &parent_surface, &qh, ());
            sub.place_below(&parent_surface);
            sub.set_desync(); // video updates must not wait for GPUI commits
            // Clicks fall through to GPUI: empty input region; opaque video region.
            let empty = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&empty));
            empty.destroy();
            let viewport = viewporter.get_viewport(&surface, &qh, ());
            surface.commit();
            conn.flush().map_err(|e| e.to_string())?;

            let (w0, h0) = (640, 360);
            let egl_win = WlEglSurface::new(surface.id(), w0, h0).map_err(|e| e.to_string())?;

            let shared = Arc::new(Shared {
                stop: AtomicBool::new(false),
                dirty: Mutex::new(true),
                cv: Condvar::new(),
                w: AtomicI32::new(w0),
                h: AtomicI32::new(h0),
                frames: Default::default(),
            });

            let mpv = Mpv::with_initializer(|i| {
                i.set_option("vo", "libmpv")?;
                i.set_option("hwdec", hwdec)?;
                i.set_option("ao", "null")?;
                i.set_option("keep-open", "yes")?;
                i.set_option("loop-file", "inf")?;
                i.set_option("background", "color")?; // opaque black bars (default would be alpha 0)
                i.set_option("background-color", "#FF000000")?;
                i.set_option("terminal", "no")?;
                if let Ok(f) = std::env::var("VP_MPV_LOG") {
                    i.set_option("log-file", f.as_str())?;
                    i.set_option("msg-level", "all=v")?;
                }
                i.set_option("input-default-bindings", "no")?;
                i.set_option("osc", "no")?;
                Ok(())
            })
            .map_err(|e| format!("mpv init: {e:?}"))?;
            let mpv = Arc::new(mpv);

            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            let (mpv2, sh2) = (mpv.clone(), shared.clone());
            let payload = SendBox((display, egl_win));
            let thread = std::thread::Builder::new()
                .name("video-render".into())
                .spawn(move || {
                    let payload = payload; // capture the whole Send wrapper, not its raw-pointer fields
                    let SendBox((display, egl_win)) = payload;
                    render_thread(display, egl_win, mpv2, sh2, tx)
                })
                .map_err(|e| e.to_string())?;
            let gl_renderer = rx.recv().map_err(|e| e.to_string())??;
            Ok(Self { mpv, shared, thread: Some(thread), conn, queue, surface, sub, viewport, compositor, qh, gl_renderer })
        }
    }

    /// Logical position/size inside the parent window (GPUI Pixels as f32) plus scale factor.
    /// Position applies at the parent's next commit, so ask GPUI for a redraw after calling.
    pub fn set_bounds(&mut self, x: f32, y: f32, w: f32, h: f32, scale: f32) {
        let (lw, lh) = (w.round().max(1.0) as i32, h.round().max(1.0) as i32);
        self.sub.set_position(x.round() as i32, y.round() as i32);
        self.viewport.set_destination(lw, lh);
        // The whole video surface is opaque: lets the compositor skip blending/blur behind it.
        let region = self.compositor.create_region(&self.qh, ());
        region.add(0, 0, lw, lh);
        self.surface.set_opaque_region(Some(&region));
        region.destroy();
        let (pw, ph) = ((w * scale).round().max(1.0) as i32, (h * scale).round().max(1.0) as i32);
        let (ow, oh) = (self.shared.w.swap(pw, SeqCst), self.shared.h.swap(ph, SeqCst));
        if (ow, oh) != (pw, ph) {
            self.shared.poke();
        }
        let _ = self.conn.flush();
        let mut st = St;
        let _ = self.queue.dispatch_pending(&mut st); // drain our (ignored) events
    }

    pub fn mpv(&self) -> &Mpv {
        &self.mpv
    }
    pub fn frames_rendered(&self) -> u64 {
        self.shared.frames.load(Relaxed)
    }
    pub fn load(&self, path: &str) {
        let _ = self.mpv.command("loadfile", &[path]);
    }
}

impl Drop for VideoSurface {
    fn drop(&mut self) {
        // 1. stop the render thread; it frees the mpv render ctx, EGL surface/ctx, wl_egl_window
        self.shared.stop.store(true, SeqCst);
        self.shared.poke();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // 2. only now may the Wayland objects go (viewport, subsurface, surface), parent untouched
        self.viewport.destroy();
        self.sub.destroy();
        self.surface.destroy();
        let _ = self.conn.flush();
        // 3. `self.mpv` (last Arc) drops after this fn returns -> mpv_terminate_destroy.
    }
}

fn render_thread(
    display: *mut c_void,
    egl_win: WlEglSurface,
    mpv: Arc<Mpv>,
    sh: Arc<Shared>,
    ready: std::sync::mpsc::Sender<Result<String, String>>,
) {
    unsafe {
        let dpy = eglGetPlatformDisplay(0x31D8, display, null());
        let (mut ma, mut mi) = (0, 0);
        if dpy.is_null() || eglInitialize(dpy, &mut ma, &mut mi) == 0 {
            return drop(ready.send(Err(format!("eglInitialize failed 0x{:x}", eglGetError()))));
        }
        eglBindAPI(0x30A2); // OPENGL_API
        let cfg_attr = [0x3033, 4, 0x3040, 8, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3038];
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
        let vendor = std::ffi::CStr::from_ptr(eglQueryString(dpy, 0x3053)).to_string_lossy().into_owned();
        let glstr: unsafe extern "C" fn(u32) -> *const c_char = std::mem::transmute(eglGetProcAddress(c"glGetString".as_ptr()));
        let renderer = std::ffi::CStr::from_ptr(glstr(0x1F01)).to_string_lossy().into_owned();
        {
            let rc = mpv.create_render_context(vec![
                    RenderParam::ApiType(RenderParamApiType::OpenGl),
                    RenderParam::InitParams(OpenGLInitParams { get_proc_address: gpa, ctx: () }),
                    RenderParam::WaylandDisplay(display as *const c_void),
                    RenderParam::AdvancedControl(true),
                ]);
            let mut rc = match rc {
                Ok(rc) => rc,
                Err(e) => return drop(ready.send(Err(format!("mpv render ctx: {e:?}")))),
            };
            // Ready only now: `loadfile` before a render context exists is fatal for vo=libmpv.
            let _ = ready.send(Ok(format!("{renderer} (EGL {ma}.{mi} {vendor})")));
            let sh2 = sh.clone();
            rc.set_update_callback(move || sh2.poke());
            let clear_color: unsafe extern "C" fn(f32, f32, f32, f32) = std::mem::transmute(eglGetProcAddress(c"glClearColor".as_ptr()));
            let clear: unsafe extern "C" fn(u32) = std::mem::transmute(eglGetProcAddress(c"glClear".as_ptr()));
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
                let flags = rc.update().unwrap_or(0) as u32;
                let (w, h) = (sh.w.load(SeqCst), sh.h.load(SeqCst));
                let resized = (w, h) != (cw, ch);
                if resized {
                    egl_win.resize(w, h, 0, 0);
                    (cw, ch) = (w, h);
                }
                if flags & mpv_render_update::Frame as u32 != 0 || resized {
                    clear_color(0.0, 0.0, 0.0, 1.0); // alpha 1: the video surface must be opaque
                    clear(0x4000);
                    let _ = rc.render::<()>(0, w, h, true);
                    eglSwapBuffers(dpy, surf);
                    rc.report_swap();
                    sh.frames.fetch_add(1, Relaxed);
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
