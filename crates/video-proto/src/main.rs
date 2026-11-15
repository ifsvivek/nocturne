//! Feasibility prototype. See docs/video-embedding.md.
//! usage: video-proto <clip> [--mode sub|own] [--gpu-api opengl|vulkan] [--hwdec auto-safe]
//!        [--cycles N] [--hold-ms 1500] [--order surface-first|window-first] [--quit-after S]
mod video;

use gpui::*;
use libmpv2::Mpv;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::time::Duration;
use video::VideoSurface;

#[derive(Clone)]
struct Args {
    clip: String,
    own: bool,
    gpu_api: String,
    hwdec: String,
    cycles: u32,
    hold_ms: u64,
    window_first: bool,
    quit_after: Option<u64>,
}

enum Surf {
    Sub(VideoSurface),
    Own(Mpv),
}

struct Root {
    args: Args,
    surf: Option<Surf>,
    want: bool,
    bounds: Option<Bounds<Pixels>>,
    pos: f64,
    dur: f64,
    paused: bool,
    last_frames: u64,
}

fn log(s: impl AsRef<str>) {
    println!("[vp {:>8.3}] {}", START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64(), s.as_ref());
}
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

impl Root {
    fn start(&mut self, window: &mut Window) {
        if self.surf.is_some() {
            return;
        }
        if self.args.own {
            let a = self.args.clone();
            let m = Mpv::with_initializer(|i| {
                i.set_option("vo", "gpu-next")?;
                i.set_option("gpu-context", if a.gpu_api == "vulkan" { "waylandvk" } else { "wayland" })?;
                i.set_option("gpu-api", a.gpu_api.as_str())?;
                i.set_option("hwdec", a.hwdec.as_str())?;
                i.set_option("ao", "null")?;
                i.set_option("terminal", "no")?;
                if let Ok(f) = std::env::var("VP_MPV_LOG") {
                    i.set_option("log-file", f.as_str())?;
                    i.set_option("msg-level", "all=v")?;
                }
                Ok(())
            })
            .expect("mpv");
            m.command("loadfile", &[&self.args.clip]).unwrap();
            self.surf = Some(Surf::Own(m));
            return;
        }
        let d = match window.display_handle().unwrap().as_raw() {
            RawDisplayHandle::Wayland(h) => h.display.as_ptr(),
            _ => panic!("not wayland"),
        };
        let s = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::Wayland(h) => h.surface.as_ptr(),
            _ => panic!("not wayland"),
        };
        let vs = VideoSurface::new(d, s, &self.args.hwdec).expect("VideoSurface");
        log(format!("video surface up, GL: {}", vs.gl_renderer));
        vs.load(&self.args.clip);
        self.surf = Some(Surf::Sub(vs));
    }

    fn stop(&mut self) {
        self.surf = None; // Drop order lives in VideoSurface::drop
    }

    fn mpv(&self) -> Option<&Mpv> {
        match self.surf.as_ref()? {
            Surf::Sub(v) => Some(v.mpv()),
            Surf::Own(m) => Some(m),
        }
    }

    fn poll(&mut self) {
        if let Some(m) = self.mpv() {
            let pos = m.get_property::<f64>("time-pos").unwrap_or(0.0);
            let dur = m.get_property::<f64>("duration").unwrap_or(0.0);
            let paused = m.get_property::<bool>("pause").unwrap_or(false);
            self.pos = pos;
            self.dur = dur;
            self.paused = paused;
        }
    }

    fn stats(&mut self) -> String {
        let rendered = match self.surf.as_ref() {
            Some(Surf::Sub(v)) => v.frames_rendered(),
            _ => 0,
        };
        let dr = rendered - self.last_frames;
        self.last_frames = rendered;
        let Some(m) = self.mpv() else { return "no player".into() };
        let g = |n: &str| m.get_property::<i64>(n).map(|v| v.to_string()).unwrap_or("-".into());
        let hw = m.get_property::<String>("hwdec-current").unwrap_or("-".into());
        let fps = m.get_property::<f64>("estimated-vf-fps").unwrap_or(0.0);
        format!(
            "hwdec={hw} vf_fps={fps:.1} rendered/s={dr} dropped(vo)={} dropped(dec)={} delayed={} pos={:.1}",
            g("frame-drop-count"),
            g("decoder-frame-drop-count"),
            g("vo-delayed-frame-count"),
            self.pos
        )
    }
}

impl Render for Root {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let frac = if self.dur > 0.0 { (self.pos / self.dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
        div()
            .size_full()
            .relative() // NOTE: no background at all => alpha 0 => video shows through
            .child(
                canvas(
                    |b, _, _| b,
                    move |b, _, window, cx| {
                        let sf = window.scale_factor();
                        view.update(cx, |t, cx| {
                            if t.want {
                                t.start(window);
                            }
                            if t.bounds != Some(b) {
                                t.bounds = Some(b);
                                if let Some(Surf::Sub(v)) = t.surf.as_mut() {
                                    v.set_bounds(
                                        f32::from(b.origin.x),
                                        f32::from(b.origin.y),
                                        f32::from(b.size.width),
                                        f32::from(b.size.height),
                                        sf,
                                    );
                                    log(format!("set_bounds {:?} scale {sf}", b));
                                    cx.notify();
                                }
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(64.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_4()
                    .bg(rgba(0x000000a0)) // 63% black over the video: tests alpha blending
                    .child(
                        div()
                            .id("pp")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x2a4fd6))
                            .text_color(white())
                            .cursor_pointer()
                            .child(if self.paused { "Play" } else { "Pause" })
                            .on_click(cx.listener(|t, _, _, cx| {
                                if let Some(m) = t.mpv() {
                                    let p = m.get_property::<bool>("pause").unwrap_or(false);
                                    let _ = m.set_property("pause", !p);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(6.0))
                            .rounded_full()
                            .bg(rgba(0xffffff40))
                            .child(div().h_full().w(relative(frac)).rounded_full().bg(rgb(0x6f8cff))),
                    )
                    .child(div().text_color(white()).child(format!("{:.0}/{:.0}s", self.pos, self.dur))),
            )
            .child(
                // Semi-transparent swatches over the video: red 50%, and a solid white chip.
                div()
                    .absolute()
                    .top(px(400.0))
                    .left(px(300.0))
                    .w(px(160.0))
                    .h(px(60.0))
                    .bg(rgba(0xff000080))
                    .text_color(white())
                    .child("GPUI overlay"),
            )
    }
}

fn parse() -> Args {
    let mut a = Args {
        clip: String::new(),
        own: false,
        gpu_api: "vulkan".into(),
        hwdec: "auto-safe".into(),
        cycles: 0,
        hold_ms: 1500,
        window_first: false,
        quit_after: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut v = || it.next().expect("missing value");
        match x.as_str() {
            "--mode" => a.own = v() == "own",
            "--gpu-api" => a.gpu_api = v(),
            "--hwdec" => a.hwdec = v(),
            "--cycles" => a.cycles = v().parse().unwrap(),
            "--hold-ms" => a.hold_ms = v().parse().unwrap(),
            "--order" => a.window_first = v() == "window-first",
            "--quit-after" => a.quit_after = Some(v().parse().unwrap()),
            c => a.clip = c.into(),
        }
    }
    assert!(!a.clip.is_empty(), "usage: video-proto <clip> [options]");
    a
}

fn main() {
    START.get_or_init(std::time::Instant::now);
    let args = parse();
    Application::new().run(move |cx: &mut App| {
        let opts = WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("video-proto".into()), ..Default::default() }),
            app_id: Some("video-proto".into()),
            window_background: WindowBackgroundAppearance::Transparent,
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(960.0), px(540.0)), cx))),
            ..Default::default()
        };
        let a2 = args.clone();
        let win = cx
            .open_window(opts, move |_, cx| {
                cx.new(|_| Root {
                    args: a2,
                    surf: None,
                    want: false,
                    bounds: None,
                    pos: 0.0,
                    dur: 0.0,
                    paused: false,
                    last_frames: 0,
                })
            })
            .expect("window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let args = args.clone();
        cx.spawn(async move |cx| {
            let ex = cx.background_executor().clone();
            let sleep = move |ms| ex.timer(Duration::from_millis(ms));
            sleep(500).await;
            let n = if args.cycles == 0 { 1 } else { args.cycles };
            for i in 1..=n {
                win.update(cx, |r, w, cx| {
                    r.want = std::env::var_os("VP_NOVIDEO").is_none();
                    r.bounds = None; // force bounds push to the new surface
                    if r.want {
                        r.start(w);
                    }
                    cx.notify();
                })
                .ok();
                if args.cycles == 0 {
                    // single long run: print stats each second
                    let t0 = std::time::Instant::now();
                    loop {
                        sleep(1000).await;
                        let s = win.update(cx, |r, _, cx| {
                            r.poll();
                            cx.notify();
                            r.stats()
                        });
                        if let Ok(s) = s {
                            log(s);
                        }
                        if args.quit_after.is_some_and(|q| t0.elapsed().as_secs() >= q) {
                            break;
                        }
                    }
                    break;
                }
                sleep(args.hold_ms).await;
                let s = win.update(cx, |r, _, _| {
                    r.poll();
                    r.stats()
                });
                log(format!("cycle {i}/{n} playing: {}", s.unwrap_or_default()));
                if args.window_first && i == n {
                    break; // handled below
                }
                win.update(cx, |r, _, cx| {
                    r.want = false;
                    r.stop();
                    cx.notify();
                })
                .ok();
                log(format!("cycle {i}/{n} stopped"));
                sleep(300).await;
            }
            if args.window_first {
                // anti-pattern ordering: destroy the window while the subsurface is still alive
                cx.update(|cx| {
                    cx.windows().into_iter().for_each(|w| {
                        let _ = w.update(cx, |_, w, _| w.remove_window());
                    })
                })
                .ok();
                sleep(300).await;
                log("window removed first; now dropping video surface");
            } else {
                win.update(cx, |r, _, _| r.stop()).ok();
            }
            log("done, quitting");
            cx.update(|cx| cx.quit()).ok();
        })
        .detach();
    });
    log("run() returned (clean exit)");
}
