//! "Who's watching?": the model lives in `nocturne_core::account::picker`; this is the GPUI view.
use crate::{theme as t, ui};
use gpui::prelude::*;
use gpui::*;
pub use nocturne_core::account::picker::*;
use nocturne_session::Account;
use std::sync::Arc;

/// The screen. `pic` finds a loaded profile picture; `on` receives every click.
pub fn view<T: 'static>(
    p: &Picker,
    pic: &dyn Fn(&Account) -> Option<Arc<Image>>,
    on: fn(&mut T, Act, &mut Context<T>),
    cx: &mut Context<T>,
) -> AnyElement {
    let n_acc = p.accounts.len();
    let tile = |i: usize, label: String, face: Div| {
        let on_it = i == p.sel;
        div()
            .id(("pick-tile", i))
            .w(px(TILE_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(t::S3))
            .cursor_pointer()
            .on_click(cx.listener(move |t, _, _, cx| {
                let a = if i < n_acc { Act::Pick(i) } else { Act::Add };
                on(t, a, cx)
            }))
            .child(
                face.border_2()
                    .border_color(if on_it { t::accent() } else { t::edge() })
                    .when(on_it, |d| {
                        d.shadow(vec![BoxShadow {
                            color: t::glow().into(),
                            offset: point(px(0.0), px(0.0)),
                            blur_radius: px(24.0),
                            spread_radius: px(0.0),
                        }])
                    })
                    .hover(|s| {
                        s.border_color(if on_it {
                            t::accent()
                        } else {
                            t::text_tertiary()
                        })
                    })
                    .font_family(t::MONO)
                    .text_size(px(44.0))
                    .text_color(t::accent_text()),
            )
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(t::BODY_LG))
                    .font_weight(if on_it {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if on_it {
                        t::text()
                    } else {
                        t::text_secondary()
                    })
                    .child(label),
            )
    };
    let mut row = div()
        .w(px(p.cols.min(p.tiles()) as f32 * (TILE_W + GAP) - GAP))
        .flex()
        .flex_wrap()
        .justify_center()
        .gap(px(GAP));
    for (i, a) in p.accounts.iter().enumerate() {
        row = row.child(tile(
            i,
            a.user_name.clone(),
            ui::avatar(PIC, &a.user_name, pic(a)),
        ));
    }
    let add = div()
        .size(px(PIC))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(t::glass())
        .text_color(t::text_secondary())
        .child("+");
    row = row.child(tile(p.accounts.len(), "Add profile".into(), add));
    let adding = p.sel == p.accounts.len();
    let check = div()
        .id("pick-default")
        .flex()
        .items_center()
        .gap(px(t::S2))
        .cursor_pointer()
        .when(adding, |d| d.opacity(0.4))
        .on_click(cx.listener(move |t, _, _, cx| on(t, Act::Toggle, cx)))
        .child(
            div()
                .size(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(if p.checked() { t::accent() } else { t::edge() })
                .when(p.checked(), |d| d.bg(t::accent()))
                .text_size(px(t::CAPTION))
                .text_color(t::on_accent())
                .child(if p.checked() { "\u{2713}" } else { "" }),
        )
        .child(
            div()
                .text_size(px(t::BODY))
                .text_color(t::text_secondary())
                .child("Use as default profile"),
        );
    ui::fade_in(
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(t::S8))
            .child(ui::wordmark(t::HEADING))
            .child(
                div()
                    .text_size(px(36.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t::text())
                    .child("Who's watching?"),
            )
            .child(row)
            .child(check)
            .child(ui::mono(
                "\u{2190} \u{2192} move    ENTER choose    SPACE default profile",
            )),
        "picker".into(),
        t::SLOW,
    )
    .into_any_element()
}
