//! Settings page: one row per preference; a toggle flips on click, a multi-choice row opens a
//! drop-down. Changes apply and save at once. Port of the GPUI `settings_page` / `setting_chip`.
use super::App;
use super::message::Message;
use crate::style::{self, fill};
use crate::theme as t;
use crate::widget::popover::Popover;
use crate::widget::press::Press;
use crate::widget::{menu_row, mono, rule, section, switch, tracked, value_chip};
use iced::widget::button::{Status, Style};
use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Stack, column, container, row, scrollable as scroll, space, text};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::dropdown::Outcome;
use nocturne_core::settings as s;

type El<'a> = Element<'a, Message>;

const PAGE_ID: &str = "settings-scroll";
const MENU_ID: &str = "settings-menu";
/// Estimated heights used to keep the selected row in view (rows wrap, so this is approximate).
const ROW_H: f32 = 72.0;
const HEAD_H: f32 = 92.0;
const TOP_H: f32 = 140.0 + 72.0; // the Sources link row sits above the rows
const MENU_ROW_H: f32 = 37.0;

/// Estimated top of row `i` inside the page content.
fn row_top(i: usize) -> f32 {
    TOP_H + i as f32 * ROW_H + (0..=i).filter(|r| s::head(*r).is_some()).count() as f32 * HEAD_H
}

impl App {
    pub(super) fn settings_page(&self) -> El<'_> {
        let mut page = column![section(
            "Settings",
            "Changes apply immediately and are saved on this device.",
            "Esc to go back".into(),
        ),];
        page = page.push(
            container(column![
                Press::new(
                    row![
                        column![
                            text("Sources").size(t::BODY).font(t::SEMIBOLD),
                            text("Servers and local folders, their order, and which are on.")
                                .size(t::CONTROL_TEXT)
                                .color(t::TEXT_SECONDARY),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        text(format!("{}", self.sources.profiles.len()))
                            .size(t::CONTROL_TEXT)
                            .color(t::TEXT_SECONDARY),
                        text("\u{203A}").size(t::BODY_LG),
                    ]
                    .spacing(t::S4)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([t::S3, t::S6])
                .on_press(Message::OpenSources)
                .style(|st| Style {
                    background: matches!(st, Status::Hovered | Status::Pressed)
                        .then_some(t::GLASS.into()),
                    text_color: t::TEXT,
                    ..Default::default()
                }),
                rule(),
            ])
            .padding([t::S4, t::PAGE_PAD]),
        );
        page = page.push(
            container(column![
                Press::new(
                    row![
                        column![
                            text("Run setup again").size(t::BODY).font(t::SEMIBOLD),
                            text("Walk through where your media lives and how NOCTURNE behaves.")
                                .size(t::CONTROL_TEXT)
                                .color(t::TEXT_SECONDARY),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        text("\u{203A}").size(t::BODY_LG),
                    ]
                    .spacing(t::S4)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([t::S3, t::S6])
                .on_press(Message::RunSetup)
                .style(|st| Style {
                    background: matches!(st, Status::Hovered | Status::Pressed)
                        .then_some(t::GLASS.into()),
                    text_color: t::TEXT,
                    ..Default::default()
                }),
                rule(),
            ])
            .padding([0.0, t::PAGE_PAD]),
        );
        for (i, r) in self.settings.rows().into_iter().enumerate() {
            let on = i == self.settings_sel;
            if let Some(h) = s::head(i) {
                let mut head = column![
                    text(tracked(h))
                        .shaping(iced::widget::text::Shaping::Advanced)
                        .size(t::CONTROL_TEXT)
                        .font(t::BOLD)
                        .color(t::TEXT_SECONDARY)
                ]
                .spacing(t::S2);
                if i == 0 {
                    head = head.push(
                        container(
                            text(s::WRITES_NOTE)
                                .size(t::BODY)
                                .line_height(1.5)
                                .color(t::TEXT_SECONDARY),
                        )
                        .max_width(720),
                    );
                }
                page = page.push(
                    container(column![
                        container(head).padding(Padding {
                            top: if i == 0 { t::S6 } else { t::S8 },
                            bottom: t::S3,
                            left: t::S6,
                            right: t::S6,
                        }),
                        rule(),
                    ])
                    .padding([0.0, t::PAGE_PAD]),
                );
            }
            let text_col = column![
                text(r.label).size(t::BODY).font(t::SEMIBOLD),
                container(
                    text(r.hint)
                        .size(t::CONTROL_TEXT)
                        .line_height(1.45)
                        .color(t::TEXT_SECONDARY)
                )
                .max_width(640),
            ]
            .spacing(2)
            .width(Length::Fill);
            let chip = container(self.setting_chip(i, &r.value, on))
                .width(220)
                .align_x(Alignment::End);
            let content = row![space().width(0).height(64), text_col, chip]
                .spacing(t::S6)
                .align_y(Alignment::Center);
            let press = Press::new(content)
                .width(Length::Fill)
                .padding([t::S3, t::S6])
                .on_press(Message::SetRow(i))
                .style(move |st| Style {
                    background: if matches!(st, Status::Hovered | Status::Pressed) {
                        Some(t::GLASS.into())
                    } else if on {
                        Some(t::ACCENT_TINT.into())
                    } else {
                        None
                    },
                    text_color: t::TEXT,
                    ..Default::default()
                });
            let bar: El = if on {
                container(space().width(3).height(Length::Fill))
                    .height(Length::Fill)
                    .style(fill(t::ACCENT))
                    .into()
            } else {
                space().into()
            };
            let cell = Stack::from_vec(vec![press.into(), bar])
                .width(Length::Fill)
                .height(Length::Shrink);
            page = page.push(container(column![cell, rule()]).padding([0.0, t::PAGE_PAD]));
        }
        scroll(page.padding(Padding {
            top: t::HEADER_H + t::S8,
            bottom: t::S8,
            ..Padding::ZERO
        }))
        .id(PAGE_ID)
        .on_scroll(|v| Message::SetScrolled(v.absolute_offset().y, v.bounds().height))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The value chip of Settings row `i`; for a multi-choice row it carries a caret and, while
    /// open, its drop-down menu (an overlay, so the page's scroll never clips it).
    fn setting_chip(&self, i: usize, value: &str, on: bool) -> El<'_> {
        if i == s::TRAKT_ROW {
            return value_chip(
                &self.trakt_chip_label(),
                false,
                on,
                false,
                Message::SetRow(i),
            );
        }
        let Some((labels, cur)) = self.settings.choices(i) else {
            return switch(value == "On", on, Message::SetRow(i));
        };
        let open = self.dd.as_ref().is_some_and(|d| d.row == i);
        let chip = value_chip(value, true, on, open, Message::SetRow(i));
        let popup = open.then(|| {
            let sel = self.dd.as_ref().map_or(0, |d| d.sel);
            let items: Vec<El> = labels
                .into_iter()
                .enumerate()
                .map(|(k, name)| {
                    menu_row(
                        row![
                            text(name).size(t::BODY).color(if k == cur {
                                t::ACCENT_TEXT
                            } else {
                                t::TEXT
                            }),
                            space().width(Length::Fill),
                            mono(if k == cur { "\u{2713}" } else { "" }),
                        ]
                        .spacing(t::S3),
                        k == sel,
                        false,
                        Message::SetChoose(i, k),
                    )
                })
                .collect();
            container(
                scroll(column(items).spacing(2))
                    .id(MENU_ID)
                    .style(style::scroll),
            )
            .width(220)
            .max_height(300)
            .padding(t::S2)
            .style(style::panel)
            .into()
        });
        Popover::new(chip, popup, Message::SetClose).into()
    }

    /// Make sure row `i` is inside the visible part of the page.
    fn reveal_row(&self, i: usize) -> Task<Message> {
        let (top, bottom) = (row_top(i), row_top(i) + ROW_H);
        let (off, h) = (self.set_off, self.set_h);
        let y = if top < off + t::HEADER_H {
            (top - t::HEADER_H - t::S8).max(0.0)
        } else if bottom > off + h {
            bottom - h + t::S8
        } else {
            return Task::none();
        };
        operation::scroll_to(PAGE_ID, AbsoluteOffset { x: 0.0, y })
    }

    fn reveal_menu(&self, sel: usize) -> Task<Message> {
        operation::scroll_to(
            MENU_ID,
            AbsoluteOffset {
                x: 0.0,
                y: (sel as f32 * MENU_ROW_H - 3.0 * MENU_ROW_H).max(0.0),
            },
        )
    }

    /// Settings row `i` stepped; applies at once and is saved.
    fn cycle_setting(&mut self, i: usize, dir: i32) -> Task<Message> {
        if i == s::PROFILE_ROW {
            let n = self.settings.profiles.list.len() + 1;
            if n > 2 {
                let at = (self.settings.profiles.current() + n)
                    .wrapping_add_signed(dir.signum() as isize)
                    % n;
                self.set_default_profile(at);
            }
            return Task::none();
        }
        self.settings.cycle(i, dir);
        self.settings_changed()
    }

    /// Entry `idx` of row `i`'s drop-down chosen.
    pub(super) fn choose_setting(&mut self, i: usize, idx: usize) -> Task<Message> {
        if i == s::PROFILE_ROW {
            self.set_default_profile(idx);
            return Task::none();
        }
        self.settings.choose(i, idx);
        self.settings_changed()
    }

    /// Remembered profiles of this server and the default one, for the Settings row.
    pub(super) fn refresh_profiles(&mut self) {
        self.settings.profiles = match self.session() {
            Some((url, _)) if !self.dev => {
                let all = nocturne_session::Accounts::load().unwrap_or_default();
                s::Profiles {
                    list: all
                        .for_server(url)
                        .iter()
                        .map(|a| (a.user_id.clone(), a.user_name.clone()))
                        .collect(),
                    default: all.default_for(url).map(|a| a.user_id.clone()),
                }
            }
            _ => s::Profiles::default(),
        };
    }

    /// "Default profile" drop-down entry `idx`: 0 asks every time, otherwise that profile opens at launch.
    fn set_default_profile(&mut self, idx: usize) {
        if let Some((url, _)) = self.session() {
            let id = self.settings.profiles.id_at(idx).map(str::to_string);
            nocturne_session::remember_default(url, id.as_deref());
        }
        self.refresh_profiles();
    }

    fn settings_changed(&mut self) -> Task<Message> {
        self.settings.save();
        crate::motion::set_reduced(
            self.settings
                .reduce_motion
                .reduced(crate::platform::system_reduces_motion()),
        );
        self.apply_write_settings();
        Task::batch([self.refresh_couch(), self.probe_monitor()])
    }

    /// Open the drop-down of a multi-choice row (a toggle row is flipped instead).
    pub(super) fn open_setting(&mut self, i: usize) -> Task<Message> {
        self.settings_sel = i;
        if i == s::TRAKT_ROW {
            return self.trakt_msg(super::trakt::Msg::Open);
        }
        let Some((labels, cur)) = self.settings.choices(i) else {
            return self.cycle_setting(i, 1);
        };
        self.dd = nocturne_core::dropdown::Dropdown::open(i, labels.len(), cur);
        self.reveal_menu(cur)
    }

    pub(super) fn settings_key(&mut self, key: &str) -> Task<Message> {
        if let Some(d) = &mut self.dd {
            match d.key(key) {
                Outcome::Stay => {
                    let sel = d.sel;
                    return self.reveal_menu(sel);
                }
                Outcome::Close => self.dd = None,
                Outcome::Pick(idx) => {
                    let row = d.row;
                    self.dd = None;
                    return self.choose_setting(row, idx);
                }
            }
            return Task::none();
        }
        match key {
            "escape" | "backspace" => self.settings_open = false,
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" => {
                if let Some(r) = s::nav_row(self.settings_sel, key) {
                    self.settings_sel = r;
                    return self.reveal_row(r);
                }
            }
            "left" => return self.cycle_setting(self.settings_sel, -1),
            "right" => return self.cycle_setting(self.settings_sel, 1),
            "enter" | "space" => return self.open_setting(self.settings_sel),
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimated_row_tops_grow_with_sections() {
        assert!(row_top(1) > row_top(0));
        // Row 2 starts a section, so it sits further than a plain row step.
        assert!(row_top(2) - row_top(1) > ROW_H);
    }
}
