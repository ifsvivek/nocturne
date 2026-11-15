//! Reusable components. Each returns a plain element; state stays in the caller.
use crate::theme as t;
use gpui::prelude::*;
use gpui::*;
use std::sync::Arc;
use std::time::Duration;

/// GPUI has no letter-spacing, so tracked caps are faked with hair spaces.
pub fn tracked(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.to_uppercase().chars().enumerate() {
        if c == ' ' {
            out.push('\u{2003}'); // em space: visible word gap between tracked letters
            continue;
        }
        if i > 0 && !out.ends_with('\u{2003}') {
            out.push('\u{200A}');
            out.push('\u{200A}');
        }
        out.push(c);
    }
    out
}

pub fn name(s: String) -> ElementId {
    ElementId::Name(s.into())
}

/// Fade-in for things that appear: pages, hero text, loaded art. Restarts when `id` changes.
pub fn fade_in<E: IntoElement + Styled + 'static>(
    el: E,
    id: String,
    dur: Duration,
) -> impl IntoElement {
    el.with_animation(
        name(id),
        Animation::new(dur).with_easing(ease_out_quint()),
        |e, v| e.opacity(v),
    )
}

/// Tracked uppercase label (tagline, eyebrows).
pub fn label(s: &str) -> Div {
    div()
        .text_size(px(t::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t::text_tertiary())
        .child(tracked(s))
}

pub fn wordmark(size: f32) -> Div {
    div()
        .text_size(px(size))
        .font_weight(FontWeight::BOLD)
        .text_color(t::text())
        .child(tracked("Nocturne"))
}

fn glow_shadow(a: f32) -> Vec<BoxShadow> {
    let mut c: Hsla = t::glow().into();
    c.a = a;
    vec![BoxShadow {
        color: c,
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(24.0),
        spread_radius: px(0.0),
    }]
}

/// Tiny monospace annotation (the only text allowed below 12px).
pub fn mono(s: impl Into<SharedString>) -> Div {
    div()
        .font_family(t::MONO)
        .text_size(px(t::MICRO))
        .text_color(t::text_tertiary())
        .child(s.into())
}

/// Outlined status chip: 1px outline, faint tinted fill, mono label.
pub fn chip(s: &str, accent: bool) -> Div {
    div()
        .px(px(t::S2))
        .h(px(20.0))
        .flex()
        .items_center()
        .rounded(px(R_CHIP))
        .border_1()
        .border_color(if accent {
            t::accent()
        } else {
            t::hairline_strong()
        })
        .bg(if accent { t::accent_tint() } else { t::glass() })
        .font_family(t::MONO)
        .text_size(px(t::MICRO))
        .text_color(if accent {
            t::accent_text()
        } else {
            t::text_secondary()
        })
        .child(tracked(s))
}
const R_CHIP: f32 = 4.0;

/// Value control of a Settings row: filled, 1px `edge` outline (>= 3:1), 13px text, `CONTROL_H` tall.
/// `caret` marks a drop-down (flips while `open`); `focused` is the keyboard ring.
pub fn value_chip(
    id: (&'static str, usize),
    value: &str,
    caret: bool,
    focused: bool,
    open: bool,
) -> Stateful<Div> {
    let lit = focused || open;
    div()
        .id(id)
        .h(px(t::CONTROL_H))
        .px(px(t::S3))
        .gap(px(t::S2))
        .flex()
        .items_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(if lit { t::accent() } else { t::edge() })
        .bg(if open { t::accent_tint() } else { t::control() })
        .text_size(px(t::CONTROL_TEXT))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t::text())
        .cursor_pointer()
        .when(!open, |d| {
            d.hover(|s| s.bg(t::control_hover()).border_color(t::text_secondary()))
        })
        .active(|s| s.bg(t::accent_tint()).border_color(t::accent()))
        .when(focused, |d| d.shadow(glow_shadow(0.7)))
        .child(
            div()
                .min_w_0()
                .max_w(px(180.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(value.to_string()),
        )
        .when(caret, |d| {
            d.child(
                svg()
                    .path(if open {
                        "icons/caret_up.svg"
                    } else {
                        "icons/caret_down.svg"
                    })
                    .size(px(12.0))
                    .flex_none()
                    .text_color(if open {
                        t::accent_text()
                    } else {
                        t::text_secondary()
                    }),
            )
        })
}

/// On / off switch: track + knob (on = accent, knob right) and the word ON / OFF beside it so the
/// state never rests on colour alone. Clicks fall through to the row.
pub fn switch(id: (&'static str, usize), on: bool, focused: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(t::CONTROL_H))
        .px(px(t::S2))
        .gap(px(t::S3))
        .flex()
        .items_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(if focused { t::accent() } else { rgba(0) })
        .cursor_pointer()
        .hover(|s| s.bg(t::glass()))
        // Solid fill under the glow, so the ON / OFF word keeps its contrast.
        .when(focused, |d| d.bg(t::raised()).shadow(glow_shadow(0.7)))
        .child(
            div()
                .w(px(28.0))
                .text_size(px(t::CONTROL_TEXT))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on {
                    t::accent_text()
                } else {
                    t::text_secondary()
                })
                .child(if on { "ON" } else { "OFF" }),
        )
        .child(
            div()
                .relative()
                .w(px(40.0))
                .h(px(22.0))
                .flex_none()
                .rounded_full()
                .border_1()
                .border_color(if on { t::accent() } else { t::edge() })
                .bg(if on { t::accent() } else { t::control() })
                .child(
                    div()
                        .absolute()
                        .top(px(2.0))
                        .left(px(if on { 20.0 } else { 2.0 }))
                        .size(px(16.0))
                        .rounded_full()
                        .bg(if on {
                            t::on_accent()
                        } else {
                            t::text_secondary()
                        }),
                ),
        )
}

/// Full-width band: hairline divider below, content spans the window.
pub fn band(content: impl IntoElement) -> Div {
    div()
        .w_full()
        .flex_none()
        .border_b_1()
        .border_color(t::hairline())
        .child(content)
}

/// Section heading: bold title + muted paragraph on the left, mono annotation on the right.
pub fn section(title: &str, text: &str, meta: String) -> Div {
    div()
        .px(px(t::PAGE_PAD))
        .flex()
        .items_end()
        .justify_between()
        .gap(px(t::S6))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(t::S2))
                .child(
                    div()
                        .text_size(px(28.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(t::text())
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .max_w(px(520.0))
                        .text_size(px(t::BODY))
                        .line_height(px(t::BODY * 1.55))
                        .text_color(t::text_secondary())
                        .child(text.to_string()),
                ),
        )
        .child(mono(meta))
}

/// Primary: solid blue, dark label. Secondary: hairline outline. `focused` = soft glow.
pub fn pill_button(
    id: &'static str,
    text: &'static str,
    primary: bool,
    focused: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let mut d = div()
        .id(id)
        .h(px(44.0))
        .px(px(t::S6))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .border_1()
        .gap(px(t::S2))
        .text_size(px(t::BODY))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .active(|s| s.opacity(0.85))
        .on_click(on_click)
        .when(primary, |d| {
            d.child(
                div()
                    .font_family(t::MONO)
                    .text_size(px(t::CAPTION))
                    .child("\u{25B6}"),
            )
        })
        .child(text);
    d = if primary {
        d.bg(if focused {
            t::accent_hover()
        } else {
            t::accent()
        })
        .border_color(t::accent())
        .text_color(t::on_accent())
        .hover(|s| s.bg(t::accent_hover()))
    } else {
        d.bg(if focused { t::accent_tint() } else { rgba(0) })
            .border_color(if focused { t::accent() } else { t::edge() })
            .text_color(t::text())
            .hover(|s| s.bg(t::glass()).border_color(t::text_tertiary()))
    };
    if focused {
        d = d.shadow(glow_shadow(0.7));
    }
    d
}

/// Top-bar text link: active = white with a small blue dot, focused = blue text.
pub fn nav_link(
    id: String,
    text: &str,
    active: bool,
    focused: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(name(id))
        .h(px(t::HEADER_H))
        .px(px(t::S3))
        .relative()
        .flex()
        .items_center()
        .text_size(px(t::BODY))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .text_color(if focused {
            t::accent_text()
        } else if active {
            t::text()
        } else {
            t::text_secondary()
        })
        .hover(|s| s.text_color(t::text()))
        .on_click(on_click)
        .child(text.to_string())
        .when(active || focused, |d| {
            d.child(
                div()
                    .absolute()
                    .bottom(px(10.0))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(div().size(px(4.0)).rounded_full().bg(t::accent())),
            )
        })
}

/// Tooltip bubble for icon-only buttons.
pub struct Tip(pub SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(t::S2))
            .py(px(t::S1))
            .rounded(px(t::R_XS))
            .border_1()
            .border_color(t::hairline_strong())
            .bg(t::raised())
            .font_family(t::MONO)
            .text_size(px(t::MICRO))
            .text_color(t::text())
            .child(self.0.clone())
    }
}

/// `el.tooltip(ui::tip("Back"))`: names an icon-only control for pointer and screen-reader users.
pub fn tip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Tip(text.into())).into()
}

/// Text tab: active = white with a blue underline; `focused` = blue text.
pub fn tab(
    id: String,
    text: &str,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(name(id))
        .h(px(36.0))
        .px(px(t::S3))
        .flex()
        .items_center()
        .border_b_2()
        .border_color(if active { t::accent() } else { rgba(0) })
        .text_size(px(t::BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if active {
            t::text()
        } else {
            t::text_secondary()
        })
        .cursor_pointer()
        .hover(|s| s.text_color(t::text()))
        .on_click(on_click)
        .child(text.to_string())
}

/// Compact text button for admin rows and dialogs. `danger` = red outline (destructive only),
/// `primary` = solid blue, otherwise hairline. `focused` adds the soft glow; `off` dims it.
pub fn action_button(
    id: String,
    text: &str,
    primary: bool,
    danger: bool,
    focused: bool,
    off: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (ink, edge) = match (primary, danger) {
        (_, true) => (t::danger(), t::danger()),
        (true, _) => (t::on_accent(), t::accent()),
        _ => (t::text(), t::edge()),
    };
    let mut d = div()
        .id(name(id))
        .h(px(32.0))
        .px(px(t::S3))
        .flex()
        .items_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(if focused && !danger {
            t::accent()
        } else {
            edge
        })
        .text_size(px(t::CAPTION + 1.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ink)
        .cursor_pointer()
        .on_click(on_click)
        .when(primary && !danger, |d| d.bg(t::accent()))
        .when(danger, |d| d.bg(t::danger_tint()))
        .when(!primary && !danger && focused, |d| d.bg(t::accent_tint()))
        .when(off, |d| d.opacity(0.4))
        .child(text.to_string());
    if focused {
        d = d.shadow(glow_shadow(0.7));
    }
    d
}

/// What an image slot can show.
pub enum Pic {
    Ready(Arc<Image>),
    /// Real image still loading: its blurhash, upscaled.
    Blur(Arc<Image>),
    Loading,
    Missing,
}

pub fn initial(title: &str) -> String {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

/// Round profile picture of `size` px: the picture when there is one, otherwise (loading, failed,
/// none set) the name's initial. Callers add their own id, border colour and click handling.
pub fn avatar(size: f32, name: &str, pic: Option<Arc<Image>>) -> Div {
    let d = div()
        .size(px(size))
        .flex_none()
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(t::raised());
    match pic {
        // The picture rounds its own corners: a parent's overflow clip is a plain rectangle.
        Some(i) => d.child(
            img(i)
                .size_full()
                .rounded_full()
                .object_fit(ObjectFit::Cover),
        ),
        None => d.child(initial(name)),
    }
}

/// Image slot: a loaded image fades in, loading shows a faint breathing skeleton, a missing
/// image shows a quiet gradient with the title's initial. Never a flat empty box.
pub fn tile(pic: Pic, title: &str, key: &str) -> Div {
    let base = div()
        .overflow_hidden()
        .rounded(px(t::R_XS))
        .bg(linear_gradient(
            160.0,
            linear_color_stop(t::raised(), 0.0),
            linear_color_stop(t::surface(), 1.0),
        ));
    match pic {
        Pic::Ready(i) => base.child(fade_in(
            div()
                .size_full()
                .child(img(i).size_full().object_fit(ObjectFit::Cover)),
            format!("img-{key}"),
            t::SLOW,
        )),
        Pic::Blur(i) => base.child(
            div()
                .size_full()
                .child(img(i).size_full().object_fit(ObjectFit::Cover)),
        ),
        Pic::Loading => base.child(
            div().size_full().bg(rgb(0xffffff)).with_animation(
                name(format!("shimmer-{key}")),
                Animation::new(Duration::from_millis(1800))
                    .repeat()
                    .with_easing(ease_in_out),
                |d, v| d.opacity(0.015 + 0.04 * (1.0 - (2.0 * v - 1.0).abs())),
            ),
        ),
        Pic::Missing => base.child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(48.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(0x8fb0ff38))
                .child(initial(title)),
        ),
    }
}

/// Thin progress line: faint track, blue fill. `frac` is 0..1.
pub fn progress_bar(frac: f32, h: f32) -> Div {
    div().w_full().h(px(h)).bg(rgba(0xffffff1f)).child(
        div()
            .h_full()
            .w(relative(frac.clamp(0.0, 1.0)))
            .bg(t::accent()),
    )
}

/// Compact icon button for the now-playing bar (optional tiny mono label beside the icon).
pub fn ctl_button(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let ink = if primary { t::on_accent() } else { t::text() };
    let d = div()
        .id(id)
        .h(px(34.0))
        .min_w(px(36.0))
        .px(px(t::S2))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(t::S1))
        .rounded(px(6.0))
        .border_1()
        .font_family(t::MONO)
        .text_size(px(t::MICRO))
        .text_color(ink)
        .cursor_pointer()
        .active(|s| s.opacity(0.85))
        .on_click(on_click)
        .child(svg().path(icon).size(px(16.0)).flex_none().text_color(ink))
        .when(!label.is_empty(), |d| d.child(label));
    if primary {
        d.bg(t::accent())
            .border_color(t::accent())
            .hover(|s| s.bg(t::accent_hover()))
    } else {
        d.border_color(t::edge()).hover(|s| s.bg(t::glass()))
    }
}

/// Poster in a hairline cell: image on top, caption strip below (title, mono meta), index
/// tag in the image corner. Cells share edges (right + bottom hairline; the container draws
/// top/left). Focus = 1px blue outline + faint glow + brighter image, faded in over 140ms.
pub struct Cell<'a> {
    pub key: &'a str,
    pub pic: Pic,
    pub title: &'a str,
    pub meta: String,
    pub index: usize,
    pub w: f32,
    pub h: f32,
    pub focused: bool,
    /// Watched fraction; draws a thin blue line along the bottom of the image.
    pub progress: Option<f32>,
    /// Small count shown in the image's bottom-right corner (unwatched episodes).
    pub badge: Option<String>,
}

pub fn poster_cell(c: Cell) -> Stateful<Div> {
    let pad = t::S3;
    let mut d =
        div()
            .id(name(format!("cell-{}", c.key)))
            .relative()
            .flex_none()
            .w(px(c.w + 2.0 * pad))
            .p(px(pad))
            .border_r_1()
            .border_b_1()
            .border_color(t::hairline())
            .cursor_pointer()
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .w(px(c.w))
                    .h(px(c.h))
                    .child(tile(c.pic, c.title, c.key).size_full())
                    .when(!c.focused, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .rounded(px(t::R_XS))
                                .bg(rgba(0x07080c40)),
                        )
                    })
                    .child(
                        div().absolute().top(px(t::S2)).left(px(t::S2)).child(
                            mono(format!("0x{:02x}", c.index + 1)).text_color(rgba(0xffffffb0)),
                        ),
                    )
                    .when_some(c.badge.clone(), |d, b| {
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(t::S2))
                                .right(px(t::S2))
                                .px(px(t::S2))
                                .rounded(px(R_CHIP))
                                .bg(t::accent())
                                .font_family(t::MONO)
                                .text_size(px(t::MICRO))
                                .text_color(t::on_accent())
                                .child(b),
                        )
                    })
                    .when_some(c.progress, |d, p| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .w_full()
                                .child(progress_bar(p, 3.0)),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(c.w))
                    .pt(px(t::S3))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(t::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if c.focused {
                                t::text()
                            } else {
                                t::text_secondary()
                            })
                            .child(c.title.to_string()),
                    )
                    .child(div().h(px(16.0)).truncate().child(mono(c.meta))),
            );
    if c.focused {
        d = d.child(fade_in(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .border_1()
                .border_color(t::accent())
                .bg(t::accent_tint())
                .shadow(glow_shadow(0.5)),
            format!("focus-{}", c.key),
            t::HOVER,
        ));
    }
    d
}

/// Faint dot grid, faded toward the edges. Rendered once; drawn very low alpha.
pub fn dots() -> Arc<Image> {
    const W: u32 = 1920;
    const H: u32 = 1080;
    const STEP: u32 = 24;
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in (STEP / 2..H).step_by(STEP as usize) {
        for x in (STEP / 2..W).step_by(STEP as usize) {
            let nx = (x as f32 / W as f32 - 0.5) * 2.0;
            let ny = (y as f32 / H as f32 - 0.35) * 2.0;
            let fade = (1.0 - (nx * nx * 0.6 + ny * ny * 0.9).sqrt()).clamp(0.0, 1.0);
            let a = (fade * 255.0) as u8;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = (((y + dy) * W + x + dx) * 4) as usize;
                buf[i..i + 4].copy_from_slice(&[0xc8, 0xd4, 0xff, a]);
            }
        }
    }
    png(&buf, W, H)
}

/// Barely-there radial blue glow, rendered once (smooth, so a low resolution scales cleanly).
pub fn glow() -> Arc<Image> {
    const W: u32 = 512;
    const H: u32 = 256;
    let mut buf = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let dx = (x as f32 / W as f32 - 0.78) * 1.6;
            let dy = y as f32 / H as f32 - 0.0;
            let r = (dx * dx + dy * dy).sqrt().min(1.0);
            let fade_y = (1.0 - y as f32 / H as f32).powi(2);
            let a = ((1.0 - r).powi(2) * 0.22 * fade_y * 255.0) as u8;
            buf.extend_from_slice(&[0x3b, 0x6b, 0xff, a]);
        }
    }
    png(&buf, W, H)
}

/// Procedural film grain: zero-mean black/white speckle, encoded once at startup.
pub fn grain() -> Arc<Image> {
    const N: u32 = 1024;
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    let mut buf = Vec::with_capacity((N * N * 4) as usize);
    for _ in 0..N * N {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let v = if s & 1 == 0 { 255 } else { 0 };
        buf.extend_from_slice(&[v, v, v, ((s >> 8) & 0xff) as u8]);
    }
    png(&buf, N, N)
}

pub fn png(rgba: &[u8], w: u32, h: u32) -> Arc<Image> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
        .expect("png encode");
    Arc::new(Image::from_bytes(ImageFormat::Png, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test; // gpui::* also exports a `test` macro

    #[test]
    fn tracking_uppercases_and_spaces() {
        assert_eq!(tracked("ab"), "A\u{200A}\u{200A}B");
        assert_eq!(tracked("a b"), "A\u{2003}B");
    }

    #[test]
    fn initial_skips_punctuation() {
        assert_eq!(initial("  \"the end"), "T");
        assert_eq!(initial("..."), "");
    }
}
