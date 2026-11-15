mod app;
mod bridge;
mod cache;
mod nav;
mod source;
mod theme;
mod ui;

use gpui::*;
use std::sync::mpsc;

fn main() {
    // Config (and .env) is read once, before any other thread exists.
    let cfg = nocturne_jellyfin::Config::load().ok();
    // Playback integration point: the UI sends requests here; the player crate consumes
    // them in a later task. For now a counter is logged (never titles or ids).
    let (tx, rx) = mpsc::channel::<app::PlayRequest>();
    std::thread::spawn(move || {
        for (n, _req) in rx.into_iter().enumerate() {
            eprintln!("play requests: {}", n + 1);
        }
    });

    Application::new().run(move |cx: &mut App| {
        let opts = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("NOCTURNE".into()),
                ..Default::default()
            }),
            app_id: Some("nocturne".into()),
            window_min_size: Some(size(px(800.0), px(560.0))),
            ..Default::default()
        };
        cx.open_window(opts, |window, cx| {
            let view = cx.new(|cx| app::Root::new(cfg, tx, window, cx));
            let handle = view.read(cx).focus.clone();
            window.focus(&handle);
            view
        })
        .expect("open window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
