//! Player page rendering and input: controls bar (two rows), popovers, skip chip, next-episode card,
//! resume prompt, queue, context menu, trickplay tooltip. State and rules live in `player_view.rs`.
use crate::player_logic as pl;
use crate::player_view::{BAR_H, Drag, PlayerView, Sheet};
use crate::source::fmt_clock;
use crate::theme as t;
use crate::ui;
use crate::video::{self, Key, MenuAct, Playback, Pop, Repeat};
use gpui::prelude::*;
use gpui::*;
use std::time::{Duration, Instant};

struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(t::S3))
            .py(px(t::S2))
            .rounded(px(6.0))
            .bg(t::raised())
            .border_1()
            .border_color(t::hairline_strong())
            .text_size(px(t::CAPTION))
            .text_color(t::text())
            .child(self.0.clone())
    }
}

#[derive(Clone, Copy)]
enum CtxAct {
    Key(Key),
    Pop(Pop),
}

fn panel() -> Div {
    div()
        .rounded(px(t::R_PANEL))
        .bg(rgba(0x0b0d14ee))
        .border_1()
        .border_color(t::hairline_strong())
        .text_size(px(13.0))
        .text_color(t::text())
}

impl PlayerView {
    fn track_frac(&self, x: Pixels, b: Bounds<Pixels>) -> f32 {
        if b.size.width <= px(0.0) {
            0.0
        } else {
            ((x - b.origin.x) / b.size.width).clamp(0.0, 1.0)
        }
    }

    fn drag_move(&mut self, x: Pixels, cx: &mut Context<Self>) {
        match self.drag {
            Some(Drag::Seek(_)) => {
                self.drag = Some(Drag::Seek(self.track_frac(x, self.track_b.get())))
            }
            Some(Drag::Vol(_)) => {
                let f = self.track_frac(x, self.vol_b.get());
                self.drag = Some(Drag::Vol(f));
                self.muted_from = None;
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(f as f64 * 130.0);
                }
            }
            None => return,
        }
        cx.notify();
    }

    fn drag_end(&mut self, cx: &mut Context<Self>) {
        if let (Some(Drag::Seek(f)), Some(d)) = (self.drag.take(), self.dur()) {
            self.seek_to(f as f64 * d);
        }
        cx.notify();
    }

    // ---- input ----

    fn ctx_rows(&self) -> Vec<(&'static str, CtxAct)> {
        use CtxAct::*;
        let mut v = vec![
            (
                if self.paused() { "Play" } else { "Pause" },
                Key(video::Key::Toggle),
            ),
            ("Next", Key(video::Key::Next)),
            ("Previous", Key(video::Key::Prev)),
            ("Queue", Key(video::Key::Queue)),
            ("Subtitles", Pop(video::Pop::Subs)),
            ("Audio", Pop(video::Pop::Audio)),
            ("Settings", Pop(video::Pop::Settings)),
            ("Fullscreen", Key(video::Key::Fullscreen)),
            ("Mini player", Key(video::Key::Mini)),
            ("Screenshot", Key(video::Key::Screenshot)),
            ("Playback info", Key(video::Key::Stats)),
            ("Stop", Key(video::Key::Stop)),
        ];
        if self.ctx.hooks.is_some() {
            v.insert(
                4,
                (
                    if self.fav {
                        "Remove favourite"
                    } else {
                        "Add favourite"
                    },
                    Key(video::Key::Favorite),
                ),
            );
        }
        v
    }

    fn run_ctx(&mut self, a: CtxAct, window: &mut Window, cx: &mut Context<Self>) {
        self.ctx_menu = None;
        match a {
            CtxAct::Key(k) => self.do_key(k, window, cx),
            CtxAct::Pop(p) => self.toggle_pop(p, cx),
        }
        cx.notify();
    }

    fn pop_key(&mut self, k: &str, cx: &mut Context<Self>) -> bool {
        let Some(pop) = self.pop else { return false };
        let items = video::items(pop, &self.tracks, &self.set);
        match k {
            "up" => self.pop_sel = self.pop_sel.saturating_sub(1),
            "down" => self.pop_sel = (self.pop_sel + 1).min(items.len().saturating_sub(1)),
            "left" => {
                if let Some(a) = items.get(self.pop_sel).and_then(|i| i.dec.clone()) {
                    self.pop_act(a, cx);
                }
            }
            "right" | "enter" => {
                if let Some(i) = items.get(self.pop_sel) {
                    self.pop_act(i.act.clone(), cx);
                }
            }
            "escape" => {
                self.pop = pop.is_sub().then_some(Pop::Settings);
                self.pop_sel = if pop.is_sub() {
                    video::settings_index(&self.tracks, &self.set, pop)
                } else {
                    0
                };
            }
            _ => {}
        }
        cx.notify();
        true
    }

    /// Player-page keys. True when consumed.
    pub fn on_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.hide.poke(Instant::now());
        let k = ev.keystroke.key.as_str();
        if self.resume_prompt.is_some() {
            match k {
                "enter" => self.resume_choice(true),
                "escape" | "o" => self.resume_choice(false),
                _ => {}
            }
            cx.notify();
            return true;
        }
        if self.ctx_menu.is_some() {
            let rows = self.ctx_rows();
            match k {
                "up" => self.pop_sel = self.pop_sel.saturating_sub(1),
                "down" => self.pop_sel = (self.pop_sel + 1).min(rows.len() - 1),
                "enter" => {
                    if let Some((_, a)) = rows.get(self.pop_sel) {
                        self.run_ctx(*a, window, cx);
                    }
                }
                _ => self.ctx_menu = None,
            }
            cx.notify();
            return true;
        }
        if self.queue_open {
            match k {
                "up" => self.queue_sel = self.queue_sel.saturating_sub(1),
                "down" => {
                    self.queue_sel = (self.queue_sel + 1).min(self.queue.len().saturating_sub(1))
                }
                "enter" => self.jump(self.queue_sel, cx),
                "escape" | "u" => self.queue_open = false,
                _ => return true,
            }
            cx.notify();
            return true;
        }
        if self.pop_key(k, cx) {
            return true;
        }
        if k == "enter" {
            if let Some(s) = self.active_seg() {
                self.skip(s);
                cx.notify();
                return true;
            }
            if let Some(i) = self.next_card().and(self.next_index()) {
                self.jump(i, cx);
                return true;
            }
        }
        let m = &ev.keystroke.modifiers;
        let Some(key) = video::key(k, m.shift, m.control) else {
            return false;
        };
        self.do_key(key, window, cx);
        cx.notify();
        true
    }

    pub fn do_key(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        let pos = self.pos();
        match key {
            Key::Toggle => self.toggle(),
            Key::Seek(s) => self.seek(s),
            Key::Volume(d) => {
                self.muted_from = None;
                let v = video::bump_volume(self.volume, d);
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(v);
                }
                self.osd(format!("Volume {}%", v.round()));
            }
            Key::Mute => self.mute(),
            Key::Stop => self.close(false, cx),
            Key::Fullscreen => self.toggle_fullscreen(window),
            Key::Back if self.help || self.stats => (self.help, self.stats) = (false, false),
            Key::Back if self.next_since.is_some() && !self.next_dismissed => {
                self.next_dismissed = true;
                self.next_since = None;
                self.osd("Autoplay cancelled");
            }
            Key::Back if self.mini => self.toggle_mini(window),
            Key::Back if window.is_fullscreen() => self.toggle_fullscreen(window),
            Key::Back => self.close(false, cx),
            Key::Next => self.step(1, cx),
            Key::Prev => self.step(-1, cx),
            Key::Queue => {
                self.queue_open = !self.queue_open;
                self.queue_sel = self.qi;
                self.pop = None;
            }
            Key::Speed(d) => self.set_speed(video::step_speed(self.set.speed, d)),
            Key::Frame(d) => {
                if let Some(p) = &self.playback {
                    let _ = p.command(&[if d > 0 {
                        "frame-step"
                    } else {
                        "frame-back-step"
                    }]);
                }
            }
            Key::Chapter(d) => {
                if let Some(t) = video::chapter_target(&self.chapters, pos, d) {
                    self.seek_to(t);
                    let title = video::chapter_at(&self.chapters, t)
                        .map(|i| self.chapters[i].1.clone())
                        .filter(|s| !s.is_empty());
                    self.osd(title.unwrap_or_else(|| "Chapter".into()));
                }
            }
            Key::SubDelay(d) => self.add_delay("sub-delay", "Subtitle", d),
            Key::AudioDelay(d) => self.add_delay("audio-delay", "Audio", d),
            Key::CycleSubs => {
                self.refresh_meta();
                let cur = self
                    .tracks
                    .iter()
                    .find(|t| t.kind == video::TrackKind::Sub && t.selected)
                    .map_or(0, |t| t.id);
                let next = video::next_sub(&self.tracks, cur);
                self.set_track(
                    "sid",
                    next,
                    if next == 0 {
                        "Subtitles off"
                    } else {
                        "Subtitles on"
                    },
                );
            }
            Key::Stats => self.stats = !self.stats,
            Key::Help => self.help = !self.help,
            Key::Screenshot => self.screenshot(),
            Key::Loop => {
                let r = if self.set.repeat == Repeat::One {
                    Repeat::Off
                } else {
                    Repeat::One
                };
                self.set_repeat(r);
            }
            Key::AbLoop => self.pop_act(MenuAct::AbLoop, cx),
            Key::Mini => self.toggle_mini(window),
            Key::Favorite => self.toggle_favorite(),
        }
    }

    // ---- pieces ----

    fn chip_button(
        id: &'static str,
        label: impl Into<SharedString>,
        primary: bool,
        on: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        let d = div()
            .id(id)
            .h(px(36.0))
            .px(px(t::S4))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .on_click(on)
            .child(label.into());
        if primary {
            d.bg(t::accent())
                .border_color(t::accent())
                .text_color(t::on_accent())
                .hover(|s| s.bg(t::accent_hover()))
        } else {
            d.bg(rgba(0x0b0d14ee))
                .border_color(t::hairline_strong())
                .text_color(t::text())
                .hover(|s| s.bg(t::glass()))
        }
    }

    /// Floating prompts above the bar: skip chip, next-episode card.
    fn floaters(&self, shown: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut v = vec![];
        let bottom = if shown { BAR_H + 16.0 } else { 24.0 };
        if let Some(s) = self.active_seg() {
            v.push(
                div()
                    .absolute()
                    .right(px(t::S6))
                    .bottom(px(bottom
                        + if self.next_card().is_some() {
                            150.0
                        } else {
                            0.0
                        }))
                    .child(Self::chip_button(
                        "skip",
                        format!("{}  \u{21B5}", s.label()),
                        true,
                        cx.listener(move |t, _, _, cx| {
                            t.skip(s);
                            cx.notify();
                        }),
                    ))
                    .into_any_element(),
            );
        }
        if let Some((title, left)) = self.next_card() {
            let label = match left {
                Some(n) => format!("Next episode in {n}"),
                None => "Next episode".into(),
            };
            v.push(
                panel()
                    .absolute()
                    .right(px(t::S6))
                    .bottom(px(bottom))
                    .w(px(300.0))
                    .p(px(t::S3))
                    .flex()
                    .flex_col()
                    .gap(px(t::S2))
                    .child(ui::mono(label.to_uppercase()))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(t::S2))
                            .child(Self::chip_button(
                                "next-play",
                                "Play now",
                                true,
                                cx.listener(|t, _, _, cx| {
                                    if let Some(i) = t.next_index() {
                                        t.jump(i, cx);
                                    }
                                }),
                            ))
                            .child(Self::chip_button(
                                "next-cancel",
                                "Cancel",
                                false,
                                cx.listener(|t, _, _, cx| {
                                    t.next_dismissed = true;
                                    t.next_since = None;
                                    cx.notify();
                                }),
                            )),
                    )
                    .into_any_element(),
            );
        }
        v
    }

    fn resume_prompt(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.resume_prompt?;
        Some(
            div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    panel()
                        .occlude()
                        .p(px(t::S6))
                        .flex()
                        .flex_col()
                        .gap(px(t::S4))
                        .items_center()
                        .child(
                            div()
                                .text_size(px(t::BODY_LG))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.media.title.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(t::S3))
                                .child(Self::chip_button(
                                    "resume",
                                    format!("Resume from {}", fmt_clock(r)),
                                    true,
                                    cx.listener(|t, _, _, cx| {
                                        t.resume_choice(true);
                                        cx.notify();
                                    }),
                                ))
                                .child(Self::chip_button(
                                    "restart",
                                    "Start over",
                                    false,
                                    cx.listener(|t, _, _, cx| {
                                        t.resume_choice(false);
                                        cx.notify();
                                    }),
                                )),
                        )
                        .child(ui::mono("Enter to resume, Esc to start over")),
                )
                .into_any_element(),
        )
    }

    fn queue_panel(&self, shown: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.queue_open {
            return None;
        }
        let bottom = if shown { BAR_H + 8.0 } else { 8.0 };
        Some(
            panel()
                .id("queue")
                .occlude()
                .absolute()
                .top(px(t::S6))
                .right(px(t::S6))
                .bottom(px(bottom))
                .w(px(360.0))
                .p(px(t::S2))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .overflow_y_scroll()
                .child(
                    div()
                        .px(px(t::S3))
                        .py(px(t::S2))
                        .child(ui::mono(format!("QUEUE  {}", self.queue.len()))),
                )
                .children(self.queue.iter().enumerate().map(|(i, m)| {
                    let (cur, kb) = (i == self.qi, i == self.queue_sel);
                    let n = m.short_ep();
                    div()
                        .id(("q", i))
                        .px(px(t::S3))
                        .py(px(t::S2))
                        .rounded(px(6.0))
                        .flex()
                        .gap(px(t::S3))
                        .cursor_pointer()
                        .when(cur, |d| d.bg(t::accent()).text_color(t::on_accent()))
                        .when(!cur && kb, |d| d.bg(t::glass()))
                        .when(!cur, |d| d.hover(|s| s.bg(t::glass())))
                        .on_click(cx.listener(move |t, _, _, cx| t.jump(i, cx)))
                        .child(
                            div()
                                .flex_none()
                                .w(px(48.0))
                                .font_family(t::MONO)
                                .text_size(px(t::MICRO))
                                .child(n),
                        )
                        .child(div().truncate().child(m.title.clone()))
                }))
                .into_any_element(),
        )
    }

    fn ctx_panel(&self, win: Size<Pixels>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let at = self.ctx_menu?;
        let h = self.ctx_rows().len() as f32 * 35.0 + 16.0;
        let top = at.y.min(win.height - px(h + 8.0)).max(px(0.0));
        let left = at.x.min(win.width - px(228.0)).max(px(0.0));
        Some(
            panel()
                .id("ctx")
                .occlude()
                .absolute()
                .left(left)
                .top(top)
                .w(px(220.0))
                .p(px(t::S2))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .children(
                    self.ctx_rows()
                        .into_iter()
                        .enumerate()
                        .map(|(i, (label, a))| {
                            div()
                                .id(("ctx", i))
                                .px(px(t::S3))
                                .py(px(6.0))
                                .rounded(px(6.0))
                                .cursor_pointer()
                                .when(i == self.pop_sel, |d| d.bg(t::glass()))
                                .hover(|s| s.bg(t::glass()))
                                .on_click(cx.listener(move |t, _, w, cx| t.run_ctx(a, w, cx)))
                                .child(label)
                        }),
                )
                .into_any_element(),
        )
    }

    /// Top-left OSD chip, stats panel and key help over the video.
    fn overlays(&self) -> Vec<AnyElement> {
        let mut v = vec![];
        if let Some((text, _)) = &self.osd {
            v.push(
                panel()
                    .absolute()
                    .top(px(t::S6))
                    .left(px(t::S6))
                    .px(px(t::S4))
                    .py(px(t::S2))
                    .child(text.clone())
                    .into_any_element(),
            );
        }
        if self.stats {
            v.push(
                panel()
                    .absolute()
                    .top(px(t::S6 + 48.0))
                    .left(px(t::S6))
                    .px(px(t::S4))
                    .py(px(t::S3))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(self.stats_rows.iter().map(|(k, val)| {
                        div()
                            .flex()
                            .gap(px(t::S4))
                            .child(div().w(px(130.0)).text_color(t::text_secondary()).child(*k))
                            .child(val.clone())
                    }))
                    .into_any_element(),
            );
        }
        if self.help {
            let keys = [
                ("Space / click", "Play / pause"),
                ("Left / Right", "Seek 10 s"),
                ("Up / Down", "Volume"),
                ("m", "Mute"),
                ("f / double-click", "Fullscreen"),
                ("Esc", "Close menu, cancel autoplay, leave fullscreen, stop"),
                ("Enter", "Skip intro / play next now"),
                ("n  p", "Next / previous item"),
                ("u", "Queue"),
                ("[ ]  \\", "Speed down / up / reset"),
                (", .", "Frame back / forward"),
                ("PgUp / PgDn", "Previous / next chapter"),
                ("c", "Cycle subtitles"),
                ("z x", "Subtitle delay -/+ 100 ms"),
                ("- =", "Audio delay -/+ 100 ms"),
                ("a", "A-B loop"),
                ("l", "Repeat one"),
                ("h", "Favourite"),
                ("i", "Playback info"),
                ("Shift+M", "Mini player"),
                ("Right-click", "Context menu"),
                ("Ctrl+S", "Screenshot"),
                ("s", "Stop"),
                ("?", "This help"),
            ];
            v.push(
                panel()
                    .absolute()
                    .top(px(t::S6 + 48.0))
                    .right(px(t::S6))
                    .px(px(t::S4))
                    .py(px(t::S3))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(keys.iter().map(|(k, d)| {
                        div()
                            .flex()
                            .gap(px(t::S4))
                            .child(
                                div()
                                    .w(px(130.0))
                                    .font_family(t::MONO)
                                    .text_color(t::accent_text())
                                    .child(*k),
                            )
                            .child(*d)
                    }))
                    .into_any_element(),
            );
        }
        v
    }

    fn popover(&self, pop: Pop, cx: &mut Context<Self>) -> Stateful<Div> {
        let rows = video::items(pop, &self.tracks, &self.set);
        panel()
            .id("np-pop")
            .occlude()
            .absolute()
            .bottom(px(BAR_H + 6.0))
            .right(px(t::S6))
            .w(px(360.0))
            .max_h(px(420.0))
            .overflow_y_scroll()
            .p(px(t::S2))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .child(ui::mono(pop.title().to_uppercase())),
            )
            .children(rows.into_iter().enumerate().map(|(i, it)| {
                let act = it.act.clone();
                let kb = i == self.pop_sel;
                let stepper = it.dec.clone();
                let mut row = div()
                    .id(("pop-row", i))
                    .px(px(t::S3))
                    .py(px(t::S2))
                    .rounded(px(6.0))
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap(px(t::S3))
                    .text_size(px(13.0))
                    .cursor_pointer()
                    .when(it.on, |d| d.bg(t::accent()).text_color(t::on_accent()))
                    .when(!it.on && kb, |d| d.bg(t::glass()))
                    .when(!it.on, |d| d.hover(|s| s.bg(t::glass())));
                if stepper.is_none() {
                    row = row.on_click(cx.listener(move |t, _, _, cx| t.pop_act(act.clone(), cx)));
                }
                let right = match stepper {
                    Some(dec) => {
                        let inc = it.act.clone();
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(t::S2))
                            .text_color(t::text_secondary())
                            .child(Self::step_btn(("dec", i), "\u{2212}", dec, cx))
                            .child(div().min_w(px(44.0)).text_center().child(it.value))
                            .child(Self::step_btn(("inc", i), "+", inc, cx))
                    }
                    None => div()
                        .flex_none()
                        .text_color(if it.on {
                            t::on_accent()
                        } else {
                            t::text_secondary()
                        })
                        .child(it.value),
                };
                row.child(div().truncate().child(it.label)).child(right)
            }))
    }

    fn step_btn(
        id: (&'static str, usize),
        glyph: &'static str,
        act: MenuAct,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .border_1()
            .border_color(t::hairline_strong())
            .text_color(t::text())
            .hover(|s| s.bg(t::glass()))
            .child(glyph)
            .on_click(cx.listener(move |t, _, _, cx| t.pop_act(act.clone(), cx)))
    }

    /// Scrub tooltip: trickplay thumbnail when we have it, else just the time.
    fn scrub_tip(&self, f: f32, d: f64, win_w: f32) -> Div {
        let secs = f as f64 * d;
        let mut text = fmt_clock(secs);
        if let Some(i) = video::chapter_at(&self.chapters, secs)
            && !self.chapters[i].1.is_empty()
        {
            text = format!("{text} \u{00B7} {}", self.chapters[i].1);
        }
        let b = self.track_b.get();
        let mut col = div()
            .absolute()
            .bottom(px(BAR_H + 4.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0));
        let mut w = 60.0;
        if let Some((_, info)) = &self.trick
            && let Some(tile) = pl::tile_at(info, secs)
            && let Some(Sheet::Ready(img_)) = self.sheets.get(&tile.sheet)
        {
            let k = 200.0 / info.width as f32;
            let (sw, sh) = pl::sheet_dims(info);
            w = 200.0;
            col = col.child(
                div()
                    .w(px(info.width as f32 * k))
                    .h(px(info.height as f32 * k))
                    .overflow_hidden()
                    .relative()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(t::hairline_strong())
                    .child(
                        img(img_.clone())
                            .absolute()
                            .left(px(-(tile.x as f32) * k))
                            .top(px(-(tile.y as f32) * k))
                            .w(px(sw as f32 * k))
                            .h(px(sh as f32 * k)),
                    ),
            );
        }
        let x = f32::from(b.origin.x + b.size.width * f) - w / 2.0;
        col.left(px(x.clamp(8.0, (win_w - w - 8.0).max(8.0))))
            .child(
                div()
                    .px(px(t::S2))
                    .py(px(3.0))
                    .rounded(px(4.0))
                    .bg(t::raised())
                    .border_1()
                    .border_color(t::hairline_strong())
                    .child(ui::mono(text).text_color(t::text())),
            )
    }

    fn bar(&mut self, win_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let (pos0, dur) = (self.pos(), self.dur());
        let mut pos = pos0;
        let paused = self.paused();
        let mut vol = self.state.as_ref().map_or(100.0, |s| s.volume);
        let mut frac = self.frac();
        if let (Some(Drag::Seek(f)), Some(d)) = (self.drag, dur) {
            (frac, pos) = (f, f as f64 * d);
        }
        if let Some(Drag::Vol(f)) = self.drag {
            vol = f as f64 * 130.0;
        }
        if self.hover_frac.is_some()
            && let (Some(f), Some(_)) = (self.hover_frac, dur)
        {
            let secs = f as f64 * dur.unwrap_or(0.0);
            // Fetching is a side effect of looking: cheap, once per sheet.
            let me = cx.entity();
            cx.defer(move |cx| me.update(cx, |t, cx| t.want_sheet(secs, cx)));
        }
        let tip = self
            .hover_frac
            .zip(dur)
            .map(|(f, d)| self.scrub_tip(f, d, win_w));
        let status = if !self.loaded {
            "loading".to_string()
        } else if self.ended {
            "finished".to_string()
        } else {
            let mut parts = vec![
                self.media.series_name.clone().unwrap_or_default(),
                self.media.episode_label(),
            ];
            if self.set.method != "Direct Play" {
                parts.push(self.set.method.to_string());
            }
            parts
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" \u{00B7} ")
        };
        let right_time = match dur {
            Some(d) if !self.show_total => format!("-{}", fmt_clock((d - pos).max(0.0))),
            Some(d) => fmt_clock(d),
            None => "--:--".into(),
        };
        let ends = dur
            .filter(|_| self.loaded && !self.ended)
            .map(|d| video::ends_at(video::local_hm(), (d - pos).max(0.0), self.set.speed));
        let cache_frac = self
            .cache_end
            .zip(dur)
            .map_or(0.0, |(c, d)| (c / d).clamp(0.0, 1.0) as f32);
        let tick_fracs: Vec<f32> = dur
            .map(|d| {
                self.chapters
                    .iter()
                    .filter(|c| c.0 > 0.5)
                    .map(|c| (c.0 / d) as f32)
                    .collect()
            })
            .unwrap_or_default();
        let seg_marks: Vec<(f32, f32)> = dur
            .map(|d| {
                self.segs
                    .iter()
                    .map(|s| ((s.start / d) as f32, ((s.end - s.start) / d) as f32))
                    .collect()
            })
            .unwrap_or_default();

        let tb = self.track_b.clone();
        let big = self.bar_hover || matches!(self.drag, Some(Drag::Seek(_)));
        let th = if big { 8.0 } else { 4.0 };
        let track = div()
            .id("np-track")
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(16.0))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .top(px(6.0))
                    .left_0()
                    .w_full()
                    .child(ui::progress_bar(frac, th)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(6.0))
                    .left(relative(frac))
                    .w(relative((cache_frac - frac).max(0.0)))
                    .h(px(th))
                    .bg(rgba(0xffffff33)),
            )
            .children(seg_marks.into_iter().map(|(l, w)| {
                div()
                    .absolute()
                    .top(px(th + 7.0))
                    .left(relative(l))
                    .w(relative(w))
                    .h(px(2.0))
                    .bg(t::accent())
            }))
            .children(tick_fracs.into_iter().map(|f| {
                div()
                    .absolute()
                    .top(px(6.0))
                    .left(relative(f))
                    .w(px(2.0))
                    .h(px(th))
                    .bg(rgba(0x07080cd0))
            }))
            .child(
                canvas(move |b, _, _| tb.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_hover(cx.listener(|t, h: &bool, _, cx| {
                t.bar_hover = *h;
                if !*h {
                    t.hover_frac = None;
                }
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|t, e: &MouseMoveEvent, _, cx| {
                t.hover_frac = Some(t.track_frac(e.position.x, t.track_b.get()));
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, e: &MouseDownEvent, _, cx| {
                    t.drag = Some(Drag::Seek(t.track_frac(e.position.x, t.track_b.get())));
                    cx.notify();
                }),
            );

        let can_prev = self.prev_index().is_some();
        let can_next = self.next_index().is_some();
        let mini = self.mini;
        let mut ctl = div().flex().items_center().gap(px(t::S2));
        if !mini {
            ctl = ctl
                .child(
                    ui::ctl_button(
                        "np-prev",
                        "icons/prev.svg",
                        "",
                        false,
                        cx.listener(|t, _, _, cx| t.step(-1, cx)),
                    )
                    .when(!can_prev, |d| d.opacity(0.4)),
                )
                .child(ui::ctl_button(
                    "np-back",
                    "icons/back.svg",
                    "10",
                    false,
                    cx.listener(|t, _, _, cx| {
                        t.seek(-10.0);
                        cx.notify();
                    }),
                ));
        }
        ctl = ctl.child(ui::ctl_button(
            "np-pause",
            if paused {
                "icons/play.svg"
            } else {
                "icons/pause.svg"
            },
            "",
            true,
            cx.listener(|t, _, _, cx| {
                t.toggle();
                cx.notify();
            }),
        ));
        if !mini {
            ctl = ctl
                .child(ui::ctl_button(
                    "np-fwd",
                    "icons/fwd.svg",
                    "30",
                    false,
                    cx.listener(|t, _, _, cx| {
                        t.seek(30.0);
                        cx.notify();
                    }),
                ))
                .child(
                    ui::ctl_button(
                        "np-next",
                        "icons/next.svg",
                        "",
                        self.ended && can_next,
                        cx.listener(|t, _, _, cx| t.step(1, cx)),
                    )
                    .when(!can_next, |d| d.opacity(0.4)),
                );
        }
        let vb = self.vol_b.clone();
        let volume = div()
            .flex()
            .items_center()
            .gap(px(t::S2))
            .child(ui::ctl_button(
                "np-mute",
                if vol <= 0.0 {
                    "icons/mute.svg"
                } else {
                    "icons/volume.svg"
                },
                "",
                false,
                cx.listener(|t, _, _, cx| {
                    t.mute();
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .id("np-vol")
                    .relative()
                    .w(px(84.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .w_full()
                            .child(ui::progress_bar((vol / 130.0) as f32, 3.0)),
                    )
                    .child(
                        canvas(move |b, _, _| vb.set(b), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|t, e: &MouseDownEvent, _, cx| {
                            t.drag = Some(Drag::Vol(0.0));
                            t.drag_move(e.position.x, cx);
                        }),
                    ),
            )
            .when(vol > 100.0, |d| {
                d.child(ui::mono(format!("{}%", vol.round())))
            });
        let video = self.video;
        let has_hooks = self.ctx.hooks.is_some();
        let fav = self.fav;
        let heart = ui::ctl_button(
            "np-fav",
            if fav {
                "icons/heart_on.svg"
            } else {
                "icons/heart.svg"
            },
            "",
            fav,
            cx.listener(|t, _, _, cx| {
                t.toggle_favorite();
                cx.notify();
            }),
        )
        .when(!has_hooks, |d| {
            d.opacity(0.4)
                .tooltip(|_, cx| cx.new(|_| Tip("Sign in to use favourites".into())).into())
        });
        let mut menus = div().flex().items_center().gap(px(t::S2));
        if !mini {
            menus = menus
                .child(heart)
                .child(
                    ui::ctl_button(
                        "np-queue",
                        "icons/queue.svg",
                        "",
                        self.queue_open,
                        cx.listener(|t, _, _, cx| {
                            t.queue_open = !t.queue_open;
                            t.queue_sel = t.qi;
                            t.pop = None;
                            cx.notify();
                        }),
                    )
                    .when(self.queue.len() <= 1, |d| d.opacity(0.5)),
                )
                .child(ui::ctl_button(
                    "np-cc",
                    "icons/cc.svg",
                    "",
                    self.pop == Some(Pop::Subs),
                    cx.listener(|t, _, _, cx| t.toggle_pop(Pop::Subs, cx)),
                ))
                .child(ui::ctl_button(
                    "np-audio",
                    "icons/audio.svg",
                    "",
                    self.pop == Some(Pop::Audio),
                    cx.listener(|t, _, _, cx| t.toggle_pop(Pop::Audio, cx)),
                ))
                .child(volume)
                .child(ui::ctl_button(
                    "np-gear",
                    "icons/gear.svg",
                    "",
                    self.pop.is_some_and(|p| p.in_settings()),
                    cx.listener(|t, _, _, cx| t.toggle_pop(Pop::Settings, cx)),
                ));
        }
        menus = menus.child(ui::ctl_button(
            "np-mini",
            "icons/pip.svg",
            "",
            mini,
            cx.listener(|t, _, w, cx| {
                t.toggle_mini(w);
                cx.notify();
            }),
        ));
        if video && !mini {
            menus = menus.child(ui::ctl_button(
                "np-fs",
                "icons/fullscreen.svg",
                "",
                false,
                cx.listener(|t, _, w, cx| {
                    t.toggle_fullscreen(w);
                    cx.notify();
                }),
            ));
        }
        menus = menus.child(ui::ctl_button(
            "np-stop",
            "icons/stop.svg",
            "",
            false,
            cx.listener(|t, _, _, cx| t.close(false, cx)),
        ));
        let mut glow = t::glow();
        glow.a = 0.10;
        let title = self.media.title.clone();
        let row1 = div()
            .h(px(52.0))
            .flex_none()
            .px(px(t::S6))
            .flex()
            .items_center()
            .gap(px(t::S4))
            .child(ctl)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(t::S3))
                    .child(ends.map_or_else(div, |e| ui::mono(e).flex_none()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t::text())
                            .child(title),
                    )
                    .child(ui::mono(status).truncate()),
            )
            .child(menus);
        let row2 = div()
            .h(px(36.0))
            .flex_none()
            .px(px(t::S6))
            .flex()
            .items_center()
            .gap(px(t::S3))
            .child(ui::mono(fmt_clock(pos)).flex_none().w(px(52.0)))
            .child(track)
            .child(
                div()
                    .id("np-remaining")
                    .flex_none()
                    .w(px(60.0))
                    .cursor_pointer()
                    .child(ui::mono(right_time).text_right())
                    .on_click(cx.listener(|t, _, _, cx| {
                        t.show_total = !t.show_total;
                        cx.notify();
                    })),
            );
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .w_full()
            .h(px(BAR_H))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(t::hairline_strong())
            .occlude()
            .bg(rgba(if self.video { 0x07080cc4 } else { 0x07080cf5 }))
            .shadow(vec![BoxShadow {
                color: glow.into(),
                offset: point(px(0.0), px(-10.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .child(row1)
            .child(row2)
            .when_some(tip, |d, tip| d.child(tip))
            .when_some(self.pop, |d, p| d.child(self.popover(p, cx)))
            .into_any_element()
    }
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending.is_some() {
            self.start_embedded(window, cx);
        }
        let shown = self.controls_visible();
        self.ctl_shown = shown;
        let view = cx.entity();
        let win_w = f32::from(window.viewport_size().width);
        let bar = shown.then(|| self.bar(win_w, cx));
        let mut page = div()
            .id("player")
            .size_full()
            .relative()
            // Black until the first frame is on its way (else the desktop shows through the hole); always on external.
            .when(!self.loaded || !self.video, |d| d.bg(rgb(0x000000)))
            .when(!shown, |d| d.cursor(CursorStyle::None))
            .on_mouse_move(cx.listener(|t, e: &MouseMoveEvent, _, cx| {
                t.hide.poke(Instant::now());
                if !t.ctl_shown {
                    t.ctl_shown = true;
                    cx.notify();
                }
                t.drag_move(e.position.x, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|t, _, _, cx| {
                    if t.drag.is_some() {
                        t.drag_end(cx);
                    }
                }),
            )
            .child(
                canvas(
                    |b, _, _| b,
                    move |b, _, window, cx| {
                        let sf = window.scale_factor();
                        view.update(cx, |t, cx| {
                            if t.vbounds == Some(b) {
                                return;
                            }
                            t.vbounds = Some(b);
                            if let Some(Playback::Embedded(p)) = t.playback.as_mut() {
                                let f = |v: Pixels| f32::from(v);
                                p.set_bounds(
                                    f(b.origin.x),
                                    f(b.origin.y),
                                    f(b.size.width),
                                    f(b.size.height),
                                    sf,
                                );
                                cx.notify(); // parent commit applies the new position
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id("video-hit")
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|t, e: &MouseDownEvent, w, cx| {
                            t.ctx_menu = None;
                            t.pop = None;
                            t.queue_open = false;
                            if t.resume_prompt.is_some() {
                                return;
                            }
                            // Each press toggles pause; the second press of a double-click undoes the first and goes fullscreen.
                            t.toggle();
                            if e.click_count == 2 {
                                t.toggle_fullscreen(w);
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|t, e: &MouseDownEvent, _, cx| {
                            t.pop = None;
                            t.queue_open = false;
                            t.pop_sel = 0;
                            t.ctx_menu = Some(e.position);
                            cx.notify();
                        }),
                    ),
            );
        if !self.video && self.playback.is_some() {
            page = page.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(t::BODY))
                    .text_color(t::text_secondary())
                    .child("Playing in the mpv window"),
            );
        }
        for o in self.overlays() {
            page = page.child(o);
        }
        for f in self.floaters(shown, cx) {
            page = page.child(f);
        }
        if let Some(q) = self.queue_panel(shown, cx) {
            page = page.child(q);
        }
        if let Some(b) = bar {
            page = page.child(b);
        }
        if let Some(c) = self.ctx_panel(window.viewport_size(), cx) {
            page = page.child(c);
        }
        if let Some(r) = self.resume_prompt(cx) {
            page = page.child(r);
        }
        if let Some((n, _)) = &self.note {
            page = page.child(
                panel()
                    .absolute()
                    .top(px(t::S6))
                    .right(px(t::S6))
                    .max_w(px(360.0))
                    .px(px(t::S4))
                    .py(px(t::S3))
                    .bg(t::raised())
                    .child(n.clone()),
            );
        }
        let _ = Duration::ZERO;
        page
    }
}
