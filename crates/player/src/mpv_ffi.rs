//! Minimal runtime-loaded libmpv binding (signatures from mpv/client.h, render.h, render_gl.h).
//!
//! libmpv is `dlopen`ed instead of linked so a missing `libmpv.so.2` becomes a recoverable error
//! (the app can fall back to the external `mpv` player) instead of the dynamic loader killing the
//! process before `main`. Only what `embedded.rs` needs is declared. In-process mpv uses the OpenGL
//! render API only (see the RULE in `embedded.rs`).

// Only the Linux in-window player calls most of this; elsewhere just the load probe is used.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

/// The library to `dlopen`, per OS, in order of preference; `exe_dir` is the directory of the
/// running executable. Linux: the distro's soname (unchanged). macOS: the app bundle's
/// `Frameworks/` (next to `Contents/MacOS`), then the default paths and Homebrew. Windows: next to
/// the exe, then the DLL search path.
fn candidates(os: &str, exe_dir: Option<&Path>) -> Vec<String> {
    let in_dir = |d: Option<PathBuf>, f: &str| d.map(|d| d.join(f).display().to_string());
    let list = match os {
        "macos" => vec![
            in_dir(exe_dir.map(|d| d.join("../Frameworks")), "libmpv.2.dylib"),
            in_dir(exe_dir.map(Path::to_path_buf), "libmpv.2.dylib"),
            Some("libmpv.2.dylib".into()),
            Some("/opt/homebrew/lib/libmpv.2.dylib".into()),
            Some("/usr/local/lib/libmpv.2.dylib".into()),
        ],
        "windows" => vec![
            in_dir(exe_dir.map(Path::to_path_buf), "libmpv-2.dll"),
            Some("libmpv-2.dll".into()),
        ],
        _ => vec![Some("libmpv.so.2".into())],
    };
    list.into_iter().flatten().collect()
}

/// Directory of the running executable.
pub(crate) fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}
/// Test/debug override for the library to load (e.g. a nonexistent path to exercise the failure path).
const ENV_OVERRIDE: &str = "NOCTURNE_LIBMPV";

const MPV_FORMAT_STRING: c_int = 1;
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_DOUBLE: c_int = 5;
const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_EVENT_FILE_LOADED: c_int = 8;
const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
const MPV_RENDER_PARAM_WL_DISPLAY: c_int = 9;
const MPV_RENDER_PARAM_ADVANCED_CONTROL: c_int = 10;
/// `mpv_render_context_update` flag: a new frame is ready to `render`.
pub const UPDATE_FRAME: u64 = 1;

#[repr(C)]
struct RawParam {
    kind: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct RawEvent {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct RawEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct GlInitParams {
    get_proc_address: GetProcAddr,
    ctx: *mut c_void,
}
#[repr(C)]
struct GlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

pub type GetProcAddr = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type Handle = *mut c_void;

struct Lib {
    client_api_version: unsafe extern "C" fn() -> std::ffi::c_ulong,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    free: unsafe extern "C" fn(*mut c_void),
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(Handle),
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(Handle, *mut *const c_char) -> c_int,
    set_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    get_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    observe_property: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(Handle, f64) -> *mut RawEvent,
    rc_create: unsafe extern "C" fn(*mut Handle, Handle, *mut RawParam) -> c_int,
    rc_set_update_callback:
        unsafe extern "C" fn(Handle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    rc_update: unsafe extern "C" fn(Handle) -> u64,
    rc_render: unsafe extern "C" fn(Handle, *mut RawParam) -> c_int,
    rc_report_swap: unsafe extern "C" fn(Handle),
    rc_free: unsafe extern "C" fn(Handle),
    /// Keeps the mapping alive: the fn pointers above are only valid while this exists.
    _lib: Library,
}

fn load(name: &str) -> Result<Lib, String> {
    unsafe {
        let lib = Library::new(name).map_err(|e| format!("cannot load {name}: {e}"))?;
        macro_rules! sym {
            ($n:literal) => {
                *lib.get(concat!($n, "\0").as_bytes())
                    .map_err(|e| format!("{name} lacks {}: {e}", $n))?
            };
        }
        let l = Lib {
            client_api_version: sym!("mpv_client_api_version"),
            error_string: sym!("mpv_error_string"),
            free: sym!("mpv_free"),
            create: sym!("mpv_create"),
            initialize: sym!("mpv_initialize"),
            terminate_destroy: sym!("mpv_terminate_destroy"),
            set_option_string: sym!("mpv_set_option_string"),
            command: sym!("mpv_command"),
            set_property: sym!("mpv_set_property"),
            get_property: sym!("mpv_get_property"),
            observe_property: sym!("mpv_observe_property"),
            wait_event: sym!("mpv_wait_event"),
            rc_create: sym!("mpv_render_context_create"),
            rc_set_update_callback: sym!("mpv_render_context_set_update_callback"),
            rc_update: sym!("mpv_render_context_update"),
            rc_render: sym!("mpv_render_context_render"),
            rc_report_swap: sym!("mpv_render_context_report_swap"),
            rc_free: sym!("mpv_render_context_free"),
            _lib: lib,
        };
        let major = (l.client_api_version)() >> 16;
        if major != 2 {
            return Err(format!(
                "{name}: unsupported mpv client API major {major} (need 2)"
            ));
        }
        Ok(l)
    }
}

/// Loaded once per process and never unloaded (mpv spawns threads that may outlive any handle).
fn lib() -> Result<&'static Lib, String> {
    static LIB: OnceLock<Result<Lib, String>> = OnceLock::new();
    LIB.get_or_init(|| {
        let names = match std::env::var(ENV_OVERRIDE) {
            Ok(n) => vec![n],
            Err(_) => candidates(os_name(), exe_dir().as_deref()),
        };
        let mut first_err = String::new();
        for n in &names {
            match load(n) {
                Ok(l) => return Ok(l),
                Err(e) if first_err.is_empty() => first_err = e,
                Err(_) => {}
            }
        }
        Err(first_err)
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Cheap probe: is a usable libmpv present? Loads it on first call.
pub fn available() -> Result<(), String> {
    lib().map(|_| ())
}

fn err(l: &Lib, code: c_int) -> String {
    let s = unsafe { CStr::from_ptr((l.error_string)(code)) };
    s.to_string_lossy().into_owned()
}

fn cstr(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| "string contains NUL".to_string())
}

fn check(l: &Lib, code: c_int) -> Result<(), String> {
    if code < 0 { Err(err(l, code)) } else { Ok(()) }
}

/// Applies the first of `alts` that `set` accepts; false if none was.
fn first_accepted(
    alts: &[(&str, &str)],
    mut set: impl FnMut(&str, &str) -> Result<(), String>,
) -> bool {
    alts.iter().any(|(k, v)| set(k, v).is_ok())
}

pub struct Mpv {
    l: &'static Lib,
    h: Handle,
}
// mpv handles are thread-safe by contract (client.h, "Threading").
unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

pub enum PropData {
    None,
    Flag(bool),
    Double(f64),
    Other,
}

pub enum Ev {
    None,
    Shutdown,
    FileLoaded,
    Property(String, PropData),
    Failed(String),
    Other,
}

impl Mpv {
    /// `mpv_create` + string options + `mpv_initialize`; `optional` groups of alternatives: in each group the first option
    /// mpv accepts is kept, and a group nothing in is accepted for is skipped. For options whose
    /// name or values differ between libmpv versions (mpv < 0.40 rejects `background=color`).
    pub fn new_with(
        options: &[(&str, &str)],
        optional: &[&[(&str, &str)]],
    ) -> Result<Self, String> {
        let l = lib()?;
        let h = unsafe { (l.create)() };
        if h.is_null() {
            return Err("mpv_create failed (out of memory or LC_NUMERIC is not C)".into());
        }
        let m = Mpv { l, h }; // Drop destroys the handle on any early return below
        for (k, v) in options {
            let (k, v) = (cstr(k)?, cstr(v)?);
            check(l, unsafe {
                (l.set_option_string)(h, k.as_ptr(), v.as_ptr())
            })?;
        }
        for alts in optional {
            first_accepted(alts, |k, v| {
                let (k, v) = (cstr(k)?, cstr(v)?);
                check(l, unsafe {
                    (l.set_option_string)(h, k.as_ptr(), v.as_ptr())
                })
            });
        }
        check(l, unsafe { (l.initialize)(h) })?;
        Ok(m)
    }

    pub fn command(&self, name: &str, args: &[&str]) -> Result<(), String> {
        let owned: Vec<CString> = std::iter::once(name)
            .chain(args.iter().copied())
            .map(cstr)
            .collect::<Result<_, _>>()?;
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(null());
        check(self.l, unsafe {
            (self.l.command)(self.h, ptrs.as_mut_ptr())
        })
    }

    pub fn set_flag(&self, name: &str, v: bool) -> Result<(), String> {
        let (n, mut v) = (cstr(name)?, v as c_int);
        let p = &mut v as *mut c_int as *mut c_void;
        check(self.l, unsafe {
            (self.l.set_property)(self.h, n.as_ptr(), MPV_FORMAT_FLAG, p)
        })
    }

    pub fn set_double(&self, name: &str, mut v: f64) -> Result<(), String> {
        let n = cstr(name)?;
        let p = &mut v as *mut f64 as *mut c_void;
        check(self.l, unsafe {
            (self.l.set_property)(self.h, n.as_ptr(), MPV_FORMAT_DOUBLE, p)
        })
    }

    pub fn get_string(&self, name: &str) -> Result<String, String> {
        let n = cstr(name)?;
        let mut out: *mut c_char = null_mut();
        let p = &mut out as *mut *mut c_char as *mut c_void;
        check(self.l, unsafe {
            (self.l.get_property)(self.h, n.as_ptr(), MPV_FORMAT_STRING, p)
        })?;
        let s = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.l.free)(out as *mut c_void) };
        Ok(s)
    }

    pub fn observe_flag(&self, name: &str, id: u64) -> Result<(), String> {
        self.observe(name, id, MPV_FORMAT_FLAG)
    }

    pub fn observe_double(&self, name: &str, id: u64) -> Result<(), String> {
        self.observe(name, id, MPV_FORMAT_DOUBLE)
    }

    fn observe(&self, name: &str, id: u64, fmt: c_int) -> Result<(), String> {
        let n = cstr(name)?;
        check(self.l, unsafe {
            (self.l.observe_property)(self.h, id, n.as_ptr(), fmt)
        })
    }

    pub fn wait_event(&self, timeout: f64) -> Ev {
        let ev = unsafe { &*(self.l.wait_event)(self.h, timeout) };
        if ev.id == MPV_EVENT_NONE {
            return Ev::None;
        }
        if ev.error < 0 {
            return Ev::Failed(err(self.l, ev.error));
        }
        match ev.id {
            MPV_EVENT_SHUTDOWN => Ev::Shutdown,
            MPV_EVENT_FILE_LOADED => Ev::FileLoaded,
            MPV_EVENT_PROPERTY_CHANGE => {
                let p = unsafe { &*(ev.data as *const RawEventProperty) };
                let name = unsafe { CStr::from_ptr(p.name) }
                    .to_string_lossy()
                    .into_owned();
                let data = match p.format {
                    0 => PropData::None,
                    MPV_FORMAT_FLAG if !p.data.is_null() => {
                        PropData::Flag(unsafe { *(p.data as *const c_int) } != 0)
                    }
                    MPV_FORMAT_DOUBLE if !p.data.is_null() => {
                        PropData::Double(unsafe { *(p.data as *const f64) })
                    }
                    _ => PropData::Other,
                };
                Ev::Property(name, data)
            }
            _ => Ev::Other,
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // Quit the core and wait for it, as the old libmpv2 binding did (teardown order in embedded.rs relies on it).
        unsafe { (self.l.terminate_destroy)(self.h) }
    }
}

/// OpenGL render context. Create, `update`, `render` and drop on the thread whose GL context is current.
pub struct RenderCtx<'a> {
    l: &'static Lib,
    h: Handle,
    cb: *mut Box<dyn Fn() + Send>,
    _mpv: PhantomData<&'a Mpv>,
}

unsafe extern "C" fn update_trampoline(ctx: *mut c_void) {
    unsafe { (*(ctx as *const Box<dyn Fn() + Send>))() }
}

impl<'a> RenderCtx<'a> {
    /// OpenGL API, `wl_display` for hwdec, advanced control on.
    pub fn new(mpv: &'a Mpv, gpa: GetProcAddr, wl_display: *const c_void) -> Result<Self, String> {
        let l = mpv.l;
        let mut init = GlInitParams {
            get_proc_address: gpa,
            ctx: null_mut(),
        };
        let mut advanced: c_int = 1;
        let mut params = [
            RawParam {
                kind: MPV_RENDER_PARAM_API_TYPE,
                data: c"opengl".as_ptr() as *mut c_void,
            },
            RawParam {
                kind: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init as *mut _ as *mut c_void,
            },
            RawParam {
                kind: MPV_RENDER_PARAM_WL_DISPLAY,
                data: wl_display as *mut c_void,
            },
            RawParam {
                kind: MPV_RENDER_PARAM_ADVANCED_CONTROL,
                data: &mut advanced as *mut _ as *mut c_void,
            },
            RawParam {
                kind: 0,
                data: null_mut(),
            },
        ];
        let mut h: Handle = null_mut();
        check(l, unsafe {
            (l.rc_create)(&mut h, mpv.h, params.as_mut_ptr())
        })?;
        Ok(RenderCtx {
            l,
            h,
            cb: null_mut(),
            _mpv: PhantomData,
        })
    }

    /// Callback runs on an mpv thread; it must not call into mpv.
    pub fn set_update_callback(&mut self, f: impl Fn() + Send + 'static) {
        let new: *mut Box<dyn Fn() + Send> = Box::into_raw(Box::new(Box::new(f)));
        unsafe {
            (self.l.rc_set_update_callback)(self.h, Some(update_trampoline), new as *mut c_void)
        };
        if !self.cb.is_null() {
            drop(unsafe { Box::from_raw(self.cb) });
        }
        self.cb = new;
    }

    pub fn update(&self) -> u64 {
        unsafe { (self.l.rc_update)(self.h) }
    }

    pub fn render(&self, fbo: i32, w: i32, h: i32, flip_y: bool) -> Result<(), String> {
        let mut f = GlFbo {
            fbo,
            w,
            h,
            internal_format: 0,
        };
        let mut flip = flip_y as c_int;
        let mut params = [
            RawParam {
                kind: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut f as *mut _ as *mut c_void,
            },
            RawParam {
                kind: MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut _ as *mut c_void,
            },
            RawParam {
                kind: 0,
                data: null_mut(),
            },
        ];
        check(self.l, unsafe {
            (self.l.rc_render)(self.h, params.as_mut_ptr())
        })
    }

    pub fn report_swap(&self) {
        unsafe { (self.l.rc_report_swap)(self.h) }
    }
}

impl Drop for RenderCtx<'_> {
    fn drop(&mut self) {
        // Free first: after this mpv no longer calls the update callback, so its box can go.
        unsafe { (self.l.rc_free)(self.h) };
        if !self.cb.is_null() {
            drop(unsafe { Box::from_raw(self.cb) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_options_fall_back_per_libmpv_version() {
        let alts = [("background", "color"), ("background", "#FF000000")];
        // mpv >= 0.40 takes the mode word ...
        let mut got = vec![];
        assert!(first_accepted(&alts, |k, v| {
            got.push((k.to_string(), v.to_string()));
            Ok(())
        }));
        assert_eq!(got, [("background".to_string(), "color".to_string())]);
        // ... older mpv (-7 for the word) takes the colour itself ...
        let mut got = vec![];
        assert!(first_accepted(&alts, |_, v| {
            got.push(v.to_string());
            if v == "color" {
                Err("bad".into())
            } else {
                Ok(())
            }
        }));
        assert_eq!(got, ["color", "#FF000000"]);
        // ... and an option mpv does not know at all is skipped, not fatal.
        assert!(!first_accepted(
            &[("background-color", "#FF000000")],
            |_, _| Err("x".into())
        ));
    }

    #[test]
    fn library_names_per_os() {
        let d = Path::new("/App/Contents/MacOS");
        assert_eq!(candidates("linux", Some(d)), ["libmpv.so.2"]);
        let mac = candidates("macos", Some(d));
        // Compare components: on a Windows host the join uses `\`.
        assert!(
            Path::new(&mac[0]).ends_with("MacOS/../Frameworks/libmpv.2.dylib"),
            "{mac:?}"
        );
        assert!(mac.contains(&"libmpv.2.dylib".to_string()));
        let win = candidates("windows", Some(Path::new("C:/n")));
        assert!(
            win[0].ends_with("libmpv-2.dll") && win[0].contains("C:/n"),
            "{win:?}"
        );
        assert_eq!(candidates("windows", None), ["libmpv-2.dll"]);
    }

    #[test]
    fn missing_library_is_an_error_not_a_panic() {
        let e = load("/nonexistent/libmpv-nocturne-test.so.2")
            .err()
            .unwrap();
        assert!(e.contains("cannot load"), "{e}");
    }

    /// Needs libmpv installed (a CI/dev prerequisite); no display or audio device.
    #[test]
    fn headless_properties_and_events_round_trip() {
        let background: [&[(&str, &str)]; 2] = [
            &[("background", "color"), ("background", "#FF000000")],
            &[("background-color", "#FF000000")],
        ];
        let m = match Mpv::new_with(
            &[("vo", "null"), ("ao", "null"), ("terminal", "no")],
            &background,
        ) {
            Ok(m) => m,
            // Only Linux embeds libmpv; Windows and macOS play through the external mpv, so a missing libmpv there is not a failure.
            Err(e) if !cfg!(target_os = "linux") => {
                eprintln!("skipped: {e}");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        m.observe_flag("pause", 1).unwrap();
        m.set_flag("pause", true).unwrap();
        assert_eq!(m.get_string("pause").unwrap(), "yes");
        m.set_double("volume", 42.0).unwrap();
        assert_eq!(m.get_string("volume").unwrap(), "42.000000");
        assert!(m.command("bogus-command", &[]).is_err());
        let mut saw_pause = false;
        for _ in 0..20 {
            match m.wait_event(0.1) {
                Ev::Property(n, PropData::Flag(true)) if n == "pause" => saw_pause = true,
                Ev::None => break,
                _ => {}
            }
        }
        assert!(saw_pause);
    }
}
