//! Chrome: header, breadcrumbs, avatar and the account menu (port of the GPUI `Root::header`,
//! `crumbs`, `avatar`, `account_menu`).
use super::message::{AcctAct, Message};
use super::{App, GLOBALS, View};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{
    avatar, menu_row, mono, nav_link, rule, source_mark, tip, value_chip, wordmark,
};
use iced::widget::button::{Status, Style};
use iced::widget::{column, container, mouse_area, opaque, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::nav;
use nocturne_core::sync::NEED_WRITES;

type El<'a> = Element<'a, Message>;

fn hist_button<'a>(glyph: &'static str, label: &'static str, enabled: bool, m: Message) -> El<'a> {
    tip(
        Press::new(container(text(glyph).size(t::BODY_LG + 4.0)).center(Length::Fill))
            .width(28)
            .height(28)
            .on_press_maybe(enabled.then_some(m))
            .style(move |s| Style {
                background: matches!(s, Status::Hovered | Status::Pressed)
                    .then_some(t::GLASS.into()),
                text_color: if enabled {
                    t::TEXT
                } else {
                    iced::Color {
                        a: 0.5,
                        ..t::TEXT_TERTIARY
                    }
                },
                border: border(t::NONE, 0.0, 6.0),
                ..Default::default()
            }),
        label,
    )
}

impl App {
    pub(super) fn header(&self) -> El<'_> {
        let home = self.stack.first();
        let libs = home.map_or(&[][..], |h| h.libs.items());
        let cur = self
            .stack
            .get(1)
            .and_then(|p| p.media.as_ref())
            .map(|m| (m.origin, m.id.as_str()));
        let at_home = self.stack.len() <= 1;
        let blur = self.admin_open;
        let mut nav_row = row![nav_link("Home", at_home && !blur, false, Message::GoHome)]
            .spacing(t::S1)
            .align_y(Alignment::Center);
        // With several sources the libraries come grouped, each group led by its source mark.
        let grouped = home.is_some_and(|h| h.fan.contributors() > 1);
        let mut group = None;
        for (i, l) in libs.iter().enumerate() {
            if grouped && group != Some(l.origin) {
                group = Some(l.origin);
                if let Some(slot) = self.catalog.slot(l.origin) {
                    nav_row = nav_row.push(source_mark(slot, true));
                }
            }
            nav_row = nav_row.push(nav_link(
                &l.title,
                Some((l.origin, l.id.as_str())) == cur && !at_home && !blur,
                false,
                Message::OpenLibrary(i),
            ));
        }
        for (gi, (key, label)) in GLOBALS.iter().enumerate() {
            nav_row = nav_row.push(nav_link(
                label,
                cur.is_some_and(|c| c.1 == *key) && !at_home && !blur,
                false,
                Message::OpenGlobal(gi),
            ));
        }
        if self.admin {
            nav_row = nav_row.push(nav_link(
                "Admin",
                self.admin_open,
                false,
                Message::OpenAdmin,
            ));
        }
        let left = row![
            row![
                hist_button(
                    "\u{2039}",
                    "Back (Alt+Left)",
                    self.stack.len() > 1,
                    Message::Back
                ),
                hist_button(
                    "\u{203A}",
                    "Forward (Alt+Right)",
                    !self.fwd.is_empty(),
                    Message::Forward
                ),
                space().width(t::S3),
                wordmark(t::CAPTION + 1.0),
            ]
            .spacing(t::S1)
            .align_y(Alignment::Center),
            // Many sources make a long link row: it scrolls sideways (wheel) instead of pushing the
            // search and avatar out of the window.
            scrollable(nav_row)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(0).scroller_width(0),
                ))
                .width(Length::Fill),
        ]
        .spacing(t::S8)
        .width(Length::Fill)
        .align_y(Alignment::Center);
        let search = tip(
            Press::new(
                row![
                    text("Search")
                        .size(t::CAPTION + 1.0)
                        .color(t::TEXT_SECONDARY),
                    mono("/")
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            )
            .height(32)
            .padding([6.0, t::S3])
            .on_press(Message::OpenSearch)
            .style(|s| Style {
                text_color: t::TEXT,
                border: border(
                    if matches!(s, Status::Hovered | Status::Pressed) {
                        t::ACCENT
                    } else {
                        t::EDGE
                    },
                    1.0,
                    6.0,
                ),
                ..Default::default()
            }),
            "Search (Ctrl+K or /)",
        );
        let mut right = row![].spacing(t::S4).align_y(Alignment::Center);
        if self.catalog.online().count() > 1 {
            right = right.push(tip(
                value_chip(
                    &self.filter_label(),
                    true,
                    false,
                    false,
                    Message::CycleFilter,
                ),
                "Show one source or all",
            ));
        }
        let right = right.push(search).push(self.avatar());
        let right = row![right].spacing(t::S4).align_y(Alignment::Center);
        let bar = row![left, right]
            .align_y(Alignment::Center)
            .height(t::HEADER_H - 1.0);
        let head = container(column![container(bar).padding([0.0, t::PAGE_PAD]), rule(),])
            .width(Length::Fill)
            .style(style::fill(t::HEADER_BG));
        container(head)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::Start)
            .into()
    }

    /// Initial in a hairline circle; opens the account menu.
    fn avatar(&self) -> El<'_> {
        let name = self
            .session()
            .map_or(if self.dev { "dev" } else { "NOCTURNE" }, |(_, s)| {
                s.user_name.as_str()
            });
        let open = self.menu;
        Press::new(avatar(
            32.0,
            name,
            self.my_pic().as_ref(),
            t::NONE,
            0.0,
            t::CAPTION + 1.0,
            t::ACCENT_TEXT,
        ))
        .on_press(Message::ToggleMenu)
        .style(move |s| Style {
            border: border(
                if open || matches!(s, Status::Hovered | Status::Pressed) {
                    t::ACCENT
                } else {
                    t::EDGE
                },
                1.0,
                16.0,
            ),
            ..Default::default()
        })
        .into()
    }

    /// Breadcrumbs for deep pages: Home > ... > current; every crumb but the last goes back to it.
    pub(super) fn crumbs(&self) -> Option<El<'_>> {
        let n = self.stack.len();
        if n < 3 {
            return None;
        }
        let title = |p: &super::Page| {
            if p.view == View::Home {
                "Home".to_string()
            } else {
                p.media.as_ref().map_or(String::new(), |m| m.title.clone())
            }
        };
        let (start, elided) = nav::crumb_window(n, 4);
        let shown: Vec<usize> = if elided {
            std::iter::once(0).chain(start..n).collect()
        } else {
            (0..n).collect()
        };
        let mut r = row![].spacing(t::S2).align_y(Alignment::Center);
        for (k, i) in shown.into_iter().enumerate() {
            if k > 0 {
                r = r.push(mono(if elided && k == 1 {
                    "\u{2026}  \u{203A}"
                } else {
                    "\u{203A}"
                }));
            }
            let last = i == n - 1;
            let label = text(title(&self.stack[i]))
                .size(t::CAPTION)
                .wrapping(text::Wrapping::None);
            r = r.push(
                container(
                    Press::new(label)
                        .on_press_maybe((!last).then_some(Message::Crumb(i)))
                        .style(move |s| Style {
                            text_color: if last {
                                t::TEXT
                            } else if matches!(s, Status::Hovered | Status::Pressed) {
                                t::ACCENT_TEXT
                            } else {
                                t::TEXT_SECONDARY
                            },
                            ..Default::default()
                        }),
                )
                .max_width(240)
                .clip(true),
            );
        }
        // Same fill as the header: scrolled content must not show through behind the trail.
        let bar = container(r.height(28))
            .width(Length::Fill)
            .padding([0.0, t::PAGE_PAD])
            .style(style::fill(t::HEADER_BG));
        Some(
            container(bar)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: t::HEADER_H,
                    ..Padding::ZERO
                })
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// Dropdown under the avatar: who and where, then the account actions.
    pub(super) fn account_menu(&self) -> El<'_> {
        let none = if self.dev {
            ("Developer token".to_string(), "read-only".to_string())
        } else {
            (
                "NOCTURNE".to_string(),
                format!("{} sources", self.sources.profiles.len()),
            )
        };
        let (name, server) = self
            .session()
            .map_or(none, |(u, s)| (s.user_name.clone(), u.clone()));
        let who = row![
            avatar(
                40.0,
                &name,
                self.my_pic().as_ref(),
                t::HAIRLINE_STRONG,
                1.0,
                t::BODY,
                t::ACCENT_TEXT
            ),
            column![
                text(name)
                    .size(t::BODY)
                    .font(t::SEMIBOLD)
                    .wrapping(text::Wrapping::None),
                mono(server).wrapping(text::Wrapping::None),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(t::S3)
        .align_y(Alignment::Center);
        let mut items: Vec<El> = vec![
            container(who).padding([t::S2, t::S3]).clip(true).into(),
            rule(),
        ];
        let writes = self.hooks.can_write();
        for (i, (label, act)) in self.menu_items().into_iter().enumerate() {
            let face = match act {
                AcctAct::SwitchTo(n) => self.other_accounts.get(n).map(|a| {
                    let pic = self.user_pic(&a.user_id, a.image_tag.as_deref());
                    avatar(
                        22.0,
                        &a.user_name,
                        pic.as_ref(),
                        t::HAIRLINE_STRONG,
                        1.0,
                        t::CAPTION,
                        t::TEXT_SECONDARY,
                    )
                }),
                _ => None,
            };
            let mut c = row![].spacing(t::S2).align_y(Alignment::Center);
            if let Some(f) = face {
                c = c.push(f);
            }
            c = c.push(text(label).size(t::BODY));
            let gated = matches!(
                act,
                AcctAct::Password
                    | AcctAct::Authorize
                    | AcctAct::ProfileImage
                    | AcctAct::RemoveImage
                    | AcctAct::Language
            ) && !writes;
            let item = menu_row(c, i == self.menu_sel, gated, Message::MenuAct(act));
            items.push(if gated { tip(item, NEED_WRITES) } else { item });
        }
        let panel = opaque(
            container(column(items).spacing(2))
                .width(260)
                .padding(t::S2)
                .style(style::panel),
        );
        // Click anywhere else closes it.
        iced::widget::stack![
            mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::CloseMenu),
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::Start)
                .padding(Padding {
                    top: t::HEADER_H + 4.0,
                    right: t::PAGE_PAD,
                    ..Padding::ZERO
                }),
        ]
        .into()
    }

    pub(super) fn menu_key(&mut self, key: &str) -> Task<Message> {
        let items = self.menu_items();
        match key {
            "escape" | "a" => self.menu = false,
            "up" => self.menu_sel = self.menu_sel.saturating_sub(1),
            "down" => self.menu_sel = (self.menu_sel + 1).min(items.len() - 1),
            "enter" => {
                if let Some((_, act)) = items.get(self.menu_sel) {
                    return self.menu_act(*act);
                }
            }
            _ => {}
        }
        Task::none()
    }
}

impl App {
    /// The source filter's chip text: All sources, or the one source's label.
    fn filter_label(&self) -> String {
        match self.filter {
            nocturne_core::sources::SourceFilter::All => "All sources".into(),
            nocturne_core::sources::SourceFilter::One(id) => self.label_of(id),
        }
    }
}
