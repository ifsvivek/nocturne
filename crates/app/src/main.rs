//! NOCTURNE, Iced front end.
// Release builds on Windows are GUI-subsystem (no console window); debug builds keep the console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(dead_code)] // the widget kit is consumed screen by screen over steps 4a-4e
mod account;
mod admin;
mod app;
mod couch;
mod detail;
mod keys;
mod motion;
mod onboarding;
mod pad;
mod platform;
mod player;
mod queue;
mod single;
mod snap;
#[cfg(feature = "theme-spike")]
mod spike;
mod style;
mod syncplay;
mod task;
mod theme;
mod timing;
mod virt;
mod widget;

use app::App;
use iced::{Color, Size, window};

fn main() -> std::process::ExitCode {
    // winit would panic (backtrace and all) without a Wayland server: say it plainly instead.
    if let Some(msg) = platform::session_problem() {
        eprintln!("{msg}");
        return std::process::ExitCode::FAILURE;
    }
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "NOCTURNE could not open its window: {e}\n\
                 Check that your graphics drivers (Vulkan or OpenGL) work and that a graphical session is running."
            );
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> iced::Result {
    // Before anything costly: a second launch just wakes the first and leaves.
    let Some(_single) = single::acquire() else {
        return Ok(());
    };
    platform::mark_start();
    platform::probe_motion_pref();
    // Loading ~1000 system fonts takes a few hundred ms and would otherwise run after the window
    // and GPU are up; do it on a thread alongside them.
    std::thread::spawn(|| {
        drop(iced::advanced::graphics::text::font_system().read());
        timing::mark("fonts loaded");
    });
    platform::install_term_handlers();
    // Feature `theme-spike` only: the element-tree theme engine on a static Home screen (see spike.rs).
    #[cfg(feature = "theme-spike")]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("theme-spike") {
        return spike::run();
    }
    // Debug builds only: run the player page alone on a local file (see player/preview.rs).
    #[cfg(debug_assertions)]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("player") {
        return player::preview::run();
    }
    // Debug builds only: run the detail page alone on a throwaway server item or a local fixture
    // (see detail/preview.rs).
    #[cfg(debug_assertions)]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("detail") {
        return detail::preview::run();
    }
    // Debug builds only: the admin screen alone on a throwaway server (see admin/preview.rs).
    #[cfg(debug_assertions)]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("admin") {
        return admin::preview::run();
    }
    // Debug builds only: Watch together alone against throwaway servers (see syncplay/preview.rs).
    #[cfg(debug_assertions)]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("syncplay") {
        return syncplay::preview::run();
    }
    // Debug builds only: the first-run walkthrough alone (see onboarding/preview.rs).
    #[cfg(debug_assertions)]
    if std::env::var("NOCTURNE_PREVIEW").as_deref() == Ok("onboarding") {
        return onboarding::preview::run();
    }
    // Config (and .env) is read once, before any other thread exists. (Not for the player preview:
    // it never touches the configured server.)
    let cfg = nocturne_jellyfin::Config::load().ok();
    // The saved global theme, before the first frame (and the only moment its font family is read).
    let (th, lay) = nocturne_core::customize::Store::load().resolve_all(None);
    theme::set(&th, &lay);
    iced::application(move || App::boot(cfg.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        // Transparent app background: every screen paints its own opaque backdrop, only the
        // player page (step 4d) leaves a hole for the video subsurface.
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme::text(),
        })
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .default_font(theme::base())
        .exit_on_close_request(false)
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(800.0, 560.0)),
            transparent: true,
            platform_specific: crate::platform::window_settings(),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
