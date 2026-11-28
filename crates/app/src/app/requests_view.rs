//! Painting for `requests.rs`: the sign-in dialog, the Request dialog, the cancel confirmation, the
//! Requests page, the Seerr rows of the search overlay and the badge over a detail page. Copy says
//! "request" and "Seerr"; colours come from the theme tokens.
use super::message::Message;
use super::requests::{AskStage, Msg, Poster, Target, Title};
use super::{App, Load};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::cell::{Pic, tile};
use crate::widget::{action_button, label, menu_row, mono, overlay, rule, section};
use iced::widget::{Space, column, container, row, scrollable as scroll, text, text_input};
use iced::{Alignment, Element, Length, Padding};
use nocturne_core::requests::{self as rq, Badge, Tab};
use nocturne_seerr::{Kind, Request};

type El<'a> = Element<'a, Message>;

fn dim<'a>(s: impl Into<String>) -> El<'a> {
    text(s.into())
        .size(t::CONTROL_TEXT)
        .line_height(1.5)
        .color(t::text_secondary())
        .into()
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Movie => "Movie",
        Kind::Tv => "Show",
    }
}

/// A status pill: accent for good news, danger for a refusal, neutral for waiting.
pub(super) fn pill<'a>(b: Badge) -> El<'a> {
    let (ink, edge, bg) = match b {
        Badge::Declined | Badge::Failed => (t::danger(), t::danger(), t::danger_tint()),
        Badge::Available | Badge::Approved => (t::accent_text(), t::accent(), t::accent_tint()),
        _ => (t::text_secondary(), t::hairline_strong(), t::GLASS),
    };
    container(
        text(b.label())
            .size(t::CAPTION)
            .font(t::semibold())
            .wrapping(text::Wrapping::None)
            .color(ink),
    )
    .padding([2.0, t::S2])
    .style(move |_| container::Style {
        background: Some(bg.into()),
        border: border(edge, 1.0, 4.0),
        ..Default::default()
    })
    .into()
}

/// The tick box of a season line.
pub(super) fn check<'a>(on: bool) -> El<'a> {
    container(
        text(if on { "\u{2713}" } else { "" })
            .size(t::CAPTION)
            .font(t::bold())
            .color(t::on_accent()),
    )
    .width(18)
    .height(18)
    .center(18)
    .style(move |_| container::Style {
        background: on.then(|| t::accent().into()),
        border: border(if on { t::accent() } else { t::edge() }, 1.0, 4.0),
        ..Default::default()
    })
    .into()
}

impl App {
    pub(super) fn rq_pic(&self, path: Option<&str>) -> Pic {
        match path.and_then(|p| self.rq.posters.get(p)) {
            Some(Poster::Ready(h)) => Pic::Ready(h.clone()),
            Some(Poster::Missing) => Pic::Missing,
            _ => Pic::Loading,
        }
    }

    // ---- Settings > Seerr server ----

    pub(super) fn rq_dialog(&self) -> El<'_> {
        let Some(d) = &self.rq.dlg else {
            return Space::new().into();
        };
        let away = Message::Requests(Msg::Close);
        let mut body: Vec<El> = vec![];
        let signed = self.rq.client.as_ref().zip(self.rq.me.as_ref());
        if let Some((c, me)) = signed {
            let who = if c.is_api_key() {
                "Connected with a developer key (read only).".to_string()
            } else {
                format!("Signed in as {} on {}.", me.name(), c.base_url())
            };
            body.push(text(who).size(t::BODY).color(t::accent_text()).into());
            if let Some(v) = &self.rq.version {
                let mut line = format!("Seerr {v}");
                if v.split('.')
                    .next()
                    .and_then(|m| m.parse::<u32>().ok())
                    .is_some_and(|m| m < 3)
                {
                    line.push_str(" (older than 3.0: not tested with NOCTURNE)");
                }
                body.push(dim(line));
            }
            for k in [Kind::Movie, Kind::Tv] {
                if let Some(q) = self.rq.quota.as_ref().and_then(|q| rq::quota_text(q, k)) {
                    body.push(dim(format!("{}s: {q}.", kind_word(k))));
                }
            }
            body.push(dim(if self.settings.seerr_requests {
                "Requests are allowed. Find a title in Search and request it, or cancel your pending ones on the Requests page."
            } else {
                "Requests are off. Turn on Allow requests in Settings to send them; the Requests page works without."
            }));
            body.push(dim(rq::SENDS_TO_SEERR));
            if d.unsaved {
                body.push(
                    text("The system keyring could not keep this sign-in, so you will be asked again next time.")
                        .size(t::CONTROL_TEXT)
                        .color(t::danger())
                        .into(),
                );
            }
            body.push(
                row![
                    action_button(
                        "Sign out",
                        false,
                        true,
                        false,
                        false,
                        Message::Requests(Msg::SignOut)
                    ),
                    action_button("Close", true, false, true, false, away.clone()),
                ]
                .spacing(t::S3)
                .into(),
            );
            return overlay::panel("Seerr server", 560.0, self.win, body, away);
        }
        if let Some(code) = d.quick_code() {
            body.push(dim("Open Quick Connect in any app where you are signed in to your media server and enter this code."));
            body.push(
                text(code.to_string())
                    .size(t::DISPLAY * 0.7)
                    .font(t::MONO)
                    .color(t::text())
                    .into(),
            );
            body.push(dim(
                "This window continues by itself once the code is accepted.",
            ));
            body.push(action_button(
                "Cancel",
                false,
                false,
                true,
                false,
                away.clone(),
            ));
            return overlay::panel("Seerr server", 560.0, self.win, body, away);
        }
        let url = normalize_or(&d.url);
        body.push(dim("Request movies and shows from your Seerr server. You sign in with your own account on the media server behind it, so you get the rights Seerr's administrator gave you and nothing more."));
        let field = |ph: &'static str, v: &str, f: fn(String) -> Msg| {
            text_input(ph, v)
                .on_input(move |x| Message::Requests(f(x)))
                .on_submit(Message::Requests(Msg::SignIn))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input)
        };
        body.push(
            field(
                "Server address, like https://seerr.example.com",
                &d.url,
                Msg::Url,
            )
            .into(),
        );
        body.push(field("Username", &d.user, Msg::User).into());
        body.push(field("Password", &d.pass, Msg::Pass).secure(true).into());
        if rq::insecure_remote(&url) {
            body.push(
                text("This address starts with http://, so what you type travels unencrypted over your network.")
                    .size(t::CONTROL_TEXT)
                    .color(t::danger())
                    .into(),
            );
        }
        body.push(dim(format!(
            "Your password is sent to {} (it passes it to its own media server) and then dropped; only the session is kept, in your system keyring. Quick Connect avoids typing it.",
            if url.is_empty() { "that server".into() } else { url }
        )));
        if let Some(n) = &d.note {
            body.push(
                text(n.clone())
                    .size(t::CONTROL_TEXT)
                    .color(t::danger())
                    .into(),
            );
        }
        body.push(
            row![
                action_button(
                    "Sign in",
                    true,
                    false,
                    true,
                    d.busy,
                    Message::Requests(Msg::SignIn)
                ),
                action_button(
                    "Use Quick Connect",
                    false,
                    false,
                    false,
                    d.busy,
                    Message::Requests(Msg::Quick)
                ),
                action_button("Close", false, false, false, false, away.clone()),
            ]
            .spacing(t::S3)
            .into(),
        );
        overlay::panel("Seerr server", 560.0, self.win, body, away)
    }

    // ---- the Request dialog ----

    pub(super) fn rq_ask_dialog(&self) -> El<'_> {
        let Some(a) = &self.rq.ask else {
            return Space::new().into();
        };
        let away = Message::Requests(Msg::AskClose);
        let detail = match &a.detail {
            Load::Ready(d) => Some(d),
            _ => None,
        };
        let title = detail
            .map(|d| d.display_title())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| a.target.title.clone());
        let year = detail.and_then(|d| d.year()).or(a.target.year);
        let poster = detail
            .and_then(|d| d.poster_path.as_deref())
            .or(a.target.poster.as_deref());
        let head = row![
            tile(
                &self.rq_pic(poster),
                &title,
                Length::Fixed(96.0),
                Length::Fixed(144.0)
            ),
            column![
                text(format!("Request \u{201C}{title}\u{201D}?"))
                    .size(t::HEADING - 4.0)
                    .font(t::bold()),
                mono(
                    [
                        Some(kind_word(a.target.kind).to_string()),
                        year.map(|y| y.to_string())
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" \u{b7} ")
                ),
                container(dim(detail
                    .and_then(|d| d.overview.clone())
                    .unwrap_or_default()))
                .max_height(96)
                .clip(true),
            ]
            .spacing(t::S2)
            .width(Length::Fill),
        ]
        .spacing(t::S4);
        let mut body: Vec<El> = vec![head.into()];
        let info = detail.and_then(|d| d.media_info.as_ref());
        let can = rq::requestable(a.target.kind, info);
        if let Some(b) = self.rq_badge(a.target.kind, a.target.tmdb, info) {
            body.push(
                row![mono("STATUS"), pill(b)]
                    .spacing(t::S3)
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        if let Some(p) = &a.picker {
            body.push(rule());
            body.push(label("Seasons").into());
            if p.partial {
                body.push(menu_row(
                    row![
                        check(p.all_picked() && p.any_free()),
                        text("All seasons").size(t::BODY)
                    ]
                    .spacing(t::S3)
                    .align_y(Alignment::Center),
                    false,
                    !p.any_free(),
                    Message::Requests(Msg::AllSeasons),
                ));
            } else {
                body.push(dim("This server requests a show as a whole."));
            }
            let rows: Vec<El> = p
                .rows
                .iter()
                .map(|r| {
                    let ticked = r.taken.is_some() || p.picked.contains(&r.number) || !p.partial;
                    let name = if r.number == 0 {
                        "Specials".to_string()
                    } else {
                        format!("Season {}", r.number)
                    };
                    let mut line = row![
                        check(ticked),
                        text(name).size(t::BODY).width(Length::Fill),
                        mono(format!("{} episodes", r.episodes)),
                    ]
                    .spacing(t::S3)
                    .align_y(Alignment::Center);
                    if let Some(b) = r.taken {
                        line = line.push(pill(b));
                    }
                    let free = r.taken.is_none() && p.partial;
                    menu_row(
                        line,
                        false,
                        !free,
                        if free {
                            Message::Requests(Msg::Tick(r.number))
                        } else {
                            Message::Noop
                        },
                    )
                })
                .collect();
            body.push(
                container(scroll(column(rows).spacing(2)).style(style::scroll))
                    .max_height(220)
                    .into(),
            );
        }
        if let Some(q) = self
            .rq
            .quota
            .as_ref()
            .and_then(|q| rq::quota_text(q, a.target.kind))
        {
            body.push(dim(format!("{q}.")));
        }
        if !matches!(a.stage, AskStage::Done(_)) {
            body.push(dim(rq::SENDS_TO_SEERR));
        }
        let mut buttons = row![].spacing(t::S3);
        match &a.stage {
            AskStage::Done(r) => {
                let b = rq::badge_of_request(r.status);
                body.push(
                    text(match b {
                        Some(Badge::Approved) => "Requested and approved.",
                        _ => "Requested. It now waits for approval on your Seerr server.",
                    })
                    .size(t::BODY)
                    .color(t::accent_text())
                    .into(),
                );
                buttons = buttons.push(action_button(
                    "Close",
                    true,
                    false,
                    true,
                    false,
                    away.clone(),
                ));
            }
            stage => {
                if let AskStage::Failed(msg) = stage {
                    body.push(
                        text(msg.clone())
                            .size(t::CONTROL_TEXT)
                            .color(t::danger())
                            .into(),
                    );
                }
                let ready =
                    matches!(a.detail, Load::Ready(_)) && !matches!(stage, AskStage::Sending);
                let ticked = a.picker.as_ref().is_none_or(|p| p.seasons().is_some());
                if matches!(a.detail, Load::Ready(_)) && !can && a.picker.is_none() {
                    body.push(dim(
                        "This is already requested or available, so there is nothing to send.",
                    ));
                }
                let go = ready && can && ticked;
                buttons = buttons
                    .push(action_button(
                        if matches!(stage, AskStage::Sending) {
                            "Sending"
                        } else {
                            "Request"
                        },
                        true,
                        false,
                        go,
                        !go,
                        if go {
                            Message::Requests(Msg::Confirm)
                        } else {
                            Message::Noop
                        },
                    ))
                    .push(action_button(
                        "Cancel",
                        false,
                        false,
                        false,
                        false,
                        away.clone(),
                    ));
            }
        }
        body.push(buttons.into());
        overlay::panel("Request", 600.0, self.win, body, away)
    }

    /// "Cancel this request?" over the Requests page.
    pub(super) fn rq_cancel_dialog(&self) -> El<'_> {
        let away = Message::Requests(Msg::CancelNo);
        let body: Vec<El> = vec![
            dim(
                "The request is withdrawn on your Seerr server. Nothing is changed on your media servers.",
            ),
            row![
                action_button(
                    "Cancel request",
                    true,
                    true,
                    true,
                    false,
                    Message::Requests(Msg::CancelYes)
                ),
                action_button("Keep it", false, false, false, false, away.clone()),
            ]
            .spacing(t::S3)
            .into(),
        ];
        overlay::panel("Cancel this request?", 460.0, self.win, body, away)
    }

    // ---- the Requests page ----

    pub(super) fn requests_page(&self) -> El<'_> {
        let Some(p) = &self.rq.page else {
            return Space::new().into();
        };
        let mut page = column![section(
            "Requests",
            if self.rq.signed_in() {
                "What you asked your Seerr server for. Nothing here changes your media servers."
            } else {
                "What you asked for, kept on this computer. A request turns Available when the title shows up on one of your sources."
            },
            "Esc to go back".into(),
        )];
        page = page.push(self.rq_view_switch());
        page = match (self.rq.disc.view, self.rq.signed_in()) {
            (super::discover::View::Discover, _) => self.discover_view(page),
            (_, false) => self.native_list(page),
            _ => self.seerr_list(page, p),
        };
        scroll(page.padding(Padding {
            top: t::header_h() + t::S8,
            bottom: t::S8,
            ..Padding::ZERO
        }))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Seerr's own request list: tabs, quota, the rows.
    fn seerr_list<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
        p: &'a super::requests::PageSt,
    ) -> iced::widget::Column<'a, Message> {
        let mut tabs = row![].spacing(t::S2);
        for tab in Tab::ALL {
            tabs = tabs.push(action_button(
                tab.label(),
                p.tab == tab,
                false,
                false,
                false,
                Message::Requests(Msg::Tab(tab)),
            ));
        }
        page = page.push(container(tabs).padding([t::S3, t::page_pad()]));
        if let Some(q) = self.rq.quota.as_ref() {
            let lines: Vec<String> = [Kind::Movie, Kind::Tv]
                .into_iter()
                .filter_map(|k| rq::quota_text(q, k).map(|t| format!("{}s: {t}", kind_word(k))))
                .collect();
            if !lines.is_empty() {
                page = page.push(container(dim(lines.join("   "))).padding([0.0, t::page_pad()]));
            }
        }
        if let Some(n) = &p.note {
            page = page.push(
                container(text(n.clone()).size(t::CONTROL_TEXT).color(t::danger()))
                    .padding([t::S2, t::page_pad()]),
            );
        }
        match &p.list {
            Load::Loading => {
                page = page.push(container(dim("Loading your requests")).padding([t::S4, t::page_pad()]))
            }
            Load::Failed => {
                page = page.push(
                    container(dim("Couldn't read your requests from Seerr. Close this page and open it again to retry."))
                        .padding([t::S4, t::page_pad()]),
                )
            }
            Load::Ready(all) => {
                let shown: Vec<&Request> = all.iter().filter(|r| p.tab.admits(r)).collect();
                if shown.is_empty() {
                    page = page.push(if all.is_empty() {
                        crate::widget::empty_state(
                            "inbox",
                            "No requests yet",
                            "Search for a title that is not in your library to request it.",
                        )
                    } else {
                        crate::widget::empty_state("inbox", "Nothing in this tab", "Requests in another state show under the other tabs.")
                    });
                }
                for r in shown {
                    page = page.push(container(column![self.rq_row(r, p.busy), rule()]).padding([0.0, t::page_pad()]));
                }
            }
        }
        page
    }

    fn rq_row<'a>(&'a self, r: &'a Request, busy: Option<u64>) -> El<'a> {
        let key = r.media_kind().zip(r.tmdb_id());
        let detail = key.and_then(|k| match self.rq.titles.get(&k) {
            Some(Title::Ready(d, _)) => Some(d),
            _ => None,
        });
        let title = match (detail, &key) {
            (Some(d), _) => d.display_title(),
            (None, Some((_, id))) => match key.and_then(|k| self.rq.titles.get(&k)) {
                Some(Title::Failed) => format!("TMDB {id}"),
                _ => "Loading".to_string(),
            },
            _ => "Unknown title".into(),
        };
        let year = detail.and_then(|d| d.year());
        let sub: Vec<String> = [
            key.map(|k| kind_word(k.0).to_string()),
            year.map(|y| y.to_string()),
            Some(rq::seasons_text(r)).filter(|s| !s.is_empty()),
            Some(rq::date_text(r))
                .filter(|s| !s.is_empty())
                .map(|d| format!("requested {d}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut actions = row![].spacing(t::S2).align_y(Alignment::Center);
        if let Some(b) = rq::badge_of_request(r.status) {
            actions = actions.push(pill(b));
        }
        if r.status == 5
            && let Some((k, id)) = key
        {
            actions = actions.push(action_button(
                "Open",
                false,
                false,
                false,
                false,
                Message::Requests(Msg::OpenCatalog(k, id, title.clone(), year)),
            ));
        }
        if rq::can_cancel(r) && self.requests_on() {
            actions = actions.push(action_button(
                if busy == Some(r.id) {
                    "Cancelling"
                } else {
                    "Cancel request"
                },
                false,
                true,
                false,
                busy.is_some(),
                Message::Requests(Msg::Cancel(r.id)),
            ));
        }
        container(
            row![
                tile(
                    &self.rq_pic(detail.and_then(|d| d.poster_path.as_deref())),
                    &title,
                    Length::Fixed(56.0),
                    Length::Fixed(84.0)
                ),
                column![
                    text(title).size(t::BODY).font(t::semibold()),
                    mono(sub.join(" \u{b7} ")),
                ]
                .spacing(2)
                .width(Length::Fill),
                actions,
            ]
            .spacing(t::S4)
            .align_y(Alignment::Center),
        )
        .padding([t::S3, t::S6])
        .into()
    }

    // ---- search ----

    /// A Seerr hit as a row: the target, its second line ("Movie, 1979, Request") and the wish to show the poster.
    pub(super) fn rq_hit_line(&self, h: &nocturne_seerr::SearchHit) -> Option<(Target, String)> {
        let target = Self::rq_target_of(h)?;
        let tail = match self.rq_badge(target.kind, target.tmdb, h.media_info.as_ref()) {
            Some(b) => Some(b.label().to_string()),
            None if self.rq_can(target.kind)
                && rq::requestable(target.kind, h.media_info.as_ref()) =>
            {
                Some("Request".to_string())
            }
            None => None,
        };
        let line = [
            Some(kind_word(target.kind).to_string()),
            target.year.map(|y| y.to_string()),
            tail,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
        Some((target, line))
    }

    /// The "Request" group under the catalog's own results: titles Seerr knows that you do not have.
    pub(super) fn rq_search_group(&self, n_core: usize, sel: usize) -> Vec<El<'_>> {
        let rows = self.rq_rows();
        if rows.is_empty() {
            return vec![];
        }
        let mut out: Vec<El> = vec![
            container(label("Request"))
                .height(36)
                .padding(Padding {
                    left: t::S4,
                    top: t::S3,
                    ..Padding::ZERO
                })
                .into(),
        ];
        for (n, h) in rows.into_iter().enumerate() {
            let Some(target) = Self::rq_target_of(h) else {
                continue;
            };
            let badge = self.rq_badge(target.kind, target.tmdb, h.media_info.as_ref());
            let can =
                self.rq_can(target.kind) && rq::requestable(target.kind, h.media_info.as_ref());
            let sub = [
                Some(kind_word(target.kind).to_string()),
                target.year.map(|y| y.to_string()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
            let mut right = row![].spacing(t::S2).align_y(Alignment::Center);
            if let Some(b) = badge {
                right = right.push(pill(b));
            } else if can {
                right = right.push(mono("REQUEST"));
            }
            out.push(super::search::hit_row(
                row![
                    tile(
                        &self.rq_pic(h.poster_path.as_deref()),
                        &target.title,
                        Length::Fixed(32.0),
                        Length::Fixed(46.0)
                    ),
                    column![
                        text(target.title.clone())
                            .size(t::BODY)
                            .font(t::semibold())
                            .wrapping(text::Wrapping::None),
                        mono(sub).wrapping(text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .clip(true),
                    right,
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center)
                .into(),
                n_core + n == sel,
                Message::Search(super::search::SMsg::Pick(n_core + n)),
            ));
        }
        out
    }

    // ---- a detail page ----

    /// The Seerr badge over a movie or show page (and "Request" for seasons nobody asked for yet).
    pub(super) fn rq_chip_view(&self) -> Option<El<'_>> {
        let c = self.rq.chip.as_ref()?;
        if self.stack.last().is_none_or(|p| p.pid != c.pid) {
            return None;
        }
        let mut r = row![].spacing(t::S2).align_y(Alignment::Center);
        if let Some(b) = c.badge {
            r = r.push(mono("SEERR")).push(pill(b));
        }
        if c.more && self.rq_can(c.target.kind) {
            let target: Target = c.target.clone();
            r = r.push(action_button(
                "Request seasons",
                false,
                false,
                false,
                false,
                Message::Requests(Msg::Ask(Box::new(target))),
            ));
        }
        Some(
            container(
                container(r)
                    .padding([t::S2, t::S3])
                    .style(style::raised_card),
            )
            .width(Length::Fill)
            .align_x(Alignment::End)
            .padding(Padding {
                top: t::header_h() + t::S3,
                right: t::S6,
                ..Padding::ZERO
            })
            .into(),
        )
    }

    /// Everything of this feature that floats over the app: its dialogs, last so they sit on top.
    pub(super) fn rq_layers<'a>(&'a self, layers: &mut Vec<El<'a>>) {
        if self.rq.dlg.is_some() {
            layers.push(self.rq_dialog());
        }
        if self.rq.ask.is_some() {
            layers.push(self.rq_ask_dialog());
        }
        if self.rq.page.as_ref().is_some_and(|p| p.cancel.is_some()) {
            layers.push(self.rq_cancel_dialog());
        }
        self.disc_layers(layers);
    }
}

/// The address as it will be used, or the text as typed.
fn normalize_or(s: &str) -> String {
    nocturne_seerr::normalize_url(s).unwrap_or_default()
}
