//! Spike: in-window libmpv video under Iced 0.14 on Wayland.
//!
//! Env: SPIKE_CLIP (default /tmp/spike.mp4), SPIKE_AUTOPLAY=1 (press play after ~1 s),
//! SPIKE_RESIZE=1 (resize window at ~4 s), SPIKE_EXIT_AFTER=<secs> (go through the close path).
use std::ffi::c_void;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, renderer};
use iced::widget::{button, column, container, stack, text};
use iced::window::raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use iced::{
    Background, Color, Element, Event, Fill, Gradient, Length, Rectangle, Size, Subscription, Task,
    mouse, window,
};
use nocturne_player::EmbeddedPlayer;

fn main() -> iced::Result {
    iced::application(Spike::boot, Spike::update, Spike::view)
        .title("iced-embed spike")
        .subscription(Spike::subscription)
        // Transparent app background: the hole (and everything not painted) stays alpha 0.
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: Color::WHITE,
        })
        .theme(iced::Theme::Dark)
        .window(window::Settings {
            size: Size::new(1100.0, 700.0),
            min_size: Some(Size::new(640.0, 400.0)),
            transparent: true,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "nocturne-spike".into(),
                ..Default::default()
            },
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}

#[derive(Debug, Clone)]
enum Msg {
    Window(Option<window::Id>),
    Scale(f32),
    Bounds(Rectangle),
    Play,
    Handles(Option<(usize, usize, &'static str)>),
    Tick(Instant),
    CloseRequested(window::Id),
}

struct Spike {
    win: Option<window::Id>,
    scale: f32,
    hole: Rectangle,
    player: Option<EmbeddedPlayer>,
    status: String,
    started: Instant,
    ticks: u32,
}

impl Spike {
    fn boot() -> (Self, Task<Msg>) {
        (
            Self {
                win: None,
                scale: 1.0,
                hole: Rectangle::new(iced::Point::ORIGIN, Size::new(1.0, 1.0)),
                player: None,
                status: "idle".into(),
                started: Instant::now(),
                ticks: 0,
            },
            // The initial window is already open when boot's task runs.
            window::oldest().map(Msg::Window),
        )
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Window(id) => {
                self.win = id;
                if let Some(id) = id {
                    return window::scale_factor(id).map(Msg::Scale);
                }
            }
            Msg::Scale(s) => self.scale = s,
            Msg::Bounds(r) => {
                self.hole = r;
                if let Some(p) = self.player.as_mut() {
                    p.set_bounds(r.x, r.y, r.width, r.height, self.scale);
                }
            }
            Msg::Play => {
                let Some(id) = self.win else {
                    return Task::none();
                };
                // Raw handles. Pointers are not Send, so ship them as usize.
                return window::run(id, |w| {
                    let win = w.window_handle().ok()?.as_raw();
                    let disp = w.display_handle().ok()?.as_raw();
                    match (win, disp) {
                        (RawWindowHandle::Wayland(s), RawDisplayHandle::Wayland(d)) => Some((
                            s.surface.as_ptr() as usize,
                            d.display.as_ptr() as usize,
                            "wayland",
                        )),
                        (RawWindowHandle::Xlib(_), _) => Some((0, 0, "xlib")),
                        (RawWindowHandle::Xcb(_), _) => Some((0, 0, "xcb")),
                        _ => Some((0, 0, "other")),
                    }
                })
                .map(Msg::Handles);
            }
            Msg::Handles(h) => {
                match h {
                    Some((s, d, kind)) if kind == "wayland" => {
                        // window::run callbacks run on the winit/main thread's event loop, and so
                        // does `update`, so creating the player here is on the right thread.
                        match EmbeddedPlayer::new(d as *mut c_void, s as *mut c_void) {
                            Ok(mut p) => {
                                let r = self.hole;
                                p.set_bounds(r.x, r.y, r.width, r.height, self.scale);
                                let clip = std::env::var("SPIKE_CLIP")
                                    .unwrap_or_else(|_| "/tmp/spike.mp4".into());
                                self.status = match p.load(&clip, None) {
                                    Ok(()) => "playing".into(),
                                    Err(e) => format!("load failed: {e}"),
                                };
                                self.player = Some(p);
                            }
                            Err(e) => self.status = format!("embed failed: {e}"),
                        }
                    }
                    Some((_, _, kind)) => {
                        self.status = format!("handle={kind}: use external Player");
                        eprintln!("HANDLE_KIND={kind}");
                    }
                    None => self.status = "no handle".into(),
                }
                eprintln!("STATUS={}", self.status);
            }
            Msg::Tick(_) => {
                self.ticks += 1;
                if let Some(p) = &self.player {
                    while let Ok(ev) = p.events().try_recv() {
                        eprintln!("EVENT={ev:?}");
                    }
                    let st = p.state();
                    self.status = format!("t={:?} paused={}", st.position, st.paused);
                }
                let secs = self.started.elapsed().as_secs_f32();
                let var = |k: &str| std::env::var(k).ok();
                if var("SPIKE_AUTOPLAY").is_some()
                    && self.player.is_none()
                    && secs > 1.0
                    && self.ticks % 10 == 0
                {
                    return Task::done(Msg::Play);
                }
                if var("SPIKE_RESIZE").is_some() && (4.0..4.2).contains(&secs) {
                    if let Some(id) = self.win {
                        return window::resize(id, Size::new(800.0, 560.0));
                    }
                }
                if let Some(n) = var("SPIKE_EXIT_AFTER").and_then(|v| v.parse::<f32>().ok()) {
                    if secs > n {
                        if let Some(id) = self.win {
                            return Task::done(Msg::CloseRequested(id));
                        }
                    }
                }
            }
            Msg::CloseRequested(id) => {
                // Drop the player (and its subsurface + EGL thread) BEFORE the window goes away.
                drop(self.player.take());
                eprintln!("PLAYER_DROPPED");
                return window::close(id).chain(iced::exit());
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(100)).map(Msg::Tick),
            window::close_requests().map(Msg::CloseRequested),
        ])
    }

    fn view(&self) -> Element<'_, Msg> {
        let grad = Gradient::Linear(
            iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                .add_stop(0.0, Color::from_rgb(0.08, 0.04, 0.16))
                .add_stop(1.0, Color::from_rgb(0.02, 0.02, 0.05)),
        );
        let hole = Hole { last: self.hole }.into_element();
        // 16:9 hole: width-limited, aspect via a fixed-ratio container.
        let video = container(stack![
            hole,
            container(
                text("OVERLAY TEXT over the video hole")
                    .size(28)
                    .color(Color::WHITE)
            )
            .padding(12)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
                ..Default::default()
            })
        ])
        .width(640)
        .height(360);
        let play = button(text(if self.player.is_some() {
            "playing"
        } else {
            "Play"
        }))
        .on_press(Msg::Play);
        // The panel must NOT paint under the hole: iced would composite it over the subsurface.
        // So the gradient is drawn on four pieces around a fixed-size hole.
        let panel = move |c, w, h| panel(c, w, h, grad);
        let none = || -> Element<'_, Msg> { iced::widget::Space::new().into() };
        column![
            panel(none(), Fill, Fill),
            iced::widget::row![
                panel(none(), Fill, Length::Fixed(360.0)),
                video,
                panel(none(), Fill, Length::Fixed(360.0))
            ],
            panel(
                column![play, text(&self.status).size(16)]
                    .spacing(12)
                    .align_x(iced::Center)
                    .into(),
                Fill,
                Fill
            ),
        ]
        .into()
    }
}

/// Transparent widget that reports its absolute layout bounds whenever they change.
/// Draws nothing, so the compositor shows the subsurface beneath through it.
struct Hole {
    last: Rectangle,
}

impl Hole {
    fn into_element<'a>(self) -> Element<'a, Msg> {
        Element::new(self)
    }
}

impl<Theme, R: renderer::Renderer> Widget<Msg, Theme, R> for Hole {
    fn size(&self) -> Size<Length> {
        Size::new(Fill, Fill)
    }
    fn layout(&mut self, _: &mut Tree, _: &R, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Fill, Fill)
    }
    fn draw(
        &self,
        _: &Tree,
        _: &mut R,
        _: &Theme,
        _: &renderer::Style,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
    }
    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &R,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
        _: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let b = layout.bounds();
            if b != self.last {
                self.last = b;
                shell.publish(Msg::Bounds(b));
            }
        }
    }
}

fn panel<'a>(c: Element<'a, Msg>, w: Length, h: Length, grad: Gradient) -> Element<'a, Msg> {
    container(c)
        .width(w)
        .height(h)
        .center_x(w)
        .style(move |_| container::Style {
            background: Some(Background::Gradient(grad)),
            ..Default::default()
        })
        .into()
}
