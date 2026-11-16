//! NOCTURNE, Iced front end (temporary binary `nocturne-iced`; becomes `nocturne` at the cut-over).
#![allow(dead_code)] // the widget kit is consumed screen by screen over steps 4a-4e
mod account;
mod app;
mod keys;
mod platform;
mod style;
mod task;
mod theme;
mod widget;

use app::App;
use iced::{Color, Size, window};

fn main() -> iced::Result {
    // Config (and .env) is read once, before any other thread exists.
    let cfg = nocturne_jellyfin::Config::load().ok();
    platform::install_term_handlers();
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
