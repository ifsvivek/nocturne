//! Things drawn over the page: the More menu, the media info panel, the versions and sources
//! picker, the playlist dialog.
use super::cells::g;
use super::view::info_table;
use super::{Detail, DlgKind, Msg};
use crate::style;
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{action_button, dot, menu_row, mono};
use iced::widget::button::{Status, Style};
use iced::widget::{
    column, container, mouse_area, opaque, row, scrollable, space, text, text_input,
};
use iced::{Alignment, Background, Element, Length, Shadow};
use nocturne_core::versions;

type El<'a> = Element<'a, Msg>;

pub(super) const DLG_ID: &str = "detail-dialog";

/// Panel over a click-away scrim.
fn modal<'a>(panel: impl Into<El<'a>>) -> El<'a> {
    mouse_area(
        container(opaque(panel))
            .center(Length::Fill)
            .align_x(Alignment::Center)
            .style(style::scrim),
    )
    .on_press(Msg::Dismiss)
    .into()
}

impl Detail {
    pub(super) fn overlays(&self) -> Vec<El<'_>> {
        let mut v: Vec<El<'_>> = vec![];
        if self.info_open
            && let Some(f) = &self.full
        {
            v.push(modal(
                container(scrollable(container(info_table(f)).padding(t::S6)).style(style::scroll))
                    .width(760)
                    .max_height(560)
                    .style(style::panel),
            ));
        } else if let Some(sel) = self.more {
            let mut col = column![].padding(t::S2);
            for (i, (label, a)) in self.more_items().into_iter().enumerate() {
                col = col.push(menu_row(
                    g(label).size(t::BODY),
                    i == sel,
                    false,
                    Msg::MoreAct(a),
                ));
            }
            v.push(modal(container(col).width(280).style(style::panel)));
        } else if let Some(cur) = self.ver_open {
            v.push(modal(self.versions_panel(cur)));
        } else if let Some(d) = &self.dialog {
            v.push(modal(self.dialog_panel(d)));
        }
        if let Some(p) = &self.photo {
            v.push(self.photo_overlay(p));
        }
        v
    }

    /// "Versions and sources": every way to play this title. Up/Down moves, Enter chooses, P also
    /// makes the choice stick for this title, Esc closes.
    fn versions_panel(&self, cur: usize) -> El<'_> {
        let list = self.versions();
        let picked = self.picked(&list);
        let mut col = column![].spacing(t::S1);
        for (i, v) in list.iter().enumerate() {
            let mark = self.mark_of(v.origin);
            let title = if v.name.is_empty() {
                self.media.title.clone()
            } else {
                v.name.clone()
            };
            let right: El<'_> = row![
                dot(if v.available {
                    mark.map_or(t::text_secondary(), |m| m.color)
                } else {
                    t::warn()
                }),
                mono(mark.map_or(String::new(), |m| m.label.clone())).color(t::text_secondary()),
            ]
            .spacing(t::S1 + 2.0)
            .align_y(Alignment::Center)
            .into();
            let sub = if !v.available {
                "not available right now".to_string()
            } else if v.detail.is_empty() {
                "quality unknown".to_string()
            } else {
                v.detail.clone()
            };
            let on = i == cur;
            let line = row![
                container(g(if i == picked { "\u{2713}" } else { "" }).color(t::accent_text()))
                    .width(20),
                column![
                    container(
                        g(title)
                            .size(t::BODY)
                            .font(t::semibold())
                            .color(if v.available {
                                t::text()
                            } else {
                                t::text_secondary()
                            })
                            .wrapping(text::Wrapping::None),
                    )
                    .clip(true),
                    mono(sub).color(if v.available {
                        t::text_tertiary()
                    } else {
                        t::warn()
                    }),
                ]
                .spacing(2)
                .width(Length::Fill),
                right,
            ]
            .spacing(t::S3)
            .align_y(Alignment::Center);
            col = col.push(
                Press::new(line)
                    .width(Length::Fill)
                    .padding([t::S2, t::S3])
                    .on_press(Msg::VersionPick(i))
                    .style(move |s| Style {
                        background: if on {
                            Some(Background::Color(t::accent_tint()))
                        } else if matches!(s, Status::Hovered | Status::Pressed) {
                            Some(Background::Color(t::GLASS))
                        } else {
                            None
                        },
                        text_color: t::text(),
                        border: style::border(t::NONE, 0.0, 6.0),
                        shadow: Shadow::default(),
                        snap: false,
                    }),
            );
        }
        let can_prefer = versions::worth_showing(&list);
        let panel = column![
            g("Versions and sources").size(t::HEADING).font(t::bold()),
            g("Choose which copy Play uses for this title.")
                .size(t::BODY)
                .color(t::text_secondary()),
            space().height(t::S2),
            col,
            space().height(t::S2),
            row![
                action_button(
                    "Always prefer this source for this title",
                    false,
                    false,
                    false,
                    !can_prefer,
                    Msg::Prefer
                ),
                space().width(Length::Fill),
                action_button("Close", false, false, false, false, Msg::Dismiss),
            ]
            .align_y(Alignment::Center),
            mono("\u{2191}\u{2193} move   enter choose   p prefer   esc close"),
        ]
        .spacing(t::S2);
        container(panel)
            .width(560)
            .padding(t::S6)
            .style(style::panel)
            .into()
    }

    fn dialog_panel<'a>(&'a self, d: &'a super::Dialog) -> El<'a> {
        let (title, body, label, go, danger) = match &d.kind {
            DlgKind::Rename { .. } => (
                "Rename playlist",
                "Give the playlist a new name.".to_string(),
                "New name",
                "Rename",
                false,
            ),
            DlgKind::Delete { name, .. } => (
                "Delete playlist",
                format!("This deletes {name} for good. The items in it are not touched."),
                "Type the playlist name to confirm",
                "Delete playlist",
                true,
            ),
            DlgKind::Add { own, .. } => (
                "Add to a playlist",
                format!(
                    "Type the name of one of your playlists to add to it, or a new name to start one. {}",
                    if own.is_empty() {
                        "You have no playlists yet; a new one is created.".to_string()
                    } else {
                        format!(
                            "Yours: {}.",
                            own.iter()
                                .map(|o| o.1.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                ),
                "Playlist name",
                "Add",
                false,
            ),
        };
        let mut col = column![
            mono("PLEASE CONFIRM"),
            g(title).size(t::HEADING).font(t::bold()),
            g(body).size(t::BODY).color(t::text_secondary()),
            mono(label.to_uppercase()),
            text_input("", &d.text)
                .id(DLG_ID)
                .on_input_maybe((!d.busy).then_some(Msg::DlgInput as fn(String) -> Msg))
                .on_submit(Msg::DlgSubmit)
                .padding(t::S3)
                .size(t::BODY)
                .style(style::input),
        ]
        .spacing(t::S3);
        if let Some(m) = d.msg {
            col = col.push(g(m).size(t::BODY).color(t::danger()));
        }
        col = col.push(
            row![
                space().width(Length::Fill),
                action_button("Cancel", false, false, false, d.busy, Msg::Dismiss),
                action_button(go, !danger, danger, false, d.busy, Msg::DlgSubmit),
            ]
            .spacing(t::S2),
        );
        container(col)
            .width(480)
            .padding(t::S6)
            .style(style::panel)
            .into()
    }
}
