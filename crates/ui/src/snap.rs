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
        let key = crate::keys::Key {
            name: name.into(),
            text: (name.chars().count() == 1).then(|| name.into()),
            ..Default::default()
        };
        Some(crate::task::after(
            Duration::from_millis(ms.parse().ok()?),
            Message::Key(key),
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
    Task::batch(keys.chain(snaps.into_iter().flatten()))
}

#[cfg(not(debug_assertions))]
pub fn on_boot() -> Task<Message> {
    Task::none()
}

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
    let Some(base) = std::env::var_os("NOCTURNE_SNAP") else {
        return;
    };
    let path = target(&base.to_string_lossy(), n);
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
