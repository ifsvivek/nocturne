//! Embedded SVG icons (drawn as masks tinted by `text_color`).
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;

macro_rules! icons {
    ($($n:literal),*) => {
        fn find(p: &str) -> Option<&'static [u8]> {
            match p {
                $(concat!("icons/", $n, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $n, ".svg"))),)*
                _ => None,
            }
        }
    };
}
icons!(
    "play",
    "pause",
    "back",
    "fwd",
    "volume",
    "mute",
    "stop",
    "cc",
    "audio",
    "gear",
    "fullscreen"
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(find(path).map(Cow::Borrowed))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
