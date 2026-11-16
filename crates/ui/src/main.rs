//! NOCTURNE, Iced front end (temporary binary `nocturne-iced`; becomes `nocturne` at the cut-over).
#![allow(dead_code)] // the widget kit is consumed screen by screen over steps 4a-4e
mod account;
mod app;
mod keys;
mod motion;
mod platform;
mod player;
mod snap;
mod style;
mod task;
mod theme;
mod timing;
mod widget;

use app::App;
use iced::{Color, Size, window};

fn main() -> iced::Result {
    platform::mark_start();
    platform::probe_motion_pref();
    // Loading ~1000 system fonts takes a few hundred ms and would otherwise run after the window
    // and GPU are up; do it on a thread alongside them.
    std::thread::spawn(|| {
        drop(iced::advanced::graphics::text::font_system().read());
        timing::mark("fonts loaded");
    });
    platform::install_term_handlers();
    // Debug builds only: run the player page alone on a local file (see player/preview.rs).
    #[cfg(debug_assertions)]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("player") {
        return player::preview::run();
    }
    // Config (and .env) is read once, before any other thread exists. (Not for the player preview:
    // it never touches the configured server.)
    let cfg = nocturne_jellyfin::Config::load().ok();
    iced::application(move || App::boot(cfg.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        // Transparent app background: every screen paints its own opaque backdrop, only the
        // player page (step 4d) leaves a hole for the video subsurface.
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme::TEXT,
        })
        .theme(App::theme)
        .default_font(theme::FONT)
        .exit_on_close_request(false)
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(800.0, 560.0)),
            transparent: true,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "nocturne".into(),
                ..Default::default()
            },
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
