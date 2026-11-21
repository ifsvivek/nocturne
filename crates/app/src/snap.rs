//! Debug builds only: `NOCTURNE_SNAP=/tmp/x.png` writes a capture of the app window (its own pixels,
//! through Iced's `window::screenshot`, never a screen grab) after `NOCTURNE_SNAP_AFTER_MS`
//! (default 1500). A comma list takes several: `x.png`, `x-2.png`, ... Release builds do nothing.
use crate::app::message::Message;
use iced::Task;

#[cfg(debug_assertions)]
pub fn on_boot() -> Task<Message> {
    use iced::window;
    use std::time::Duration;
    // `NOCTURNE_KEYS="enter@3000,escape@4000"`: named keys injected into the root (QA without a
    // human; the app never reads it in release builds).
    let keys = std::env::var("NOCTURNE_KEYS").unwrap_or_default();
    let keys = keys.split(',').filter_map(|k| {
        let (name, ms) = k.split_once('@')?;
        let msg = match name {
            "open-admin" => Some(Message::OpenAdmin),
            "run-setup" => Some(Message::RunSetup),
            "open-settings" => Some(Message::MenuAct(crate::app::message::AcctAct::Settings)),
            "smart-menu" => Some(Message::Smart(crate::app::smart::Msg::Menu)),
            "smart-manage" => Some(Message::Smart(crate::app::smart::Msg::Manage)),
            "smart-new" => Some(Message::Smart(crate::app::smart::Msg::New)),
            n if n.starts_with("smart-open-") => n[11..]
                .parse()
                .ok()
                .map(|i| Message::Smart(crate::app::smart::Msg::Open(i))),
            "open-calendar" => Some(Message::Calendar(crate::app::calendar::Msg::Open)),
            "calendar-week" => Some(Message::Calendar(crate::app::calendar::Msg::View(
                nocturne_core::calendar::View::Week,
            ))),
            "calendar-next" => Some(Message::Calendar(crate::app::calendar::Msg::Step(1))),
            "open-sources" => Some(Message::OpenSources),
            // The Themes page: open it, type the path in `NOCTURNE_THEME_FILE` into the file field,
            // import it, preview it, apply it.
            "open-themes" => Some(Message::Themes(crate::app::themes::Msg::Open)),
            "theme-path" => Some(Message::Themes(crate::app::themes::Msg::Path(
                std::env::var("NOCTURNE_THEME_FILE").unwrap_or_default(),
            ))),
            "theme-import" => Some(Message::Themes(crate::app::themes::Msg::Import)),
            "theme-preview" => Some(Message::Themes(crate::app::themes::Msg::Preview)),
            "theme-apply" => Some(Message::Themes(crate::app::themes::Msg::Apply)),
            // `open-folders-1`: the Local folders screen of source id 1.
            n if n.starts_with("open-folders-") => n[13..].parse().ok().map(|i| {
                Message::Src(crate::app::message::SrcAct::Folders(
                    nocturne_core::source::SourceId(i),
                ))
            }),
            // Server UI QA: `cycle-filter`, `layout-1` (the per-server switch of source 1),
            // `sections-1` (its editor), `sec-toggle-2`, `sec-up-2`, `sec-down-2`, `sec-save`.
            "cycle-filter" => Some(Message::CycleFilter),
            "layout-all" => Some(Message::Src(crate::app::message::SrcAct::Layout(None))),
            n if n.starts_with("layout-") => n[7..].parse().ok().map(|i| {
                Message::Src(crate::app::message::SrcAct::Layout(Some(
                    nocturne_core::source::SourceId(i),
                )))
            }),
            // Search QA: `open-search` opens the overlay, `search-<text>` types the text into it (`_` is a space).
            "open-search" => Some(Message::OpenSearch),
            n if n.starts_with("search-") => Some(Message::Search(
                crate::app::search::SMsg::Input(n[7..].replace('_', " ")),
            )),
            // `NOCTURNE_THEME_REPORT=<file>`: what the active theme did (counts, imports, fonts) as JSON.
            "theme-report" => Some(Message::Themes(crate::app::themes::Msg::Report(
                std::env::var("NOCTURNE_THEME_REPORT").unwrap_or_default(),
            ))),
            // Theme Studio QA: runs the file in `NOCTURNE_STUDIO_SCRIPT` (see `app::studio::script_msgs`).
            #[cfg(feature = "theme-engine")]
            "studio-script" => Some(Message::Studio(crate::app::studio::Msg::Script)),
            // Themes gallery QA: `gallery-official`, `gallery-community`, `gallery-mine`, `gallery-refresh`, `gallery-view-editor`, `gallery-view-gallery`, `gallery-search-<text>`, `gallery-install-<id>`, `gallery-apply-<id>`.
            n if n.starts_with("gallery-") => {
                use crate::app::themes_gallery::{GMsg, Tab};
                let g = match &n[8..] {
                    "official" => GMsg::Tab(Tab::Official),
                    "community" => GMsg::Tab(Tab::Community),
                    "mine" => GMsg::Tab(Tab::Mine),
                    "refresh" => GMsg::Refresh,
                    "view-editor" => GMsg::View(crate::app::themes_gallery::View::Editor),
                    "view-gallery" => GMsg::View(crate::app::themes_gallery::View::Gallery),
                    i if i.starts_with("search-") => GMsg::Search(i[7..].replace('_', " ")),
                    i if i.starts_with("install-") => GMsg::Install(i[8..].into()),
                    i if i.starts_with("apply-") => GMsg::Apply(i[6..].into()),
                    _ => return None,
                };
                Some(Message::Themes(crate::app::themes::Msg::Gallery(g)))
            }
            "theme-safe" => Some(Message::Themes(crate::app::themes::Msg::Safe)),
            n if n.starts_with("sections-") => n[9..].parse().ok().map(|i| {
                Message::Src(crate::app::message::SrcAct::Sections(
                    nocturne_core::source::SourceId(i),
                ))
            }),
            n if n.starts_with("sec-toggle-") => n[11..]
                .parse()
                .ok()
                .map(|i| Message::Src(crate::app::message::SrcAct::SecToggle(i))),
            n if n.starts_with("sec-up-") => n[7..]
                .parse()
                .ok()
                .map(|i| Message::Src(crate::app::message::SrcAct::SecShift(i, -1))),
            n if n.starts_with("sec-down-") => n[9..]
                .parse()
                .ok()
                .map(|i| Message::Src(crate::app::message::SrcAct::SecShift(i, 1))),
            "sec-save" => Some(Message::Src(crate::app::message::SrcAct::SecSave)),
            "open-trakt" => Some(Message::Trakt(crate::app::trakt::Msg::Open)),
            // Seerr QA against the throwaway server: `open-seerr` (Settings > Seerr server), `seerr-fill` (form from
            // `NOCTURNE_SEERR_URL` / `_USER` / `_PASS`), `seerr-signin`, `seerr-signout`, `seerr-close`, `open-requests`,
            // `seerr-ask-movie-348` / `seerr-ask-tv-1396` (the Request dialog), `seerr-tick-<season>`, `seerr-confirm`,
            // `seerr-cancel-<id>` then `seerr-cancel-yes`.
            "open-seerr" => Some(Message::Requests(crate::app::requests::Msg::Open)),
            "seerr-fill" => Some(Message::Requests(crate::app::requests::Msg::Fill)),
            "seerr-signin" => Some(Message::Requests(crate::app::requests::Msg::SignIn)),
            "seerr-signout" => Some(Message::Requests(crate::app::requests::Msg::SignOut)),
            "seerr-close" => Some(Message::Requests(crate::app::requests::Msg::Close)),
            "open-requests" => Some(Message::Requests(crate::app::requests::Msg::PageOpen)),
            "seerr-confirm" => Some(Message::Requests(crate::app::requests::Msg::Confirm)),
            "seerr-cancel-yes" => Some(Message::Requests(crate::app::requests::Msg::CancelYes)),
            n if n.starts_with("seerr-tick-") => n[11..]
                .parse()
                .ok()
                .map(|i| Message::Requests(crate::app::requests::Msg::Tick(i))),
            n if n.starts_with("seerr-cancel-") => n[13..]
                .parse()
                .ok()
                .map(|i| Message::Requests(crate::app::requests::Msg::Cancel(i))),
            n if n.starts_with("seerr-ask-") => {
                let (k, id) = n[10..].split_once('-')?;
                Some(crate::app::requests::ask(
                    nocturne_seerr::Kind::parse(k)?,
                    id.parse().ok()?,
                    String::new(),
                    None,
                ))
            }
            // `open-global-0`: the header's n-th merged link (Movies, Shows, ...).
            "view-poster" | "view-thumb" | "view-list" => {
                use nocturne_core::libview::ViewKind as V;
                Some(Message::DebugView(
                    match name {
                        "view-poster" => V::Poster,
                        "view-thumb" => V::Landscape,
                        _ => V::List,
                    },
                    80,
                ))
            }
            "open-movies" => Some(Message::OpenKind(nocturne_core::unified::UKind::Movies)),
            "open-shows" => Some(Message::OpenKind(nocturne_core::unified::UKind::Shows)),
            n if n.starts_with("open-library-") => n[13..].parse().ok().map(Message::OpenLibrary),
            n if n.starts_with("open-global-") => n[12..].parse().ok().map(Message::OpenGlobal),
            _ => None,
        };
        if let Some(msg) = msg {
            return Some(crate::task::after(
                Duration::from_millis(ms.parse().ok()?),
                msg,
            ));
        }
        // `shift+c`: the same key with Shift held (the player's secondary-subtitle cycle).
        let (name, shift) = name
            .strip_prefix("shift+")
            .map_or((name, false), |n| (n, true));
        let key = crate::keys::Key {
            name: name.into(),
            text: (name.chars().count() == 1).then(|| name.into()),
            shift,
            ..Default::default()
        };
        Some(crate::task::after(
            Duration::from_millis(ms.parse().ok()?),
            Message::Key(key),
        ))
    });
    // `NOCTURNE_PAD="pads1@500,down@2000,a@3000"`: gamepad events injected the same way (a pad is not
    // guaranteed on the test machine): `pads<N>` = N connected, else a button name (see pad::parse).
    let pad = std::env::var("NOCTURNE_PAD").unwrap_or_default();
    let pad = pad.split(',').filter_map(|k| {
        let (name, ms) = k.split_once('@')?;
        Some(crate::task::after(
            Duration::from_millis(ms.parse().ok()?),
            Message::Pad(crate::pad::parse(name)?),
        ))
    });
    let snaps = std::env::var_os("NOCTURNE_SNAP").is_some().then(|| {
        delays().into_iter().enumerate().map(|(n, ms)| {
            crate::task::after(Duration::from_millis(ms), n).then(|n| {
                window::oldest()
                    .and_then(window::screenshot)
                    .map(move |s| Message::Snapped(s, n))
            })
        })
    });
    Task::batch(keys.chain(pad).chain(snaps.into_iter().flatten()))
}

#[cfg(not(debug_assertions))]
pub fn on_boot() -> Task<Message> {
    Task::none()
}

/// Capture on demand: `NOCTURNE_SNAP_WATCH=/tmp/req` makes every tick look for that file; when it exists its
/// first line is the output path, the file is removed and a capture is written there.
#[cfg(debug_assertions)]
pub fn poll() -> Task<Message> {
    use iced::window;
    let Some(req) = std::env::var_os("NOCTURNE_SNAP_WATCH") else {
        return Task::none();
    };
    let Ok(text) = std::fs::read_to_string(&req) else {
        return Task::none();
    };
    let _ = std::fs::remove_file(&req);
    *NEXT.lock().unwrap() = Some(text.trim().into());
    window::oldest()
        .and_then(window::screenshot)
        .map(|s| Message::Snapped(s, usize::MAX))
}

#[cfg(not(debug_assertions))]
pub fn poll() -> Task<Message> {
    Task::none()
}

#[cfg(debug_assertions)]
static NEXT: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

#[cfg(debug_assertions)]
pub fn delays() -> Vec<u64> {
    let v: Vec<u64> = std::env::var("NOCTURNE_SNAP_AFTER_MS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if v.is_empty() { vec![1500] } else { v }
}

/// Capture number `n` (0 is the plain path, the rest get `-<n+1>` before the extension).
#[cfg(debug_assertions)]
fn target(base: &str, n: usize) -> std::path::PathBuf {
    let p = std::path::Path::new(base);
    if n == 0 {
        return p.to_path_buf();
    }
    let stem = p
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into());
    let ext = p
        .extension()
        .map_or_else(|| "png".into(), |e| e.to_string_lossy().to_string());
    p.with_file_name(format!("{stem}-{}.{ext}", n + 1))
}

#[cfg(debug_assertions)]
pub fn save(shot: &iced::window::Screenshot, n: usize) {
    let path = if n == usize::MAX {
        let Some(p) = NEXT.lock().unwrap().take() else {
            return;
        };
        p
    } else {
        let Some(base) = std::env::var_os("NOCTURNE_SNAP") else {
            return;
        };
        target(&base.to_string_lossy(), n)
    };
    let r = image::save_buffer(
        &path,
        &shot.rgba,
        shot.size.width,
        shot.size.height,
        image::ColorType::Rgba8,
    );
    match r {
        Ok(()) => eprintln!("snap: wrote {}", path.display()),
        Err(e) => eprintln!("snap: failed: {e}"),
    }
}

#[cfg(not(debug_assertions))]
pub fn save(_: &iced::window::Screenshot, _: usize) {}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn later_captures_get_a_number() {
        assert_eq!(
            target("/tmp/x.png", 0),
            std::path::PathBuf::from("/tmp/x.png")
        );
        assert_eq!(
            target("/tmp/x.png", 1),
            std::path::PathBuf::from("/tmp/x-2.png")
        );
    }
}
