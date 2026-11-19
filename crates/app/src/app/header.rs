//! Chrome: header, breadcrumbs, avatar and the account menu (port of the old `Root::header`,
//! `crumbs`, `avatar`, `account_menu`).
use super::home::HRow;
use super::message::{AcctAct, Message};
use super::{App, GLOBALS, View};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::popover::Popover;
use crate::widget::press::Press;
use crate::widget::{
    avatar, icon, menu_row, mono, nav_link, nav_link_icon, rule, source_mark, tip, value_chip,
    wordmark,
};
use iced::widget::button::{Status, Style};
use iced::widget::{column, container, mouse_area, opaque, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::nav;
use nocturne_core::sync::NEED_WRITES;
use nocturne_core::unified::{Link, links};

type El<'a> = Element<'a, Message>;

pub(super) fn hist_button<'a>(
    glyph: &'static str,
    label: &'static str,
    enabled: bool,
    m: Message,
) -> El<'a> {
    tip(
        Press::new(container(text(glyph).size(t::BODY_LG + 4.0)).center(Length::Fill))
            .width(28)
            .height(28)
            .on_press_maybe(enabled.then_some(m))
            .style(move |s| Style {
                background: matches!(s, Status::Hovered | Status::Pressed)
                    .then_some(t::GLASS.into()),
                text_color: if enabled {
                    t::text()
                } else {
                    iced::Color {
                        a: 0.5,
                        ..t::text_tertiary()
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
        let nav_focus = self
            .stack
            .last()
            .filter(|_| at_home)
            .and_then(|p| p.home.as_deref())
            .filter(|h| h.layout().get(h.row) == Some(&HRow::Nav))
            .map(|h| h.col);
        let hp = t::parts(None, None);
        let icons = hp.header.link_icons == Some(true);
        let ico = |l: &Link| icons.then(|| link_icon(l, libs)).flatten();
        let mut nav_row = row![].spacing(t::S1).align_y(Alignment::Center);
        if hp.header.align == Some(nocturne_core::customize::HeaderAlign::Center) {
            // The links fit in the strip: push them to the middle (they scroll as before when not).
            let (_, vw, _) = self.nav_geom();
            let content: f32 = self.nav_widths().iter().sum();
            nav_row = nav_row.push(space().width(((vw - content) / 2.0).max(0.0)));
        }
        for (n, link) in self.nav_links().into_iter().enumerate() {
            let focused = nav_focus == Some(n);
            nav_row = nav_row.push(match link {
                Link::Home => nav_link_icon(
                    "Home",
                    ico(&link),
                    at_home && !blur,
                    focused,
                    Message::GoHome,
                ),
                Link::Kind(k) => nav_link_icon(
                    k.label(),
                    ico(&link),
                    cur.is_some_and(|c| c.1 == k.scope().key) && !at_home && !blur,
                    focused,
                    Message::OpenKind(k),
                ),
                Link::Lib(i) => nav_link_icon(
                    &libs[i].title,
                    ico(&link),
                    Some((libs[i].origin, libs[i].id.as_str())) == cur && !at_home && !blur,
                    focused,
                    Message::OpenLibrary(i),
                ),
                Link::Libraries => self.libraries_menu(
                    libs,
                    cur.is_some_and(|c| libs.iter().any(|l| (l.origin, l.id.as_str()) == c))
                        && !at_home
                        && !blur,
                    focused,
                ),
                Link::Global(gi) => nav_link_icon(
                    GLOBALS[gi].1,
                    ico(&link),
                    cur.is_some_and(|c| c.1 == GLOBALS[gi].0) && !at_home && !blur,
                    focused,
                    Message::OpenGlobal(gi),
                ),
                Link::Smart => self.smart_menu(
                    cur.is_some_and(|c| c.1.starts_with("smart-")) && !at_home && !blur,
                    focused,
                ),
                Link::Admin => nav_link_icon(
                    "Admin",
                    ico(&link),
                    self.admin_open,
                    focused,
                    Message::OpenAdmin,
                ),
            });
        }
        // Couch mode drops the pointer-sized history buttons (B goes back) and keeps the big tabs.
        let mut brand = row![].spacing(t::S1).align_y(Alignment::Center);
        if !crate::couch::on() && t::header_item(t::HeaderItem::History) {
            brand = brand
                .push(hist_button(
                    "\u{2039}",
                    "Back (Alt+Left)",
                    self.stack.len() > 1,
                    Message::Back,
                ))
                .push(hist_button(
                    "\u{203A}",
                    "Forward (Alt+Right)",
                    !self.fwd.is_empty(),
                    Message::Forward,
                ))
                .push(space().width(t::S3));
        }
        let name: El = match hp
            .header_brand
            .shown()
            .then(|| self.brand_text(&hp.header_brand))
        {
            Some(n) => {
                let b = &hp.header_brand;
                let ty = t::typo(
                    &nocturne_core::customize::Text {
                        font_size: b.font_size,
                        font_weight: b.font_weight,
                        color: b.color,
                        ..Default::default()
                    },
                    t::BODY_LG,
                    iced::font::Weight::Bold,
                );
                ty.text(n, t::text()).wrapping(text::Wrapping::None).into()
            }
            None => wordmark(t::CAPTION + 1.0).into(),
        };
        let left = row![
            brand.push(name),
            // Many sources make a long link row: it scrolls sideways (wheel, chevrons, or the focus
            // on Home) instead of pushing the search and avatar out of the window.
            self.nav_strip(nav_row),
        ]
        .spacing(t::S8)
        .width(Length::Fill)
        .align_y(Alignment::Center);
        let search = tip(
            Press::new(
                row![
                    text("Search")
                        .size(t::CAPTION + 1.0)
                        .color(t::text_secondary()),
                    mono("/")
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            )
            .height(32)
            .padding([6.0, t::S3])
            .on_press(Message::OpenSearch)
            .style(|s| Style {
                text_color: t::text(),
                border: border(
                    if matches!(s, Status::Hovered | Status::Pressed) {
                        t::accent()
                    } else {
                        t::edge()
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
        if hp.header_icons.hidden == Some(false) {
            right = right.push(tip(
                Press::new(icon("people", 22.0, t::text()))
                    .on_press(Message::SyncPlay(crate::syncplay::Msg::OpenScreen))
                    .style(|_| Style::default()),
                "Watch together",
            ));
        }
        if t::header_item(t::HeaderItem::Search) {
            right = right.push(search);
        }
        if t::header_item(t::HeaderItem::Account) {
            right = right.push(self.avatar());
        }
        let right = row![right].spacing(t::S4).align_y(Alignment::Center);
        let bar = row![left, right]
            .align_y(Alignment::Center)
            .height(t::header_h() - 1.0);
        let head = container(column![
            container(bar).padding([0.0, t::page_pad()]),
            rule(),
        ])
        .width(Length::Fill)
        .style(style::fill(t::header_bg()));
        container(head)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::Start)
            .into()
    }

    /// The name `.header-brand` shows: the active server, the signed-in user or the app.
    pub(super) fn brand_text(&self, b: &nocturne_core::customize::Brand) -> String {
        use nocturne_core::customize::BrandContent;
        let app = "NOCTURNE".to_string();
        match b.content {
            Some(BrandContent::User) => self.session().map_or(app, |(_, s)| s.user_name.clone()),
            Some(BrandContent::App) => app,
            _ => self
                .active
                .map(|id| self.label_of(id))
                .filter(|l| !l.is_empty())
                .or_else(|| {
                    self.catalog
                        .online()
                        .next()
                        .map(|s| s.profile.label.clone())
                })
                .unwrap_or(app),
        }
    }

    /// Initial in a hairline circle; opens the account menu.
    pub(super) fn avatar(&self) -> El<'_> {
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
            t::accent_text(),
        ))
        .on_press(Message::ToggleMenu)
        .style(move |s| Style {
            border: border(
                if open || matches!(s, Status::Hovered | Status::Pressed) {
                    t::accent()
                } else {
                    t::edge()
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
                                t::text()
                            } else if matches!(s, Status::Hovered | Status::Pressed) {
                                t::accent_text()
                            } else {
                                t::text_secondary()
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
            .padding([0.0, t::page_pad()])
            .style(style::fill(t::header_bg()));
        Some(
            container(bar)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: t::header_h(),
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
                t::hairline_strong(),
                1.0,
                t::BODY,
                t::accent_text()
            ),
            column![
                text(name)
                    .size(t::BODY)
                    .font(t::semibold())
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
                        t::hairline_strong(),
                        1.0,
                        t::CAPTION,
                        t::text_secondary(),
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
                    top: t::header_h() + 4.0,
                    right: t::page_pad(),
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

pub const NAV_ID: &str = "nav-scroll";

/// The small icon in front of a header link (`--header-link-icons: on`).
pub(super) fn link_icon(l: &Link, libs: &[nocturne_core::source::Media]) -> Option<&'static str> {
    use nocturne_core::unified::UKind;
    let by_collection = |c: Option<&str>| match c {
        Some("movies") => "film",
        Some("tvshows") => "tv",
        Some("music") => "music",
        _ => "folder",
    };
    Some(match l {
        Link::Home => "home",
        Link::Kind(UKind::Movies) => "film",
        Link::Kind(UKind::Shows) => "tv",
        Link::Kind(UKind::Music) => "music",
        Link::Kind(_) => "folder",
        Link::Lib(i) => by_collection(libs.get(*i)?.collection.as_deref()),
        Link::Global(0) => "layers",
        Link::Global(1) => "queue",
        Link::Global(_) => "heart",
        Link::Admin => "gear",
        Link::Libraries | Link::Smart => return None,
    })
}

/// Rough width of one header link (label at body size plus its padding and gap); the real widths
/// are not known until layout, and the strip only needs to land near the focused link.
pub(super) fn link_w(label: &str) -> f32 {
    let size = t::typo(
        &t::parts(None, None).header_link,
        t::BODY,
        iced::font::Weight::Medium,
    )
    .size();
    label.chars().count() as f32 * size * 0.58 + 2.0 * t::S3 + t::S1
}

/// Where to scroll the strip so link `col` is in view (`None`: already visible). `widths` are the
/// links' widths in order, `off` / `vw` the current offset and viewport width.
pub fn nav_target(widths: &[f32], col: usize, off: f32, vw: f32) -> Option<f32> {
    let start: f32 = widths.iter().take(col).sum();
    let w = widths.get(col).copied()?;
    let content: f32 = widths.iter().sum();
    // A chevron (48 px) covers each end that has more beyond it.
    let (more_l, more_r) = nav_more(off, vw, content);
    let (pad_l, pad_r) = (
        if more_l { 48.0 } else { 0.0 },
        if more_r { 48.0 } else { 0.0 },
    );
    if start >= off + pad_l && start + w <= off + vw - pad_r {
        return None;
    }
    let max = (content - vw).max(0.0);
    Some((start - (vw - w) / 2.0).clamp(0.0, max))
}

/// Whether there is more of the strip to the left / right of the visible part.
pub fn nav_more(off: f32, vw: f32, content: f32) -> (bool, bool) {
    // Nothing is hidden when everything fits, whatever offset a scroll left behind.
    let overflow = content > vw + 1.0;
    (overflow && off > 1.0, overflow && off + vw < content - 1.0)
}

impl App {
    pub(super) fn nav_label(&self, l: &Link) -> String {
        let libs = self.stack.first().map_or(&[][..], |h| h.libs.items());
        match l {
            Link::Home => "Home".into(),
            Link::Kind(k) => k.label().into(),
            Link::Lib(i) => libs.get(*i).map_or(String::new(), |m| m.title.clone()),
            Link::Libraries => "Libraries \u{25BE}".into(),
            Link::Global(i) => GLOBALS[*i].1.into(),
            Link::Smart => "Smart \u{25BE}".into(),
            Link::Admin => "Admin".into(),
        }
    }

    pub(super) fn nav_widths(&self) -> Vec<f32> {
        self.nav_links()
            .iter()
            .map(|l| link_w(&self.nav_label(l)))
            .collect()
    }

    /// Offset, viewport width and content width of the link strip: measured once it has scrolled,
    /// estimated before (the chrome around it takes about 600 px).
    pub(super) fn nav_geom(&self) -> (f32, f32, f32) {
        if let Some(g) = self.nav_vp {
            return g;
        }
        let content: f32 = self.nav_widths().iter().sum();
        (
            0.0,
            (self.win.width - 2.0 * t::page_pad() - 600.0).max(200.0),
            content,
        )
    }

    /// Scroll task that brings link `col` into view.
    pub(super) fn nav_reveal(&self, col: usize) -> Task<Message> {
        let (off, vw, _) = self.nav_geom();
        match nav_target(&self.nav_widths(), col, off, vw) {
            Some(x) => iced::widget::operation::scroll_to(
                iced::widget::Id::from(NAV_ID),
                iced::widget::scrollable::AbsoluteOffset { x, y: 0.0 },
            ),
            None => Task::none(),
        }
    }

    /// Chevrons and fades at the ends of the strip when links are hidden past them.
    fn nav_strip<'a>(&'a self, nav: iced::widget::Row<'a, Message>) -> El<'a> {
        use iced::widget::Stack;
        let strip = scrollable(nav)
            .id(NAV_ID)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ))
            .on_scroll(|v| {
                Message::NavScrolled(
                    v.absolute_offset().x,
                    v.bounds().width,
                    v.content_bounds().width,
                )
            })
            .width(Length::Fill);
        let (off, vw, content) = self.nav_geom();
        let (left, right) = nav_more(off, vw, content);
        if !left && !right {
            return strip.into();
        }
        let edge = |glyph: &'static str, to_right: bool, dir: f32| -> El<'a> {
            // Solid frame colour at the outer edge, clear on the strip side.
            let clear = iced::Color {
                a: 0.0,
                ..t::frame()
            };
            let angle = if to_right { 0.0 } else { std::f32::consts::PI };
            let g = iced::gradient::Linear::new(angle)
                .add_stop(0.0, clear)
                .add_stop(1.0, t::frame());
            let chev = Press::new(container(text(glyph).size(t::HEADING)).center(Length::Fill))
                .width(48)
                .height(t::header_h() - 1.0)
                .on_press(Message::NavPage(dir))
                .style(|s| Style {
                    text_color: if matches!(s, Status::Hovered | Status::Pressed) {
                        t::accent_text()
                    } else {
                        t::text()
                    },
                    ..Default::default()
                });
            container(container(chev).style(move |_| container::Style {
                background: Some(iced::Background::Gradient(g.into())),
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(if to_right {
                Alignment::End
            } else {
                Alignment::Start
            })
            .into()
        };
        let mut layers: Vec<El<'a>> = vec![strip.into()];
        if left {
            layers.push(edge("\u{2039}", false, -1.0));
        }
        if right {
            layers.push(edge("\u{203A}", true, 1.0));
        }
        Stack::with_children(layers).width(Length::Fill).into()
    }

    /// The header's links for the libraries known now (see `unified::links`).
    pub(super) fn nav_links(&self) -> Vec<Link> {
        if !t::header_item(t::HeaderItem::Nav) {
            return vec![];
        }
        let home = self.stack.first();
        let libs = home.map_or(&[][..], |h| h.libs.items());
        let several = home.is_some_and(|h| h.fan.contributors() > 1);
        let mut v = links(
            libs,
            several,
            |o| {
                self.catalog
                    .slot(o)
                    .is_some_and(|s| s.backend.as_ref().is_some_and(|b| b.is_local()))
            },
            GLOBALS.len(),
            self.admin,
        );
        // Smart collections sit just before the globals.
        let at = v
            .iter()
            .position(|l| matches!(l, Link::Global(_) | Link::Admin))
            .unwrap_or(v.len());
        v.insert(at, Link::Smart);
        v
    }

    /// "Libraries": every source's libraries grouped under its source mark.
    pub(super) fn libraries_menu<'a>(
        &'a self,
        libs: &'a [nocturne_core::source::Media],
        active: bool,
        focused: bool,
    ) -> El<'a> {
        let link = nav_link(
            "Libraries \u{25BE}",
            active || self.lib_menu,
            focused,
            Message::ToggleLibMenu,
        );
        let popup = self.lib_menu.then(|| {
            let mut items: Vec<El> = vec![];
            for (origin, idxs) in nocturne_core::unified::group_libraries(libs) {
                if let Some(slot) = self.catalog.slot(origin) {
                    items.push(
                        container(source_mark(slot, true))
                            .padding([t::S2, t::S3])
                            .into(),
                    );
                }
                for i in idxs {
                    items.push(menu_row(
                        text(libs[i].title.clone()).size(t::BODY),
                        false,
                        false,
                        Message::OpenLibrary(i),
                    ));
                }
            }
            container(scrollable(column(items).spacing(2)).style(style::scroll))
                .width(260)
                .max_height(420)
                .padding(t::S2)
                .style(style::panel)
                .into()
        });
        Popover::new(link, popup, Message::ToggleLibMenu)
            .left()
            .into()
    }

    /// The source filter's chip text: All sources, or the one source's label.
    pub(super) fn filter_label(&self) -> String {
        match self.filter {
            nocturne_core::sources::SourceFilter::All => "All sources".into(),
            nocturne_core::sources::SourceFilter::One(id) => self.label_of(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_follows_the_focused_link() {
        let w = [100.0; 10]; // 1000 px of links in a 400 px strip
        assert_eq!(
            nav_target(&w, 0, 0.0, 400.0),
            None,
            "the first link is in view"
        );
        assert_eq!(nav_target(&w, 2, 0.0, 400.0), None);
        // Link 3 ends at 400: under the right chevron, so it scrolls (centred).
        assert_eq!(nav_target(&w, 3, 0.0, 400.0), Some(300.0 - 150.0));
        // The last link lands against the end, not past it.
        assert_eq!(nav_target(&w, 9, 0.0, 400.0), Some(600.0));
        // Going back left: link 1 sits under the left chevron at offset 100.
        assert_eq!(nav_target(&w, 1, 100.0, 400.0), Some(0.0));
        assert_eq!(nav_target(&w, 20, 0.0, 400.0), None, "no such link");
    }

    #[test]
    fn chevrons_only_where_links_are_hidden() {
        assert_eq!(nav_more(0.0, 400.0, 300.0), (false, false));
        assert_eq!(nav_more(50.0, 400.0, 300.0), (false, false));
        assert_eq!(nav_more(0.0, 400.0, 1000.0), (false, true));
        assert_eq!(nav_more(300.0, 400.0, 1000.0), (true, true));
        assert_eq!(nav_more(600.0, 400.0, 1000.0), (true, false));
    }
}
