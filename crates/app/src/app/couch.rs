//! Couch mode glue: resolving the mode (setting, gamepad, display), gamepad presses, and the
//! on-screen button hints.
use super::message::Message;
use super::{App, Phase};
use crate::pad::{self, Btn};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::mono;
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Task, window};
use std::time::Duration;

/// A window that fills a display at least this many physical pixels wide counts as a TV for
/// Automatic mode.
const TV_MIN_PX: f32 = 1900.0;

impl App {
    /// Re-resolve Couch mode; when it flips, the scale changes, so re-read the window's size and scale.
    pub(super) fn refresh_couch(&mut self) -> Task<Message> {
        let tv = self.monitor.is_some_and(|m| {
            m.width * self.scale >= TV_MIN_PX
                && self.win.width >= m.width - 8.0
                && self.win.height >= m.height - 8.0
        });
        let on = self.settings.couch.active(self.pads > 0, tv);
        if on == crate::couch::on() {
            return Task::none();
        }
        crate::couch::set(on);
        crate::task::after(Duration::from_millis(250), Message::Rescale)
    }

    /// The monitor under the window, only needed to resolve Automatic.
    pub(super) fn probe_monitor(&self) -> Task<Message> {
        if self.settings.couch != nocturne_core::settings::CouchMode::Auto {
            return Task::none();
        }
        window::oldest()
            .and_then(window::monitor_size)
            .map(Message::Monitor)
    }

    pub(super) fn on_pad(&mut self, e: pad::Ev) -> Task<Message> {
        match e {
            pad::Ev::Pads(n) => {
                self.pads = n;
                self.refresh_couch()
            }
            pad::Ev::Press(b) => {
                // A press proves a pad is there (hints switch to button names).
                self.pads = self.pads.max(1);
                let in_player = self.player.is_some();
                let enter = self.player.as_ref().is_some_and(|p| p.wants_enter());
                match pad::key(b, in_player, enter) {
                    Some(k) => self.on_key(k),
                    None => Task::none(),
                }
            }
        }
    }

    /// Button hints along the edge of the screen, in the vocabulary of what is plugged in: pad
    /// buttons with a gamepad, key names otherwise (remote, keyboard).
    pub(super) fn hints(&self) -> Option<Element<'_, Message>> {
        if !crate::couch::on() || !matches!(self.phase, Phase::Ready) {
            return None;
        }
        if let Some(p) = &self.player
            && !p.controls_visible()
        {
            return None;
        }
        let pad = self.pads > 0;
        let in_player = self.player.is_some();
        let list: &[(&str, &str)] = match (in_player, pad) {
            (false, true) => &[
                (Btn::A.label(), "Select"),
                (Btn::B.label(), "Back"),
                (Btn::Y.label(), "Search"),
                (Btn::Start.label(), "Account"),
                ("LB RB", "Page"),
            ],
            (false, false) => &[
                ("Enter", "Select"),
                ("Esc", "Back"),
                ("/", "Search"),
                ("A", "Account"),
                ("PgUp PgDn", "Page"),
            ],
            (true, true) => &[
                (Btn::A.label(), "Play / pause"),
                (Btn::B.label(), "Back"),
                (Btn::X.label(), "Subtitles"),
                (Btn::Y.label(), "Audio"),
                (Btn::Start.label(), "Menu"),
                ("LT RT", "Seek"),
            ],
            (true, false) => &[
                ("Space", "Play / pause"),
                ("Esc", "Back"),
                ("T", "Subtitles"),
                ("V", "Audio"),
                ("O", "Menu"),
                ("Shift \u{2190}\u{2192}", "Seek"),
            ],
        };
        let mut r = row![].spacing(t::S6).align_y(Alignment::Center);
        for (k, label) in list {
            r = r.push(
                row![
                    container(mono(*k).color(t::text()))
                        .padding([2.0, t::S2])
                        .style(|_| container::Style {
                            background: Some(t::control().into()),
                            border: border(t::edge(), 1.0, t::r_xs() + 3.0),
                            ..Default::default()
                        }),
                    text(*label)
                        .size(t::CAPTION + 1.0)
                        .color(t::text_secondary()),
                ]
                .spacing(t::S2)
                .align_y(Alignment::Center),
            );
        }
        let bar = container(r)
            .padding([t::S2, t::S6])
            .style(style::raised_card);
        Some(
            container(bar)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(if in_player {
                    Alignment::Start
                } else {
                    Alignment::End
                })
                .padding(t::S4)
                .into(),
        )
    }
}
