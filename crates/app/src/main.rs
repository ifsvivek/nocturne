mod account;
mod admin;
mod app;
mod assets;
mod bridge;
mod cache;
mod hooks;
mod nav;
mod playback;
mod settings;
mod source;
mod theme;
mod ui;
mod video;

use gpui::*;

fn main() {
    // Config (and .env) is read once, before any other thread exists.
    let cfg = nocturne_jellyfin::Config::load().ok();
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
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
