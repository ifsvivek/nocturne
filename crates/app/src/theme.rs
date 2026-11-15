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
/// Boundary of an interactive control (chip, button, switch track, avatar): opaque, >= 3:1 on every
/// surface (WCAG 1.4.11). Hairlines stay decorative dividers and panel outlines.
pub fn edge() -> Rgba {
    rgb(0x747a91)
}
/// Quiet filled background of a value control; the edge carries the boundary.
pub fn control() -> Rgba {
    rgb(0x1a1e2c)
}
pub fn control_hover() -> Rgba {
    rgb(0x232838)
}

// Text.
pub fn text() -> Rgba {
    rgb(0xeceef5)
}
pub fn text_secondary() -> Rgba {
    rgb(0xa4aabf)
}
pub fn text_tertiary() -> Rgba {
    rgb(0x8990a9)
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

/// Type size of control values (Settings chips) and the minimum control height.
pub const CONTROL_TEXT: f32 = 13.0;
pub const CONTROL_H: f32 = 32.0;

#[cfg(test)]
mod tests {
    //! WCAG 2.x contrast audit of the tokens: text >= 4.5:1 on every surface it can sit on, control
    //! edges and focus rings >= 3:1. Semi-transparent fills are composited over the real surfaces.
    use super::*;

    type Rgb = [f32; 3];

    fn over(fg: Rgba, bg: Rgb) -> Rgb {
        [
            fg.r * fg.a + bg[0] * (1.0 - fg.a),
            fg.g * fg.a + bg[1] * (1.0 - fg.a),
            fg.b * fg.a + bg[2] * (1.0 - fg.a),
        ]
    }
    fn lum(c: Rgb) -> f32 {
        let l = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
    }
    fn ratio(a: Rgb, b: Rgb) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    fn solid(c: Rgba) -> Rgb {
        [c.r, c.g, c.b]
    }

    /// Every surface a label can sit on: each base, bare and under glass (hover), accent tint
    /// (selected) and danger tint (destructive).
    fn grounds() -> Vec<(String, Rgb)> {
        let mut v = vec![];
        for (n, b) in [
            ("frame", frame()),
            ("surface", surface()),
            ("raised", raised()),
        ] {
            let b = solid(b);
            v.push((n.to_string(), b));
            v.push((format!("{n}+glass"), over(glass(), b)));
            v.push((format!("{n}+tint"), over(accent_tint(), b)));
            v.push((format!("{n}+danger"), over(danger_tint(), b)));
        }
        v.push(("control".into(), solid(control())));
        v.push(("control-hover".into(), solid(control_hover())));
        v
    }

    #[test]
    fn text_tokens_reach_4_5_on_every_surface() {
        let mut bad = vec![];
        for (tn, c) in [
            ("text", text()),
            ("secondary", text_secondary()),
            ("tertiary", text_tertiary()),
            ("accent", accent()),
            ("accent_text", accent_text()),
            ("danger", danger()),
            ("warn", warn()),
        ] {
            for (gn, g) in grounds() {
                let r = ratio(solid(c), g);
                if r < 4.5 {
                    bad.push(format!("{tn} on {gn}: {r:.2}"));
                }
            }
        }
        assert!(bad.is_empty(), "below 4.5:1: {bad:#?}");
    }

    #[test]
    fn labels_on_filled_controls_reach_4_5() {
        for fill in [accent(), accent_hover()] {
            let r = ratio(solid(on_accent()), solid(fill));
            assert!(r >= 4.5, "on_accent on accent fill: {r:.2}");
        }
    }

    #[test]
    fn control_edges_and_focus_rings_reach_3() {
        for (tn, c) in [
            ("edge", edge()),
            ("accent ring", accent()),
            ("danger", danger()),
        ] {
            for (gn, g) in grounds() {
                let r = ratio(solid(c), g);
                assert!(r >= 3.0, "{tn} on {gn}: {r:.2}");
            }
        }
        // The control's own fill must also sit apart from the page so the edge reads as a boundary.
        assert!(ratio(solid(edge()), solid(control_hover())) >= 3.0);
    }

    #[test]
    fn hierarchy_stays_quiet_but_distinct() {
        let (t, s, q) = (
            solid(text()),
            solid(text_secondary()),
            solid(text_tertiary()),
        );
        assert!(lum(t) > lum(s) && lum(s) > lum(q));
        assert!(
            ratio(s, q) >= 1.25,
            "secondary vs tertiary: {:.2}",
            ratio(s, q)
        );
        assert!(ratio(t, s) >= 1.5);
    }

    #[test]
    fn contrast_table() {
        // Run with --nocapture to print the numbers quoted in docs/DESIGN.md.
        for (tn, c) in [
            ("text", text()),
            ("secondary", text_secondary()),
            ("tertiary", text_tertiary()),
            ("accent", accent()),
            ("edge", edge()),
        ] {
            let all: Vec<f32> = grounds().iter().map(|(_, g)| ratio(solid(c), *g)).collect();
            let (lo, hi) = (
                all.iter().cloned().fold(f32::MAX, f32::min),
                all.iter().cloned().fold(0.0, f32::max),
            );
            println!("{tn}: {lo:.2} - {hi:.2}");
        }
    }
}
