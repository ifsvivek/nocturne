//! The detail page body: hero, facts, chips, about, the episode / track list, strips, media info.
//! Every band has the height `geom` says, so focus can scroll rows into view without measuring.
use super::cells::{self, Cell, g};
use super::geom::{self, TOP};
use super::model::{self, Chip, DAct, DRow, Sec};
use super::{Detail, Load, Msg};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{band, chip, mono, pill_button_dim, rule, section, tab, tip};
use iced::widget::button::{Status, Style};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Stack, column, container, image, row, scrollable, space, text};
use iced::{Alignment, Background, Color, ContentFit, Degrees, Element, Length, Padding, Shadow};
use nocturne_core::backend::PlayError;
use nocturne_core::detail::{self, Full, StreamRow};
use nocturne_core::source::{Art, Kind, Media, fmt_runtime};
use nocturne_core::sync::NEED_WRITES;

type El<'a> = Element<'a, Msg>;

pub(super) const PAGE_ID: &str = "detail-page";

pub(super) fn strip_id(s: Sec) -> String {
    format!("detail-strip-{}", s.idx())
}

/// Secondary line of a poster cell.
pub(super) fn caption(m: &Media) -> String {
    let second = match m.kind {
        Kind::Episode => m.episode_label(),
        Kind::MusicAlbum | Kind::Audio => m.sub.clone().unwrap_or_default(),
        _ => m.kind_label(),
    };
    [second, m.meta_line()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

fn frame(a: f32) -> Color {
    Color { a, ..t::FRAME }
}

impl Detail {
    pub fn view(&self) -> El<'_> {
        let mut layers: Vec<El<'_>> = vec![self.page()];
        layers.extend(self.overlays());
        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn page(&self) -> El<'_> {
        let m = &self.media;
        let empty = Full::default();
        let full = self.full.as_ref().unwrap_or(&empty);
        let shape = self.shape();
        let cur = shape.current(&self.focus);
        let at = |r: DRow| (cur == r).then_some(self.focus.col);
        let mut page = column![
            space().height(TOP),
            self.hero(at(DRow::Actions)),
            rule(),
            band(self.facts(full)),
        ]
        .width(Length::Fill);
        let chips = self.chips();
        if !chips.is_empty() {
            page = page.push(band(self.chips_band(chips, at(DRow::Chips))));
        }
        page = page.push(band(self.about(full)));
        if model::has_list(m.kind) {
            page = self.list(page, cur == DRow::List);
        }
        for s in Sec::ALL {
            let items = self.sec_items(s);
            if !items.is_empty() {
                page = page.push(self.strip(s, items, at(DRow::Strip(s))));
            }
        }
        if m.kind == Kind::Person {
            let all = [&self.kids, &self.kids2];
            if all.iter().all(|l| l.items().is_empty()) {
                let note: El = if all.iter().any(|l| matches!(l, Load::Loading)) {
                    mono("loading").into()
                } else if all.iter().any(|l| matches!(l, Load::Failed)) {
                    g("This couldn't be loaded.")
                        .color(t::TEXT_SECONDARY)
                        .into()
                } else {
                    g("Nothing in your library features this person.")
                        .color(t::TEXT_SECONDARY)
                        .into()
                };
                page = page.push(band(container(note).padding(t::S6).height(geom::MSG_H)));
            }
        }
        if !full.info.is_empty() {
            page = page.push(band(
                container(info_table(full)).padding([t::S8, t::PAGE_PAD]),
            ));
        }
        page = page.push(space().height(t::S8 * 2.0));
        scrollable(page)
            .id(PAGE_ID)
            .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y, v.bounds().height))
            .style(style::scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ---- hero ----

    fn hero(&self, focus: Option<usize>) -> El<'_> {
        let m = &self.media;
        let h = geom::hero_h(self.win);
        let w = self.win.width;
        let art = geom::hero_art(m);
        let backdrop: El = match (self.img(m, art, 1920), self.blur(m, art)) {
            (Some(i), _) | (None, Some(i)) => image(i.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Cover)
                .into(),
            _ => cells::tile(None, None, "", 0.0),
        };
        let mut meta = row![chip(&m.kind_label(), true)]
            .spacing(t::S2)
            .align_y(Alignment::Center);
        if let Some(y) = m.year {
            meta = meta.push(chip(&y.to_string(), false));
        }
        if let Some(r) = m.runtime_min {
            meta = meta.push(chip(&fmt_runtime(r), false));
        }
        for gname in m.genres.iter().take(3) {
            meta = meta.push(chip(gname, false));
        }
        let size = (w / 26.0).clamp(40.0, 72.0);
        let logo = m
            .logo
            .is_some()
            .then(|| self.img(m, Art::Logo, 800))
            .flatten();
        let title: El = match (&m.logo, logo) {
            (_, Some(l)) => image(l.clone())
                .height(110)
                .content_fit(ContentFit::Contain)
                .into(),
            (Some(_), None) if self.pics.contains_key(&super::pic_key(m, Art::Logo, 800)) => {
                self.title_text(size)
            }
            (Some(_), None) => space().height(110).into(),
            _ => self.title_text(size),
        };
        let eyebrow = if m.kind == Kind::Episode {
            format!(
                "0x01 \u{b7} {} \u{b7} {}",
                m.series_name.clone().unwrap_or_default(),
                m.episode_label()
            )
        } else {
            format!("0x01 \u{b7} {}", m.kind_label())
        };
        let mut head = row![mono(eyebrow)]
            .spacing(t::S3)
            .align_y(Alignment::Center);
        if self.marks.len() > 1
            && let Some(k) = self.mark_of(m.origin)
        {
            head = head.push(
                row![
                    crate::widget::dot(k.color),
                    mono(k.label.clone()).color(t::TEXT_SECONDARY)
                ]
                .spacing(t::S1 + 2.0)
                .align_y(Alignment::Center),
            );
        }
        let sub = m.overview.clone().unwrap_or_else(|| m.meta_line());
        let mut text_col = column![head, meta, title].spacing(t::S4).max_width(680);
        if m.logo.is_some() && matches!(m.kind, Kind::Season | Kind::Episode) {
            text_col = text_col.push(
                g(m.title.clone())
                    .size(t::HEADING)
                    .font(t::SEMIBOLD)
                    .color(t::TEXT),
            );
        }
        text_col = text_col.push(
            container(
                g(sub)
                    .size(t::BODY_LG)
                    .line_height(1.6)
                    .color(t::TEXT_SECONDARY),
            )
            .max_width(560)
            .height(3.0 * t::BODY_LG * 1.6)
            .clip(true),
        );
        text_col = text_col.push(self.buttons(focus));
        if let Some(e) = self.play_err {
            text_col = text_col.push(g(e.message()).size(t::BODY).color(t::WARN));
        } else if !m.available {
            text_col = text_col.push(g(PlayError::Unavailable.message()).color(t::WARN));
        }
        let shade = |deg: f32, from: (f32, Color), to: (f32, Color)| {
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Gradient(
                        iced::gradient::Linear::new(Degrees(deg))
                            .add_stop(from.0, from.1)
                            .add_stop(to.0, to.1)
                            .into(),
                    )),
                    ..Default::default()
                })
        };
        Stack::with_children([
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::fill(t::SURFACE))
                .into(),
            backdrop,
            shade(90.0, (0.0, frame(0.94)), (0.65, frame(0.0))).into(),
            shade(180.0, (0.45, frame(0.0)), (1.0, frame(1.0))).into(),
            container(text_col)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(Alignment::End)
                .padding(Padding {
                    left: t::PAGE_PAD,
                    bottom: t::S8,
                    ..Padding::ZERO
                })
                .into(),
        ])
        .width(Length::Fill)
        .height(h)
        .into()
    }

    fn title_text(&self, size: f32) -> El<'_> {
        g(self.media.title.clone())
            .size(size)
            .line_height(1.08)
            .font(t::BOLD)
            .color(t::TEXT)
            .into()
    }

    fn buttons(&self, focus: Option<usize>) -> El<'_> {
        let m = &self.media;
        let list = self.versions();
        let mut r: Vec<El<'_>> = vec![];
        for (i, a) in self.dacts().into_iter().enumerate() {
            let (label, primary) = model::spec(a, m);
            let label = if a == DAct::Versions {
                let v = list.get(self.picked(&list));
                // Quality and codec only: the full line is in the picker.
                let s = v.map_or(String::new(), |v| {
                    if v.detail.is_empty() {
                        self.mark_of(v.origin)
                            .map_or(String::new(), |k| k.label.clone())
                    } else {
                        v.detail
                            .split(" \u{b7} ")
                            .take(2)
                            .collect::<Vec<_>>()
                            .join(" \u{b7} ")
                    }
                });
                format!(
                    "{} \u{25BE}",
                    if s.is_empty() { label.to_string() } else { s }
                )
            } else {
                label.to_string()
            };
            let off = model::is_write(a) && !self.can_write(m);
            let b = pill_button_dim(&label, primary, focus == Some(i), off, Msg::Act(a));
            r.push(if off { tip(b, NEED_WRITES) } else { b });
        }
        if m.kind == Kind::Book {
            r.push(
                container(g("Not playable in NOCTURNE yet.").color(t::TEXT_SECONDARY))
                    .padding([t::S3, 0.0])
                    .into(),
            );
        }
        row(r).spacing(t::S3).wrap().vertical_spacing(t::S3).into()
    }

    // ---- bands ----

    fn facts(&self, f: &Full) -> El<'_> {
        let m = &self.media;
        let mut line = row![].spacing(t::S3).align_y(Alignment::Center);
        if let Some(r) = m.rating {
            let (full, half, empty) = detail::stars(r);
            line = line
                .push(row![
                    g("\u{2605}".repeat(full as usize))
                        .size(t::BODY)
                        .color(t::ACCENT_TEXT),
                    g(if half { "\u{2605}" } else { "" })
                        .size(t::BODY)
                        .color(Color {
                            a: 0.5,
                            ..t::ACCENT_TEXT
                        }),
                    g("\u{2606}".repeat(empty as usize))
                        .size(t::BODY)
                        .color(t::TEXT_TERTIARY),
                ])
                .push(mono(format!("{r:.1}")));
        }
        if let Some(c) = f.critic {
            line = line.push(mono(format!("{c:.0}% critics")));
        }
        if let Some(r) = f.official_rating.as_deref() {
            line = line.push(chip(r, false));
        }
        if let Some(y) = m.year {
            line = line.push(mono(y.to_string()));
        }
        if let Some(r) = m.runtime_min {
            line = line.push(mono(fmt_runtime(r)));
            if matches!(m.kind, Kind::Movie | Kind::Episode | Kind::Video) {
                let left = m
                    .progress()
                    .map_or(r, |p| ((1.0 - p) * r as f32).round() as u32);
                line = line.push(mono(format!(
                    "ends {}",
                    detail::ends_at(detail::local_minutes(), left)
                )));
            }
        }
        if m.kind == Kind::Series
            && let Some(n) = m.unplayed.filter(|n| *n > 0)
        {
            line = line.push(mono(format!("{n} unwatched")));
        }
        if let Some(n) = m
            .child_count
            .filter(|_| matches!(m.kind, Kind::BoxSet | Kind::Playlist | Kind::MusicAlbum))
        {
            line = line.push(mono(format!("{n} items")));
        }
        container(line)
            .padding([0.0, t::PAGE_PAD])
            .height(geom::FACTS_H)
            .center_y(geom::FACTS_H)
            .into()
    }

    fn chips_band(&self, chips: Vec<Chip>, focus: Option<usize>) -> El<'_> {
        let mut r: Vec<El<'_>> = vec![];
        for (i, c) in chips.into_iter().enumerate() {
            let (label, link) = match &c {
                Chip::Genre(x) => (x.clone(), false),
                Chip::Studio(_, n) => (n.clone(), false),
                Chip::Link(n, _) => (format!("{n} \u{2197}"), true),
            };
            let on = focus == Some(i);
            r.push(
                Press::new(container(g(label).size(t::CAPTION + 1.0)).center_y(28))
                    .height(28)
                    .padding([0.0, t::S3])
                    .on_press(Msg::Chip(i))
                    .style(move |s| {
                        let hot = matches!(s, Status::Hovered | Status::Pressed);
                        Style {
                            background: Some(Background::Color(if on {
                                t::ACCENT_TINT
                            } else {
                                t::GLASS
                            })),
                            text_color: if link {
                                t::ACCENT_TEXT
                            } else {
                                t::TEXT_SECONDARY
                            },
                            border: border(
                                if on || hot {
                                    t::ACCENT
                                } else {
                                    t::HAIRLINE_STRONG
                                },
                                1.0,
                                14.0,
                            ),
                            shadow: Shadow::default(),
                            snap: false,
                        }
                    })
                    .into(),
            );
        }
        container(row(r).spacing(t::S2).wrap().vertical_spacing(t::S2))
            .padding(Padding {
                left: t::PAGE_PAD,
                right: t::PAGE_PAD,
                bottom: t::S4,
                top: 0.0,
            })
            .height(geom::CHIPS_H)
            .into()
    }

    fn about(&self, f: &Full) -> El<'_> {
        let m = &self.media;
        let mut left = column![].spacing(t::S3).width(Length::Fill);
        if let Some(tag) = &f.tagline {
            left = left.push(
                g(tag.clone())
                    .size(t::BODY_LG)
                    .font(t::SEMIBOLD)
                    .color(t::ACCENT_TEXT),
            );
        }
        let overview = m
            .overview
            .clone()
            .unwrap_or_else(|| "No description available.".into());
        left = left.push(
            container(
                g(overview)
                    .size(t::BODY_LG)
                    .line_height(1.6)
                    .color(t::TEXT_SECONDARY),
            )
            .max_width(760),
        );
        let mut right = column![].spacing(t::S2).width(300);
        for (k, v) in geom::facts(m, f) {
            right = right.push(
                row![
                    mono(k.to_uppercase()),
                    space().width(Length::Fill),
                    container(g(v).size(t::CAPTION + 1.0).color(t::TEXT_SECONDARY))
                        .max_width(210)
                        .clip(true),
                ]
                .spacing(t::S4),
            );
        }
        let mut r = row![].spacing(t::S8);
        if geom::about_poster(m) {
            r = r.push(
                container(cells::tile(
                    self.img(m, Art::Poster, 480),
                    self.blur(m, Art::Poster),
                    &m.title,
                    t::R_XS,
                ))
                .width(180)
                .height(240),
            );
        }
        let h = geom::about_h(m, f, self.win.width);
        container(r.push(left).push(right))
            .padding([t::S6, t::PAGE_PAD])
            .height(h)
            .clip(true)
            .into()
    }

    // ---- list ----

    fn list<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Msg>,
        focused: bool,
    ) -> iced::widget::Column<'a, Msg> {
        let m = &self.media;
        let (heading, sub) = match m.kind {
            Kind::Series => ("Episodes", "Pick a season, then an episode."),
            Kind::Season => ("Episodes", "Everything in this season."),
            Kind::MusicAlbum => ("Tracks", "In album order."),
            Kind::MusicArtist => ("Albums", "Everything by this artist."),
            Kind::Playlist => ("Playlist", "In the order it plays."),
            _ => ("In this collection", "Everything grouped here."),
        };
        let meta = if m.kind == Kind::Series {
            match self.seasons.items().len() {
                1 => "1 season".to_string(),
                n => format!("{n} seasons"),
            }
        } else {
            self.episodes.items().len().to_string()
        };
        let mut head = column![section(heading, sub, meta)].spacing(t::S4);
        let mut head_h = 2.0 * t::S8 + geom::HEAD_BLOCK;
        if m.kind == Kind::Series {
            head_h += geom::TABS_H;
            let mut tabs = row![].spacing(t::S1);
            let shape = self.shape();
            let on_tabs = shape.current(&self.focus) == DRow::Seasons;
            for (i, s) in self.seasons.items().iter().enumerate() {
                let n = match s.unplayed {
                    Some(0) => " \u{2713}".to_string(),
                    Some(n) => format!("  {n}"),
                    None => String::new(),
                };
                let t = tab(
                    &format!("{}{n}", s.title),
                    i == self.focus.season,
                    Msg::SeasonTab(i),
                );
                let on = on_tabs && i == self.focus.season;
                tabs = tabs.push(container(t).style(move |_| container::Style {
                    background: on.then_some(t::ACCENT_TINT.into()),
                    ..Default::default()
                }));
            }
            head = head.push(column![
                container(tabs).padding([0.0, t::PAGE_PAD]).height(36),
                rule()
            ]);
        }
        page = page.push(band(container(head).padding([t::S8, 0.0]).height(head_h)));
        let w = self.win.width;
        match &self.episodes {
            Load::Loading => page.push(band(
                container(mono("loading"))
                    .padding(t::S6)
                    .height(geom::MSG_H),
            )),
            Load::Failed => page.push(band(
                container(g("This couldn't be loaded.").color(t::TEXT_SECONDARY))
                    .padding(t::S6)
                    .height(geom::MSG_H),
            )),
            Load::Ready(v) if v.is_empty() => page.push(band(
                container(
                    g(if m.kind == Kind::Series {
                        "No episodes have been added yet."
                    } else {
                        "Nothing in here yet."
                    })
                    .color(t::TEXT_SECONDARY),
                )
                .padding(t::S6)
                .height(geom::MSG_H),
            )),
            Load::Ready(v) if model::card_grid(m.kind) => {
                let (cols, cw) = (geom::card_cols(w), geom::card_w(w));
                let now = detail::local_minutes();
                for (r, chunk) in v.chunks(cols).enumerate() {
                    let mut line = row![].width(Length::Fill);
                    for (c, e) in chunk.iter().enumerate() {
                        let i = r * cols + c;
                        let (img, blur) = (
                            self.img(e, Art::Landscape, geom::CARD_PX),
                            self.blur(e, Art::Landscape),
                        );
                        line = line.push(cells::episode_card(
                            e,
                            i,
                            cw,
                            focused && i == self.focus.ep,
                            img,
                            blur,
                            self.can_write(e),
                            now,
                        ));
                    }
                    page = page.push(
                        container(line)
                            .padding([0.0, t::PAGE_PAD])
                            .height(geom::card_h(w)),
                    );
                }
                page
            }
            Load::Ready(v) => {
                for (i, e) in v.iter().enumerate() {
                    page = page.push(band(cells::list_row(
                        e,
                        i,
                        focused && i == self.focus.ep,
                        m.kind,
                        self.img(e, Art::Poster, 320),
                        self.blur(e, Art::Poster),
                    )));
                }
                page
            }
        }
    }

    // ---- strips ----

    fn strip<'a>(&'a self, s: Sec, items: &'a [Media], focus: Option<usize>) -> El<'a> {
        let (title, body) = s.title(self.media.kind);
        let (cw, ch) = geom::dims(s);
        let (art, px) = geom::strip_art(s);
        let mut cells_row = row![];
        for (i, m) in items.iter().enumerate() {
            cells_row = cells_row.push(cells::poster_cell(Cell {
                idx: i,
                w: cw,
                h: ch,
                img: self.img(m, art, px),
                blur: self.blur(m, art),
                title: &m.title,
                meta: caption(m),
                focused: focus == Some(i),
                progress: m.progress(),
                badge: None,
                on_press: Msg::Sec(s, i),
                on_enter: Msg::Hover(DRow::Strip(s), i),
            }));
        }
        // The first cell's picture lines up with the page margin (cells carry their own padding).
        let strip = scrollable(container(cells_row).padding(Padding {
            left: t::PAGE_PAD - t::S3,
            ..Padding::ZERO
        }))
        .id(strip_id(s))
        .direction(Direction::Horizontal(
            Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Length::Fill)
        .height(geom::strip_h(s));
        band(
            container(
                column![
                    section(title, body, items.len().to_string()),
                    space().height(0),
                    strip
                ]
                .spacing(t::S4),
            )
            .padding([t::S8, 0.0])
            .height(geom::strip_block(s) - 1.0),
        )
    }
}

/// Streams of every file, as the "Media info" table.
pub(super) fn info_table<'a>(f: &Full) -> El<'a> {
    let mut d = column![crate::widget::label("Media info")].spacing(t::S4);
    for b in &f.info {
        let head = [b.container.as_str(), b.size.as_str(), b.bitrate.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        let mut tbl = column![
            container(mono(head))
                .padding([0.0, t::S4])
                .height(36)
                .center_y(36)
        ];
        for s in &b.streams {
            tbl = tbl.push(rule()).push(stream_row(s));
        }
        d = d.push(container(tbl).style(|_| container::Style {
            border: border(t::HAIRLINE, 1.0, t::R_PANEL),
            ..Default::default()
        }));
    }
    d.into()
}

fn stream_row<'a>(s: &StreamRow) -> El<'a> {
    let col = |x: String| -> El<'a> {
        container(
            g(x).size(t::CAPTION + 1.0)
                .color(t::TEXT_SECONDARY)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::FillPortion(1))
        .clip(true)
        .into()
    };
    container(
        row![
            container(mono(s.kind.to_uppercase())).width(80),
            container(
                g(s.codec.clone())
                    .size(t::CAPTION + 1.0)
                    .color(t::TEXT_SECONDARY)
            )
            .width(72),
            col(s.title.clone()),
            col(s.detail.clone()),
            container(mono(s.bitrate.clone())).width(90),
        ]
        .spacing(t::S4)
        .align_y(Alignment::Center),
    )
    .padding([0.0, t::S4])
    .height(36)
    .center_y(36)
    .into()
}
