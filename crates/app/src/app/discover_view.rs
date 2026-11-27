//! Painting for `discover.rs`: the view switch of the Requests page, Discover (search, filters, cards),
//! the list kept on this computer, the request dialog and the Radarr / Sonarr connect dialog.
//! Copy says "request"; colours come from the theme tokens.
use super::discover::{Msg, View};
use super::message::Message;
use super::requests_view::{check, pill};
use super::{App, Load};
use crate::style;
use crate::theme as t;
use crate::widget::cell::tile;
use crate::widget::{action_button, label, menu_row, mono, overlay, rule};
use iced::widget::{Column, Space, column, container, row, scrollable as scroll, text, text_input};
use iced::{Alignment, Element, Length};
use nocturne_arr::Service;
use nocturne_core::discover::{self as dc, Card, List, Scope};
use nocturne_core::requests::Badge;
use nocturne_core::wishlist::{self, Entry};
use nocturne_seerr::Kind;

type El<'a> = Element<'a, Message>;

const CARD_W: f32 = 160.0;
const CARD_H: f32 = 240.0;

fn dim<'a>(s: impl Into<String>) -> El<'a> {
    text(s.into())
        .size(t::CONTROL_TEXT)
        .line_height(1.5)
        .color(t::text_secondary())
        .into()
}

fn msg(m: Msg) -> Message {
    Message::Discover(m)
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Movie => "Movie",
        Kind::Tv => "Show",
    }
}

fn pad<'a>(e: impl Into<El<'a>>) -> El<'a> {
    container(e).padding([t::S2, t::page_pad()]).into()
}

impl App {
    /// "My requests | Discover" under the page title.
    pub(super) fn rq_view_switch(&self) -> El<'_> {
        let v = self.rq.disc.view;
        pad(row![
            action_button(
                "My requests",
                v == View::Mine,
                false,
                false,
                false,
                msg(Msg::View(View::Mine))
            ),
            action_button(
                "Discover",
                v == View::Discover,
                false,
                false,
                false,
                msg(Msg::View(View::Discover))
            ),
        ]
        .spacing(t::S2))
    }

    // ---- Discover ----

    pub(super) fn discover_view<'a>(
        &'a self,
        mut page: Column<'a, Message>,
    ) -> Column<'a, Message> {
        let d = &self.rq.disc;
        let f = &d.filter;
        // search + scope
        let input = text_input("Search movies and shows", &d.query)
            .on_input(|q| msg(Msg::Query(q)))
            .on_submit(msg(Msg::Search))
            .size(t::BODY)
            .padding([t::S2, t::S3])
            .style(style::input)
            .width(Length::Fixed(360.0));
        let mut top = row![
            input,
            action_button("Search", true, false, false, false, msg(Msg::Search))
        ]
        .spacing(t::S2)
        .align_y(Alignment::Center);
        if f.searching() {
            top = top.push(action_button(
                "Clear",
                false,
                false,
                false,
                false,
                msg(Msg::ClearSearch),
            ));
        }
        top = top.push(Space::new().width(t::S4));
        for s in Scope::ALL {
            top = top.push(action_button(
                s.label(),
                f.scope == s,
                false,
                false,
                false,
                msg(Msg::Scope(s)),
            ));
        }
        page = page.push(pad(top));
        if !f.searching() {
            // lists and the year
            let mut lists = row![].spacing(t::S2).align_y(Alignment::Center);
            for l in List::ALL {
                lists = lists.push(action_button(
                    l.label(),
                    f.list == l && !f.narrowed(),
                    false,
                    false,
                    false,
                    msg(Msg::List(l)),
                ));
            }
            lists = lists
                .push(Space::new().width(t::S4))
                .push(mono("YEAR"))
                .push(
                    text_input("Any", &d.year_text)
                        .on_input(|y| msg(Msg::Year(y)))
                        .size(t::BODY)
                        .padding([t::S2, t::S3])
                        .style(style::input)
                        .width(Length::Fixed(84.0)),
                );
            page = page.push(pad(lists));
            let mut chips: Vec<El> = vec![action_button(
                "Any genre",
                f.genre.is_none(),
                false,
                false,
                false,
                msg(Msg::Genre(None)),
            )];
            for g in dc::genres(f.scope) {
                chips.push(action_button(
                    g.name,
                    f.genre == Some(g.id),
                    false,
                    false,
                    false,
                    msg(Msg::Genre(Some(g.id))),
                ));
            }
            let genres = row(chips).spacing(t::S2).wrap().vertical_spacing(t::S2);
            page = page.push(pad(genres));
            if f.narrowed() {
                page = page.push(pad(dim("Genre and year narrow the Popular list.")));
            }
        }
        if let Some(src) = d.src {
            page = page.push(pad(
                row![mono("FROM"), mono(src.label().to_uppercase())].spacing(t::S2)
            ));
        }
        let have = self.disc_have();
        match (&d.cards, d.src) {
            (_, None) if d.sources_ready() => {
                page = page.push(pad(dim(dc::connect_text(have))));
            }
            (None, _) | (Some(Load::Loading), _) => page = page.push(pad(dim("Loading"))),
            (Some(Load::Failed), _) => {
                page = page.push(pad(row![
                    dim(d
                        .note
                        .clone()
                        .unwrap_or_else(|| "Could not load this list.".into())),
                    action_button("Try again", false, false, false, false, msg(Msg::Retry)),
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center)));
            }
            (Some(Load::Ready(cards)), _) => {
                if cards.is_empty() {
                    page = page.push(pad(dim(if f.searching() {
                        "Nothing found for that search."
                    } else {
                        "Nothing in this list."
                    })));
                }
                let grid = row(cards.iter().map(|c| self.card_view(c)))
                    .spacing(t::S4)
                    .wrap()
                    .vertical_spacing(t::S6);
                page = page.push(pad(grid));
                if let Some(n) = &d.note {
                    page = page.push(pad(text(n.clone())
                        .size(t::CONTROL_TEXT)
                        .color(t::danger())));
                }
                if d.more {
                    page = page.push(pad(action_button(
                        if d.loading_more {
                            "Loading"
                        } else {
                            "Load more"
                        },
                        false,
                        false,
                        false,
                        d.loading_more,
                        msg(Msg::More),
                    )));
                }
            }
        }
        page
    }

    fn card_view<'a>(&'a self, c: &'a Card) -> El<'a> {
        let badge = self.card_badge(c);
        let on_server = matches!(self.rq.disc.avail.get(&c.key()), Some(Some(_)));
        let sub: Vec<String> = [
            Some(kind_word(c.kind()).to_string()),
            c.year.map(|y| y.to_string()),
            c.rating.map(|r| format!("\u{2605} {r:.1}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let status: El = match badge {
            Some(Badge::Available) if on_server => text(dc::card_label(badge))
                .size(t::CAPTION)
                .font(t::semibold())
                .color(t::accent_text())
                .into(),
            Some(b) => pill(b),
            None => text(dc::card_label(None))
                .size(t::CAPTION)
                .color(t::text_tertiary())
                .into(),
        };
        let action: Option<El> = if badge == Some(Badge::Available) && on_server {
            Some(action_button(
                "Open",
                false,
                false,
                false,
                false,
                msg(Msg::Open(c.key())),
            ))
        } else if self.card_can_ask(c) {
            Some(action_button(
                "Request",
                true,
                false,
                false,
                false,
                msg(Msg::Ask(Box::new(c.clone()))),
            ))
        } else {
            None
        };
        let mut col = column![
            tile(
                &self.rq_pic(c.poster.as_deref()),
                &c.title,
                Length::Fixed(CARD_W),
                Length::Fixed(CARD_H)
            ),
            text(c.title.clone())
                .size(t::BODY)
                .font(t::semibold())
                .wrapping(text::Wrapping::None),
            mono(sub.join(" \u{b7} ")).wrapping(text::Wrapping::None),
            status,
        ]
        .spacing(t::S1)
        .width(Length::Fixed(CARD_W));
        if let Some(a) = action {
            col = col.push(Space::new().height(t::S1)).push(a);
        }
        container(col).clip(true).into()
    }

    fn disc_have(&self) -> dc::Have {
        let d = &self.rq.disc;
        dc::Have {
            seerr: self.rq.client.is_some(),
            tmdb: d.has_tmdb(),
            trakt: d.has_trakt(),
            arr: !d.arr.is_empty(),
        }
    }

    // ---- the list on this computer ----

    pub(super) fn native_list<'a>(&'a self, mut page: Column<'a, Message>) -> Column<'a, Message> {
        let w = &self.rq.disc.wish;
        if w.entries.is_empty() {
            return page.push(pad(dim(
                "You have not requested anything yet. Open Discover to find a title, or use Request on a Calendar entry.",
            )));
        }
        for e in &w.entries {
            page = page
                .push(container(column![self.native_row(e), rule()]).padding([0.0, t::page_pad()]));
        }
        page
    }

    fn native_row<'a>(&'a self, e: &'a Entry) -> El<'a> {
        let kind = if e.tv { Kind::Tv } else { Kind::Movie };
        let sub: Vec<String> = [
            Some(kind_word(kind).to_string()),
            e.year.map(|y| y.to_string()),
            Some(e.seasons_text()).filter(|s| !s.is_empty()),
            e.via.as_ref().map(|v| format!("in {v}")),
            Some(format!("requested {}", wishlist::date_text(e.requested))),
        ]
        .into_iter()
        .flatten()
        .collect();
        let badge = dc::native_badge(e.status);
        let mut actions = row![pill(badge)].spacing(t::S2).align_y(Alignment::Center);
        if e.status == wishlist::Status::Available {
            actions = actions.push(action_button(
                "Open",
                false,
                false,
                false,
                false,
                Message::Requests(super::requests::Msg::OpenCatalog(
                    kind,
                    e.tmdb,
                    e.title.clone(),
                    e.year,
                )),
            ));
        }
        actions = actions.push(action_button(
            "Cancel request",
            false,
            true,
            false,
            false,
            msg(Msg::Remove(e.tv, e.tmdb)),
        ));
        container(
            row![
                tile(
                    &self.rq_pic(e.poster.as_deref()),
                    &e.title,
                    Length::Fixed(56.0),
                    Length::Fixed(84.0)
                ),
                column![
                    text(e.title.clone()).size(t::BODY).font(t::semibold()),
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

    // ---- dialogs ----

    /// Everything of this feature that floats over the app.
    pub(super) fn disc_layers<'a>(&'a self, layers: &mut Vec<El<'a>>) {
        if self.rq.disc.ask.is_some() {
            layers.push(self.nat_dialog());
        }
        if self.rq.disc.dlg.is_some() {
            layers.push(self.arr_dialog());
        }
    }

    /// The request dialog without Seerr.
    fn nat_dialog(&self) -> El<'_> {
        let Some(a) = &self.rq.disc.ask else {
            return Space::new().into();
        };
        let away = msg(Msg::Close);
        let head = row![
            tile(
                &self.rq_pic(a.target.poster.as_deref()),
                &a.target.title,
                Length::Fixed(96.0),
                Length::Fixed(144.0)
            ),
            column![
                text(format!("Request \u{201C}{}\u{201D}?", a.target.title))
                    .size(t::HEADING - 4.0)
                    .font(t::bold()),
                mono(
                    [
                        Some(kind_word(a.target.kind).to_string()),
                        a.target.year.map(|y| y.to_string())
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" \u{b7} ")
                ),
                container(dim(a
                    .card
                    .as_ref()
                    .map(|c| c.overview.clone())
                    .unwrap_or_default()))
                .max_height(96)
                .clip(true),
            ]
            .spacing(t::S2)
            .width(Length::Fill),
        ]
        .spacing(t::S4);
        let mut body: Vec<El> = vec![head.into()];
        if let Some(b) = self.native_badge(a.target.kind, a.target.tmdb) {
            body.push(
                row![mono("STATUS"), pill(b)]
                    .spacing(t::S3)
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        if a.seasons_loading {
            body.push(dim("Reading the seasons"));
        }
        if let Some(p) = &a.picker {
            body.push(rule());
            body.push(label("Seasons").into());
            body.push(menu_row(
                row![
                    check(p.all_picked() && p.any_free()),
                    text("All seasons").size(t::BODY)
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
                false,
                !p.any_free(),
                msg(Msg::All),
            ));
            let rows: Vec<El> = p
                .rows
                .iter()
                .map(|r| {
                    let ticked = r.taken.is_some() || p.picked.contains(&r.number);
                    let mut line = row![
                        check(ticked),
                        text(format!("Season {}", r.number))
                            .size(t::BODY)
                            .width(Length::Fill)
                    ]
                    .spacing(t::S3)
                    .align_y(Alignment::Center);
                    if r.episodes > 0 {
                        line = line.push(mono(format!("{} episodes", r.episodes)));
                    }
                    if let Some(b) = r.taken {
                        line = line.push(pill(b));
                    }
                    let free = r.taken.is_none();
                    menu_row(
                        line,
                        false,
                        !free,
                        if free {
                            msg(Msg::Tick(r.number))
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
        if a.done().is_none() {
            body.push(dim(match a.via {
            Some(s) => format!(
                "This adds the request to your list on this computer and to your {}, which will look for it. Nothing is changed on your media servers.",
                s.name()
            ),
            None => "This adds the request to your list on this computer. Nothing is sent anywhere, and nothing is changed on your media servers.".to_string(),
        }));
        }
        let mut buttons = row![].spacing(t::S3);
        if let Some(done) = a.done() {
            body.push(
                text(done.to_string())
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
        } else {
            if let Some(m) = a.failed() {
                body.push(
                    text(m.to_string())
                        .size(t::CONTROL_TEXT)
                        .color(t::danger())
                        .into(),
                );
            }
            let ticked = a.picker.as_ref().is_none_or(|p| p.seasons().is_some());
            let go = !a.sending() && !a.seasons_loading && ticked;
            buttons = buttons
                .push(action_button(
                    if a.sending() { "Sending" } else { "Request" },
                    true,
                    false,
                    go,
                    !go,
                    if go { msg(Msg::Confirm) } else { Message::Noop },
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
        body.push(buttons.into());
        overlay::panel("Request", 600.0, self.win, body, away)
    }

    /// Settings > Radarr / Sonarr.
    fn arr_dialog(&self) -> El<'_> {
        let Some(d) = &self.rq.disc.dlg else {
            return Space::new().into();
        };
        let away = msg(Msg::ArrClose);
        let name = d.service.name();
        let what = match d.service {
            Service::Radarr => "movies",
            Service::Sonarr => "shows",
        };
        let mut body: Vec<El> = vec![dim(format!(
            "Connect your own {name} to have requested {what} added to it. NOCTURNE adds a request only after you confirm it in the request dialog, and only when \u{201C}Add requests to Radarr and Sonarr\u{201D} is on in Settings. The API key stays in your system keyring."
        ))];
        let field = |ph: &'static str, v: &str, f: fn(String) -> Msg| {
            text_input(ph, v)
                .on_input(move |x| msg(f(x)))
                .on_submit(msg(Msg::ArrTest))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input)
        };
        body.push(
            field(
                "Address, like http://192.168.1.10:7878",
                &d.url,
                Msg::ArrUrl,
            )
            .into(),
        );
        body.push(
            field(
                if self.rq.disc.arr.contains_key(&d.service) {
                    "API key (saved; type a new one to replace it)"
                } else {
                    "API key (in the service under Settings, General)"
                },
                &d.key,
                Msg::ArrKey,
            )
            .secure(true)
            .into(),
        );
        if nocturne_core::requests::insecure_remote(&d.url) {
            body.push(
                text("This address starts with http://, so the API key travels unencrypted over your network.")
                    .size(t::CONTROL_TEXT)
                    .color(t::danger())
                    .into(),
            );
        }
        if let Some(t) = &d.tested {
            body.push(
                text(format!("Connected to {name} {}.", t.version))
                    .size(t::BODY)
                    .color(t::accent_text())
                    .into(),
            );
            let pick = |label: &str, value: String, m: Msg| -> El {
                row![
                    mono(label.to_string()),
                    action_button(&value, false, false, false, false, msg(m))
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center)
                .into()
            };
            if let Some(p) = t.profiles.get(d.profile) {
                body.push(pick("QUALITY PROFILE", p.name.clone(), Msg::ArrProfile));
            }
            if let Some(f) = t.folders.get(d.folder) {
                body.push(pick("ROOT FOLDER", f.path.clone(), Msg::ArrFolder));
            }
            body.push(dim(
                "Click a choice to see the next one. The first is used by default.",
            ));
        }
        if let Some(n) = &d.note {
            body.push(
                text(n.clone())
                    .size(t::CONTROL_TEXT)
                    .color(t::danger())
                    .into(),
            );
        }
        let mut buttons = row![action_button(
            "Test connection",
            d.tested.is_none(),
            false,
            true,
            d.busy,
            msg(Msg::ArrTest)
        ),]
        .spacing(t::S3);
        if d.tested.is_some() {
            buttons = buttons.push(action_button(
                "Save",
                true,
                false,
                false,
                false,
                msg(Msg::ArrSave),
            ));
        }
        if !self.arr_cfg_url(d.service).is_empty() {
            buttons = buttons.push(action_button(
                "Disconnect",
                false,
                true,
                false,
                false,
                msg(Msg::ArrDisconnect),
            ));
        }
        buttons = buttons.push(action_button(
            "Close",
            false,
            false,
            false,
            false,
            away.clone(),
        ));
        body.push(buttons.into());
        overlay::panel(name, 560.0, self.win, body, away)
    }

    fn arr_cfg_url(&self, s: Service) -> &str {
        match s {
            Service::Radarr => &self.settings.radarr.url,
            Service::Sonarr => &self.settings.sonarr.url,
        }
    }
}
