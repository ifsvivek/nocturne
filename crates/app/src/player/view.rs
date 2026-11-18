//! The player page as one stack: video hole at the bottom, then the click layer, the loading
//! overlay, floating panels and the controls bar. The window is transparent: nothing opaque is
//! drawn over the hole except the loading overlay (by design) and, before the first frame or with
//! the external mpv, a solid black fill.
use super::controls::{bar, chip_button};
use super::popovers::{
    ctx_panel, floaters, hdr_prompt, overlays, popover, problem, queue_panel, resume_prompt,
    scrub_tip,
};
use super::{Act, Msg, Player};
use crate::motion;
use crate::theme as t;
use crate::widget::buffering::{self, Splash};
use crate::widget::hole::hole;
use iced::widget::{Stack, column, container, mouse_area, space, text};
use iced::{Alignment, Element, Length, mouse};
use nocturne_core::video::MenuAct;

impl Player {
    pub fn view(&self) -> Element<'_, Msg> {
        let shown = self.controls_visible();
        let black = !self.loaded || !self.video;
        let mut layers: Vec<Element<Msg>> = vec![hole(self.hole, black, Msg::Hole)];

        // Clicks on the picture: pause, fullscreen on double-click, context menu on right-click.
        let hidden = !shown && self.video;
        let mut click = mouse_area(space().width(Length::Fill).height(Length::Fill))
            .on_press(Msg::Act(Act::VideoPress))
            .on_double_click(Msg::Act(Act::VideoDouble))
            .on_right_press(Msg::Act(Act::VideoRight));
        if hidden {
            click = click.interaction(mouse::Interaction::Hidden);
        }
        layers.push(click.into());

        if self.popout && self.playback.is_some() && self.problem.is_none() {
            layers.push(popout_panel());
        } else if !self.video && self.playback.is_some() && self.problem.is_none() {
            layers.push(
                container(
                    text("Playing in the mpv window")
                        .size(t::BODY)
                        .color(t::text_secondary()),
                )
                .center(Length::Fill)
                .into(),
            );
        }
        if let Some(s) = self.splash_view() {
            layers.push(s);
        }
        layers.extend(overlays(self));
        layers.extend(floaters(self, shown));
        if let Some(q) = queue_panel(self, shown) {
            layers.push(q);
        }
        if shown {
            layers.push(
                container(bar(self))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_bottom(Length::Fill)
                    .into(),
            );
            if let Some(p) = self.pop {
                layers.push(popover(self, p));
            }
            if let Some(tip) = scrub_tip(self) {
                layers.push(tip);
            }
        }
        if let Some(c) = ctx_panel(self) {
            layers.push(c);
        }
        if let Some(r) = resume_prompt(self) {
            layers.push(r);
        }
        if let Some(h) = hdr_prompt(self) {
            layers.push(h);
        }
        if let Some(p) = problem(self) {
            layers.push(p);
        }
        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The loading overlay: full while opening, light on a stall during playback.
    fn splash_view(&self) -> Option<Element<'_, Msg>> {
        if !self.splash && !self.light || self.problem.is_some() {
            return None;
        }
        let (now, reduced) = (self.now, motion::reduced());
        let since = now.saturating_duration_since(self.born);
        let reveal = self.prog.value(now, reduced);
        let status = if self.splash && !self.loaded {
            "Opening".to_string()
        } else {
            buffering::percent_label(reveal)
        };
        let title = self
            .media
            .series_name
            .as_deref()
            .unwrap_or(&self.media.title);
        Some(buffering::view(Splash {
            title,
            backdrop: self.backdrop.as_ref().filter(|_| self.splash),
            logo: self.logo.as_ref(),
            status,
            reveal,
            breath: buffering::breath(since.as_secs_f32(), self.prog.stalled(now), reduced),
            zoom: buffering::zoom(since, reduced),
            fade: self
                .leaving
                .map_or(1.0, |t| buffering::fade_out(t, now, reduced)),
            reduced,
            light: !self.splash,
            win: self.win,
        }))
    }
}

/// Shown in the app window while the picture is in the HDR pop-out: what is going on, the way back.
fn popout_panel<'a>() -> Element<'a, Msg> {
    container(
        column![
            text("Playing in HDR").size(t::HEADING).font(t::semibold()),
            text("The picture is in mpv's own window. The controls below drive it from here.")
                .size(t::BODY)
                .color(t::text_secondary()),
            chip_button(
                "Play in this window (SDR)",
                false,
                Msg::Act(Act::Menu(MenuAct::HdrSwitch)),
            ),
        ]
        .spacing(t::S4)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}
