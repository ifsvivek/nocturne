//! Design tokens: near-black base, hairline structure, one electric blue accent, monochrome
//! chrome (library art is the only rich colour). Dark only. Raw hex lives here and nowhere else.
use gpui::{Rgba, rgb, rgba};
use std::time::Duration;

pub const FONT: &str = "Noto Sans"; // ponytail: system fonts; bundle faces when branding needs it
pub const MONO: &str = "JetBrainsMono Nerd Font";

// Surfaces.
pub fn frame() -> Rgba {
    rgb(0x07080c)
}
pub fn surface() -> Rgba {
    rgb(0x0c0e16)
}
pub fn raised() -> Rgba {
    rgb(0x12151f)
}

// Structure: hairlines ~8% white, faint glass fill.
pub fn hairline() -> Rgba {
    rgba(0xffffff14)
}
pub fn hairline_strong() -> Rgba {
    rgba(0xffffff29)
}
pub fn glass() -> Rgba {
    rgba(0xffffff0a)
}

// Text.
pub fn text() -> Rgba {
    rgb(0xeceef5)
}
pub fn text_secondary() -> Rgba {
    rgb(0x8a90a6)
}
pub fn text_tertiary() -> Rgba {
    rgb(0x666c82)
}

// Single accent: electric blue.
pub fn accent() -> Rgba {
    rgb(0x4c8dff)
}
pub fn accent_hover() -> Rgba {
    rgb(0x6ba0ff)
}
pub fn accent_text() -> Rgba {
    rgb(0x9cc0ff)
}
pub fn accent_tint() -> Rgba {
    rgba(0x4c8dff1f)
}
pub fn glow() -> Rgba {
    rgba(0x4c8dff59)
}
pub fn on_accent() -> Rgba {
    rgb(0x050816)
}

// Destructive confirmations and warnings only; never decoration.
pub fn danger() -> Rgba {
    rgb(0xf0716f)
}
pub fn danger_tint() -> Rgba {
    rgba(0xf0716f1f)
}
pub fn warn() -> Rgba {
    rgb(0xe3b341)
}

// Type scale (px). Only mono micro-annotations may use MICRO; everything else >= 12.
pub const MICRO: f32 = 11.0;
pub const CAPTION: f32 = 12.0;
pub const BODY: f32 = 15.0;
pub const BODY_LG: f32 = 16.0;
pub const HEADING: f32 = 22.0;
pub const DISPLAY: f32 = 56.0;

// Spacing (4px scale) and radii: panels ~10, cells square-ish.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S6: f32 = 24.0;
pub const S8: f32 = 32.0;
pub const R_XS: f32 = 3.0;
pub const R_PANEL: f32 = 10.0;
pub const PAGE_PAD: f32 = 48.0;
pub const HEADER_H: f32 = 56.0;

// Motion: short, ease-out, never gratuitous.
pub const HOVER: Duration = Duration::from_millis(140);
pub const BASE: Duration = Duration::from_millis(220);
pub const SLOW: Duration = Duration::from_millis(300);
