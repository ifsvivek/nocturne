mod account;
mod admin;
mod app;
mod assets;
mod blurhash;
mod bridge;
mod cache;
mod detail;
mod imgcache;
mod libview;
mod nav;
mod player_logic;
mod player_mpris;
mod player_ui;
mod player_view;
mod search;
mod settings;
mod source;
mod sync;
mod theme;
mod ui;
mod video;

use gpui::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by SIGTERM/SIGINT; `Root::tick` turns it into a normal quit (settings are saved on change).
pub static TERM: AtomicBool = AtomicBool::new(false);

extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::Relaxed);
}

/// The now-playing poster the player wrote for desktop widgets.
pub fn remove_art_file() {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        let _ = std::fs::remove_file(
            std::path::Path::new(&d).join(format!("nocturne-art-{}.img", std::process::id())),
        );
    }
}

fn main() {
    // Config (and .env) is read once, before any other thread exists.
    let cfg = nocturne_jellyfin::Config::load().ok();
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            let opts = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("NOCTURNE".into()),
                    ..Default::default()
                }),
                app_id: Some("nocturne".into()),
                // Transparent so the video subsurface below shows through; every normal screen paints
                // an opaque background (Root), only the player page leaves a hole.
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.0), px(800.0)),
                    cx,
                ))),
                window_min_size: Some(size(px(800.0), px(560.0))),
                ..Default::default()
            };
            cx.open_window(opts, |window, cx| {
                let view = cx.new(|cx| app::Root::new(cfg, window, cx));
                let handle = view.read(cx).focus.clone();
                window.focus(&handle);
                view
            })
            .expect("open window");
            cx.on_app_quit(|_| async { remove_art_file() }).detach();
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
