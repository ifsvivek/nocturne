//! The player's controls bar as an element tree for the theme engine (feature `theme-engine`): `.player-controls` with two rows, `.player-btn` buttons (`.button .button-icon`, `data-action`), the clock, title and status leaves, and slots for the pieces that are real Iced widgets (the svg icons, the seek track and the volume track, which drag). The video hole, the click layer, the loading overlay and the pop-overs stay what they were: the Hole is the bottom layer of the player's stack exactly as before, and the video is a subsurface below the window, so this tree only replaces the bar above it. The bar and its buttons are marked `essential`: a theme can restyle them but cannot hide them. The tree is built while the view is (a pure function of the player's state; the hover and press state lives in `BarHost`), so it can never show stale numbers. `NOCTURNE_ENGINE_UI=0` shows the hand-written bar.
use super::controls::{self, bar, facts, glyph};
use super::{Act, BAR_H, Msg, Player};
use crate::theme as t;
use crate::widget::styled::{Styled, StyledEvent};
use crate::widget::styled_preview::shape;
use iced::widget::{container, mouse_area, opaque};
use iced::{Color, Element, Length, Task};
use nocturne_core::video::Pop;
use nocturne_style::{Builders, Doc, Engine, Id, Interaction, MeasureCache, Scene};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A real Iced element hosted at a node.
#[derive(Clone, Copy)]
pub(super) enum PSlot {
    Icon { name: &'static str, color: Color },
    Seek,
    Volume,
}

pub(super) struct BarTree {
    doc: Doc,
    acts: HashMap<Id, Msg>,
    slots: Vec<PSlot>,
}

impl BarTree {
    fn slot(&mut self, n: Id, s: PSlot) {
        self.slots.push(s);
        self.doc.slot(n, self.slots.len() as u32);
    }
}

pub(super) struct BarHost {
    engine: Engine,
    sheet: String,
    user: Option<String>,
    cache: MeasureCache,
    it: Interaction,
    scene: Option<Rc<Scene>>,
    acts: HashMap<Id, Msg>,
}

impl BarHost {
    pub(super) fn new() -> Option<RefCell<Self>> {
        if std::env::var("NOCTURNE_ENGINE_UI").is_ok_and(|v| v == "0") {
            return None;
        }
        let sheet = sheet();
        let user = std::env::var("NOCTURNE_ENGINE_CSS")
            .ok()
            .filter(|_| cfg!(debug_assertions))
            .and_then(|p| std::fs::read_to_string(p).ok());
        let mut engine = Engine::new(&sheet);
        engine.set_user_css(user.as_deref());
        Some(RefCell::new(Self {
            engine,
            sheet,
            user,
            cache: MeasureCache::new(),
            it: Interaction {
                focus_visible: false,
                ..Interaction::default()
            },
            scene: None,
            acts: HashMap::new(),
        }))
    }
}

fn sheet() -> String {
    let k = t::active();
    let a = |c: Color, a: f32| t::css_color(Color { a, ..c });
    let mut s = crate::app::engine_ui::sheet_css();
    s += &format!(
        ":root {{ --nocturne-frame-77: {}; --nocturne-frame-96: {}; --nocturne-glow-10: {}; }}\n",
        a(k.frame, 0.77),
        a(k.frame, 0.96),
        a(k.glow, 0.10)
    );
    s
}

fn px(v: f32) -> String {
    format!("{}px", (v * 1000.0).round() / 1000.0)
}

fn fill_css(f: &nocturne_core::customize::Fill) -> String {
    use nocturne_core::customize::Fill;
    let rgba = |c: nocturne_core::customize::Color| {
        let [r, g, b, a] = c.to_f32();
        t::css_color(Color { r, g, b, a })
    };
    match f {
        Fill::Solid(c) => rgba(*c),
        Fill::Linear { angle, stops } => {
            let s: Vec<String> = stops
                .iter()
                .map(|(c, at)| format!("{} {}%", rgba(*c), (at * 1000.0).round() / 10.0))
                .collect();
            format!("linear-gradient({angle}deg, {})", s.join(", "))
        }
    }
}

impl Player {
    fn bar_tree(&self) -> BarTree {
        let f = facts(self);
        let mut doc = Doc::new("page");
        doc.attr(0, "data-screen", "player");
        doc.attr(0, "data-video", if self.video { "true" } else { "false" });
        doc.nodes[0].classes.push("player-controls".into());
        doc.essential(0);
        let mut css = format!("height: {}", px(BAR_H));
        if let Some(bg) = t::parts(None, None).player_controls.background.as_ref() {
            css += &format!("; background: {}", fill_css(bg));
        }
        doc.inline(0, &css);
        let mut tr = BarTree {
            doc,
            acts: HashMap::new(),
            slots: vec![],
        };
        let row1 = tr.doc.add(0, "div.player-row.player-row-1");
        let ctl = tr.doc.add(row1, "div.player-transport");
        let (can_prev, can_next) = (self.prev_index().is_some(), self.next_index().is_some());
        if !self.mini {
            self.button(
                &mut tr,
                ctl,
                "prev",
                "prev",
                "",
                false,
                !can_prev,
                Act::Step(-1),
            );
            self.button(
                &mut tr,
                ctl,
                "back",
                "back",
                "10",
                false,
                false,
                Act::Seek(-10.0),
            );
        }
        let (name, label) = if f.paused {
            ("play", "play")
        } else {
            ("pause", "pause")
        };
        self.button(&mut tr, ctl, label, name, "", true, false, Act::Toggle);
        if !self.mini {
            self.button(
                &mut tr,
                ctl,
                "fwd",
                "fwd",
                "30",
                false,
                false,
                Act::Seek(30.0),
            );
            self.button(
                &mut tr,
                ctl,
                "next",
                "next",
                "",
                self.ended && can_next,
                !can_next,
                Act::Step(1),
            );
        }
        let info = tr.doc.add(row1, "div.player-info");
        if let Some(e) = &f.ends {
            let n = tr.doc.add(info, "div.player-ends");
            tr.doc.text(n, e);
        }
        let ti = tr.doc.add(info, "div.player-title");
        tr.doc.text(ti, &self.media.title);
        let st = tr.doc.add(info, "div.player-status");
        tr.doc.text(st, &f.status);
        let menus = tr.doc.add(row1, "div.player-menus");
        if !self.mini {
            let can_fav = self.can_favorite();
            self.button(
                &mut tr,
                menus,
                "favourite",
                if self.fav { "heart_on" } else { "heart" },
                "",
                self.fav,
                !can_fav,
                Act::Fav,
            );
            self.button(
                &mut tr,
                menus,
                "queue",
                "queue",
                "",
                self.queue_open,
                self.queue.len() <= 1,
                Act::Queue,
            );
            self.button(
                &mut tr,
                menus,
                "subtitles",
                "cc",
                "",
                self.pop == Some(Pop::Subs),
                false,
                Act::Pop(Pop::Subs),
            );
            self.button(
                &mut tr,
                menus,
                "audio",
                "audio",
                "",
                self.pop == Some(Pop::Audio),
                false,
                Act::Pop(Pop::Audio),
            );
            let vol = tr.doc.add(menus, "div.player-volume");
            self.button(
                &mut tr,
                vol,
                "mute",
                if f.vol <= 0.0 { "mute" } else { "volume" },
                "",
                false,
                false,
                Act::Mute,
            );
            let v = tr.doc.add(vol, "div.player-vol-track");
            tr.slot(v, PSlot::Volume);
            if f.vol > 100.0 {
                let n = tr.doc.add(vol, "div.player-vol-pct");
                tr.doc.text(n, &format!("{}%", f.vol.round()));
            }
            self.button(
                &mut tr,
                menus,
                "settings",
                "gear",
                "",
                self.pop.is_some_and(|x| x.in_settings()),
                false,
                Act::Pop(Pop::Settings),
            );
        }
        self.button(
            &mut tr,
            menus,
            "mini",
            "pip",
            "",
            self.mini,
            false,
            Act::Mini,
        );
        if self.video && !self.mini {
            self.button(
                &mut tr,
                menus,
                "fullscreen",
                "fullscreen",
                "",
                false,
                false,
                Act::Fullscreen,
            );
        }
        self.button(&mut tr, menus, "stop", "stop", "", false, false, Act::Stop);
        let row2 = tr.doc.add(0, "div.player-row.player-row-2");
        let clock = tr.doc.add(row2, "div.player-clock");
        tr.doc.text(clock, &nocturne_core::source::fmt_clock(f.pos));
        let seek = tr.doc.add(row2, "div.player-seek");
        tr.slot(seek, PSlot::Seek);
        let remain = tr.doc.add(row2, "a.player-remain");
        let l = tr.doc.add(remain, "span.player-remain-label");
        tr.doc.text(l, &f.right_time);
        tr.acts.insert(remain, Msg::Act(Act::ShowTotal));
        tr.doc.essential(remain);
        tr.doc.add(0, "div.player-frame");
        tr
    }

    /// One icon button of the bar. `lit`: the play button or an open menu; `dim`: unavailable but shown.
    #[allow(clippy::too_many_arguments)]
    fn button(
        &self,
        tr: &mut BarTree,
        parent: Id,
        action: &str,
        icon: &'static str,
        label: &str,
        lit: bool,
        dim: bool,
        act: Act,
    ) {
        let mut class = String::from("a.button.button-icon.player-btn");
        if lit {
            class += ".button-primary.primary";
        }
        if dim {
            class += ".dim";
        }
        if !label.is_empty() {
            class += ".labeled";
        }
        let b = tr.doc.add(parent, &class);
        tr.doc.attr(b, "data-action", action);
        tr.doc.essential(b);
        let ink = if lit { t::on_accent() } else { t::text() };
        let ink = Color {
            a: ink.a * if dim { 0.4 } else { 1.0 },
            ..ink
        };
        let i = tr.doc.add(b, "span.player-icon");
        tr.slot(
            i,
            PSlot::Icon {
                name: icon,
                color: ink,
            },
        );
        if !label.is_empty() {
            let l = tr.doc.add(b, "span.player-label");
            tr.doc.text(l, label);
        }
        tr.acts.insert(b, Msg::Act(act));
    }

    /// The bar as the engine draws it (wrapped as the hand-written bar is: opaque to the picture's click layer, and telling the player when the pointer is on it).
    pub(super) fn bar_view(&self) -> Element<'_, Msg> {
        let Some(host) = self.bar.as_ref() else {
            return bar(self);
        };
        let tree = self.bar_tree();
        let mut h = host.borrow_mut();
        let css = sheet();
        if css != h.sheet {
            let safe = h.engine.safe_mode();
            h.engine = Engine::new(&css);
            h.engine.set_safe_mode(safe);
            let user = h.user.clone();
            h.engine.set_user_css(user.as_deref());
            h.sheet = css;
        }
        let it = h.it;
        let BarHost { engine, cache, .. } = &mut *h;
        let scene = Rc::new(engine.build(
            &tree.doc,
            (self.win.width, BAR_H),
            it,
            &Builders::new(),
            &mut |text, st, max| cache.measure(text, st, max, &mut shape),
        ));
        for n in engine.take_notices() {
            eprintln!("engine: {n}");
        }
        h.scene = Some(scene.clone());
        h.acts = tree.acts;
        drop(h);
        let mut s = Styled::shared(scene, Msg::Eng).keys(false);
        let f = facts(self);
        for (i, slot) in tree.slots.iter().enumerate() {
            let el: Element<'_, Msg> = match slot {
                PSlot::Icon { name, color } => glyph(name, 16.0, *color).into(),
                PSlot::Seek => controls::seek_track(self, f.frac),
                PSlot::Volume => controls::volume_track(f.vol),
            };
            s = s.slot(i as u32 + 1, el);
        }
        mouse_area(opaque(container(s).width(Length::Fill).height(BAR_H)))
            .on_enter(Msg::Act(Act::BarHover(true)))
            .on_exit(Msg::Act(Act::BarHover(false)))
            .into()
    }

    /// The user stylesheet of the active theme (layered over the default sheet), or none.
    pub fn set_user_css(&mut self, css: Option<String>) {
        if let Some(host) = self.bar.as_ref() {
            let mut h = host.borrow_mut();
            if h.user != css {
                h.engine.set_user_css(css.as_deref());
                h.user = css;
            }
        }
    }

    /// An event of the paint widget: the hover and press state, and clicks.
    pub(super) fn bar_event(&mut self, e: StyledEvent) -> Task<Msg> {
        let Some(host) = self.bar.as_ref() else {
            return Task::none();
        };
        let mut h = host.borrow_mut();
        match e {
            StyledEvent::Hover(n) => h.it.hover = n,
            StyledEvent::Active(n) => h.it.active = n,
            StyledEvent::Click(n) => {
                let msg = h.scene.as_ref().and_then(|sc| {
                    let mut at = Some(n);
                    while let Some(i) = at {
                        if let Some(m) = h.acts.get(&i) {
                            return Some(m.clone());
                        }
                        at = sc.doc.nodes[i].parent;
                    }
                    None
                });
                drop(h);
                return msg.map_or_else(Task::none, Task::done);
            }
            _ => {}
        }
        Task::none()
    }
}
