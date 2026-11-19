//! The Theme Studio's docked panel: the selected element, the scope switch and the grouped controls (every one reads the cascade and
//! writes a rule through `studio.rs`), the raw CSS tab, the starters gallery and saving.
use super::App;
use super::message::Message;
use super::studio::{Key, Msg, PANEL_W, Scope, Tab, fmt, hex_of, starters};
use crate::style::{self, border};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{action_button, mono, tracked, value_chip};
use iced::widget::slider::{self, Handle, HandleShape, Rail};
use iced::widget::{
    Row, column, container, row, scrollable as scroll, slider as slider_fn, space, text,
    text_editor, text_input,
};
use iced::{Alignment, Background, Color, Element, Length, Padding};
use nocturne_style::inspect::Step;

type El<'a> = Element<'a, Message>;

fn m(x: Msg) -> Message {
    Message::Studio(x)
}

fn rgb(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], f32::from(c[3]) / 255.0)
}

fn caption<'a>(s: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(s).size(t::CAPTION).color(t::text_secondary())
}

fn slide_style(_: &iced::Theme, _: slider::Status) -> slider::Style {
    slider::Style {
        rail: Rail {
            backgrounds: (Background::Color(t::accent()), Background::Color(t::edge())),
            width: 4.0,
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
        },
        handle: Handle {
            shape: HandleShape::Circle { radius: 7.0 },
            background: Background::Color(t::text()),
            border_width: 0.0,
            border_color: t::NONE,
        },
    }
}

fn field<'a>(value: &str, on: impl Fn(String) -> Msg + 'a, w: f32) -> El<'a> {
    text_input("", value)
        .on_input(move |s| m(on(s)))
        .size(t::CONTROL_TEXT)
        .padding([t::S1, t::S2])
        .width(w)
        .style(style::input)
        .into()
}

fn group<'a>(title: &str, rows: Vec<El<'a>>) -> El<'a> {
    column![
        text(tracked(title))
            .shaping(text::Shaping::Advanced)
            .size(t::MICRO)
            .font(t::MONO)
            .color(t::text_tertiary()),
        column(rows).spacing(t::S2),
    ]
    .spacing(t::S2)
    .into()
}

fn chips<'a>(items: Vec<El<'a>>) -> El<'a> {
    Row::with_children(items)
        .spacing(t::S1)
        .wrap()
        .vertical_spacing(t::S1)
        .into()
}

impl App {
    /// The whole docked panel.
    pub(super) fn studio_panel(&self) -> El<'_> {
        let st = &self.studio;
        let top = row![
            text("Theme Studio").size(t::BODY).font(t::semibold()),
            space().width(Length::Fill),
            action_button("Done", true, false, false, false, m(Msg::Close)),
        ]
        .spacing(t::S2)
        .align_y(Alignment::Center);
        let tools = row![
            value_chip(
                if st.pick { "Picking" } else { "Pick" },
                false,
                st.pick,
                false,
                m(Msg::Pick(!st.pick))
            ),
            action_button(
                "Undo",
                false,
                false,
                false,
                !self.studio_can_undo(),
                m(Msg::Undo)
            ),
            action_button(
                "Redo",
                false,
                false,
                false,
                !self.studio_can_redo(),
                m(Msg::Redo)
            ),
        ]
        .spacing(t::S2)
        .align_y(Alignment::Center);
        let tab =
            |label: &str, tb: Tab| value_chip(label, false, st.tab == tb, false, m(Msg::Tab(tb)));
        let tabs = row![
            tab("Edit", Tab::Edit),
            tab("CSS", Tab::Css),
            tab("Starters", Tab::Starters),
            tab("Save", Tab::Save),
        ]
        .spacing(t::S1);
        let body: El = match st.tab {
            Tab::Edit => self.studio_edit_tab(),
            Tab::Css => self.studio_css_tab(),
            Tab::Starters => self.studio_starters_tab(),
            Tab::Save => self.studio_save_tab(),
        };
        container(column![top, tools, tabs, body].spacing(t::S3))
            .padding(t::S4)
            .width(PANEL_W)
            .height(Length::Fill)
            .style(style::panel)
            .into()
    }

    fn studio_can_undo(&self) -> bool {
        self.studio.can_undo()
    }

    fn studio_can_redo(&self) -> bool {
        self.studio.can_redo()
    }

    fn studio_css_tab(&self) -> El<'_> {
        let st = &self.studio;
        let editor = text_editor(&st.content)
            .on_action(|a| m(Msg::Edit(a)))
            .font(t::MONO)
            .size(t::CONTROL_TEXT)
            .padding(t::S3)
            .height(Length::Fill)
            .style(|_, s| {
                let ring = match s {
                    text_editor::Status::Focused { .. } => t::accent(),
                    text_editor::Status::Hovered => t::text_secondary(),
                    _ => t::edge(),
                };
                text_editor::Style {
                    background: Background::Color(t::NONE),
                    border: border(ring, 1.0, 6.0),
                    placeholder: t::text_tertiary(),
                    value: t::text(),
                    selection: t::accent_tint(),
                }
            });
        let mut col = column![
            caption("The theme's CSS. Typing here updates the controls; the controls write here."),
            editor
        ]
        .spacing(t::S2);
        for w in st.warnings.iter().take(6) {
            col = col.push(mono(w.clone()));
        }
        col.height(Length::Fill).into()
    }

    fn studio_starters_tab(&self) -> El<'_> {
        let blurb = |n: &str| match n {
            "Spotlight" => {
                "Full-bleed backdrop under a floating rounded header, light primary and glass secondary pill buttons, accent bars before section titles."
            }
            "Glass" => {
                "Black canvas, a glass header pill, a rotating full-bleed hero with the title as logo art, light and glass pill buttons, accent rails and squircle cards."
            }
            "Marquee" => "A large billboard hero and wide rows: the cinematic look.",
            "Feed" => "One column of big cards, like a feed.",
            "Classic" => "A compact grid with the plain NOCTURNE chrome.",
            _ => "",
        };
        let mut col = column![caption(
            "Start from a look, then change any element with the controls. Your current work stays in Undo."
        )]
        .spacing(t::S3);
        for (i, (name, _)) in starters().into_iter().enumerate() {
            col = col.push(
                container(
                    row![
                        wire(name),
                        column![
                            text(name).size(t::BODY).font(t::semibold()),
                            caption(blurb(name)),
                            action_button(
                                "Start from this",
                                false,
                                false,
                                false,
                                false,
                                m(Msg::Starter(i))
                            ),
                        ]
                        .spacing(t::S1)
                        .width(Length::Fill),
                    ]
                    .spacing(t::S3),
                )
                .padding(t::S3)
                .style(style::raised_card),
            );
        }
        scroll(col).style(style::scroll).height(Length::Fill).into()
    }

    fn studio_save_tab(&self) -> El<'_> {
        let st = &self.studio;
        let here = self.theme_source().is_some();
        let mut col = column![
            caption("Studio themes are stored like any theme and can be applied to everything or to one server."),
            field(&st.name, Msg::Name, 340.0),
            row![
                action_button("Save", false, false, false, false, m(Msg::Save)),
                action_button("Apply to the whole app", true, false, false, false, m(Msg::Apply)),
            ]
            .spacing(t::S2),
            row![
                action_button("Use for this server", false, false, false, !here, m(Msg::UseHere)),
                action_button("Export .css", false, false, false, false, m(Msg::Export)),
            ]
            .spacing(t::S2),
        ]
        .spacing(t::S3);
        if !here {
            col = col.push(mono(
                "Pick one server in the source filter to use the theme for it only.",
            ));
        }
        if let Some(n) = &st.note {
            col = col.push(caption(n.clone()));
        }
        col.push(mono("Ctrl+Shift+0 always brings the built-in look back."))
            .into()
    }

    fn studio_edit_tab(&self) -> El<'_> {
        let st = &self.studio;
        let Some(sel) = &st.sel else {
            return column![
                caption(if st.pick {
                    "Hover an element of the app and click it, or press Enter. The arrow keys move to the parent, the first child and the siblings. Esc stops picking so you can scroll and open other screens."
                } else {
                    "Press Pick, then click an element of the app. Navigate to Home, a library, Search or any screen first."
                }),
                container(
                    action_button("Pick an element", true, false, false, st.pick, m(Msg::Pick(true)))
                )
                .padding(Padding { top: t::S3, ..Padding::ZERO }),
            ]
            .spacing(t::S2)
            .into();
        };
        let info = &sel.insp.info;
        let c = &sel.insp.computed;
        let mut name = info.tag.clone();
        if let Some(i) = &info.dom_id {
            name.push('#');
            name.push_str(i);
        }
        for k in &info.classes {
            name.push('.');
            name.push_str(k);
        }
        let path = info
            .path
            .iter()
            .rev()
            .take(4)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join(" > ");
        let nav = row![
            action_button(
                "Parent",
                false,
                false,
                false,
                false,
                m(Msg::Move(Step::Parent))
            ),
            action_button(
                "Child",
                false,
                false,
                false,
                false,
                m(Msg::Move(Step::Child))
            ),
            action_button("Prev", false, false, false, false, m(Msg::Move(Step::Prev))),
            action_button("Next", false, false, false, false, m(Msg::Move(Step::Next))),
        ]
        .spacing(t::S1);
        let mut head = column![
            text(name)
                .size(t::BODY)
                .font(t::semibold())
                .color(t::accent_text()),
            mono(path),
        ]
        .spacing(2);
        for (k, v) in &info.attrs {
            head = head.push(mono(if v.is_empty() {
                k.clone()
            } else {
                format!("{k}=\"{v}\"")
            }));
        }
        if let Some(tx) = &info.text {
            head = head.push(mono(format!(
                "text: {}",
                tx.chars().take(40).collect::<String>()
            )));
        }
        head = head.push(mono(format!(
            "{} x {} at {:.0}, {:.0}",
            info.rect[2].round(),
            info.rect[3].round(),
            info.rect[0],
            info.rect[1]
        )));

        // Scope.
        let like = sel.likes.get(sel.like_ix);
        let like_label = match like {
            Some(l) => format!("All like this ({})", l.count),
            None => "All like this".to_owned(),
        };
        let mut scope = column![
            row![
                value_chip(
                    "This element only",
                    false,
                    st.scope == Scope::This,
                    false,
                    m(Msg::Scope(Scope::This))
                ),
                value_chip(
                    &like_label,
                    false,
                    st.scope == Scope::Like,
                    false,
                    m(Msg::Scope(Scope::Like))
                ),
            ]
            .spacing(t::S1),
        ]
        .spacing(t::S1);
        if st.scope == Scope::Like && sel.likes.len() > 1 {
            scope = scope.push(chips(
                sel.likes
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        value_chip(
                            &format!("{} ({})", l.selector, l.count),
                            false,
                            i == sel.like_ix,
                            false,
                            m(Msg::LikeIx(i)),
                        )
                    })
                    .collect(),
            ));
        }
        let writing = match (st.scope, like) {
            (Scope::Like, Some(l)) => l.selector.clone(),
            _ => sel.this.clone(),
        };
        scope = scope.push(mono(format!("writes: {writing}")));

        let num = |label: &'static str, key: Key, v: f32| self.num_row(label, key, v);
        let col = |label: &'static str, key: Key, v: Option<[u8; 4]>| self.color_row(label, key, v);
        let pick = |label: &'static str, key: Key, opts: &[&'static str], cur: &str| {
            choice_row(label, key, opts, cur)
        };

        let colours = group(
            "Colours",
            vec![
                col("Text", Key::Color, c.color),
                col("Background", Key::Background, c.background),
                col("Border", Key::BorderColor, c.border_color),
            ],
        );
        let size = group(
            "Size and spacing",
            vec![
                num("Width", Key::Width, c.width),
                num("Height", Key::Height, c.height),
                num("Padding up/down", Key::PadY, c.padding.0),
                num("Padding left/right", Key::PadX, c.padding.1),
                num("Margin up/down", Key::MarY, c.margin.0),
                num("Margin left/right", Key::MarX, c.margin.1),
                num("Gap", Key::Gap, c.gap),
            ],
        );
        let shape = group(
            "Shape",
            vec![
                num("Corner radius", Key::Radius, c.radius),
                num("Border width", Key::BorderW, c.border_width),
            ],
        );
        let sh = c.shadow.unwrap_or((0.0, 0.0, 0.0, 0.0, [0, 0, 0, 0]));
        let effects = group(
            "Effects",
            vec![
                num("Shadow x", Key::ShX, sh.0),
                num("Shadow y", Key::ShY, sh.1),
                num("Shadow blur", Key::ShBlur, sh.2),
                num("Shadow spread", Key::ShSpread, sh.3),
                col("Shadow colour", Key::ShColor, c.shadow.map(|s| s.4)),
                chips(vec![value_chip(
                    "No shadow",
                    false,
                    false,
                    false,
                    m(Msg::Choice(Key::ShX, "none")),
                )]),
                num("Opacity", Key::Opacity, c.opacity),
            ],
        );
        let bar = &st.hover.bar;
        let decoration = group(
            "Accent bar",
            vec![
                num("Bar width", Key::BarW, bar.w),
                num("Gap to the content", Key::BarGap, bar.gap),
                col("Bar colour", Key::BarColor, token_rgba(&bar.color)),
            ],
        );
        let typo = group(
            "Typography",
            vec![
                self.family_row(c.family.as_deref()),
                num("Size", Key::FontSize, c.font_size),
                num("Weight", Key::Weight, f32::from(c.weight)),
                num("Letter spacing", Key::Spacing, c.letter_spacing),
                num("Line height", Key::LineH, c.line_height),
                pick(
                    "Case",
                    Key::Transform,
                    &["none", "uppercase", "lowercase"],
                    c.transform,
                ),
                pick("Align", Key::Align, &["left", "center", "right"], c.align),
            ],
        );
        let mut layout_rows = vec![
            pick(
                "Display",
                Key::Display,
                &["block", "flex", "grid"],
                &c.display,
            ),
            pick(
                "Direction",
                Key::Direction,
                &["row", "column"],
                &c.direction,
            ),
            pick("Wrap", Key::Wrap, &["nowrap", "wrap"], &c.wrap),
            pick(
                "Align items",
                Key::AlignItems,
                &["flex-start", "center", "flex-end", "stretch"],
                &c.align_items,
            ),
            pick(
                "Justify",
                Key::Justify,
                &[
                    "flex-start",
                    "center",
                    "flex-end",
                    "space-between",
                    "space-around",
                ],
                &c.justify,
            ),
            num("Order", Key::Order, c.order as f32),
            pick(
                "Position",
                Key::Position,
                &["static", "relative", "absolute", "fixed"],
                &c.position,
            ),
            pick(
                "Visibility",
                Key::Visibility,
                &["visible", "hidden"],
                if c.visible { "visible" } else { "hidden" },
            ),
        ];
        layout_rows.push(if info.essential {
            caption("Hide is off: the app keeps this element (back, search, the player's controls or something inside it) visible whatever a theme says.").into()
        } else {
            chips(vec![value_chip("Hide this", false, c.hidden, false, m(Msg::Hide(!c.hidden)))])
        });
        let layout = group("Layout", layout_rows);
        let h = &st.hover;
        let hover = group(
            "Hover",
            vec![
                num("Scale", Key::HoverScale, h.scale.unwrap_or(1.0)),
                col("Colour on hover", Key::HoverBg, h.bg),
                num("Transition", Key::Transition, h.ms.unwrap_or(0.0)),
            ],
        );

        let mut tail = column![
            row![
                action_button(
                    "Reset this scope",
                    false,
                    true,
                    false,
                    false,
                    m(Msg::ResetElement)
                ),
                action_button(
                    "Force with !important",
                    false,
                    false,
                    false,
                    !self.studio_overridden(),
                    m(Msg::Force)
                ),
            ]
            .spacing(t::S2),
        ]
        .spacing(t::S2);
        if self.studio_overridden() {
            tail = tail.push(caption(
                "Another rule is stronger than your last change. Force makes yours win.",
            ));
        }
        tail = tail.push(group("Matched rules", self.rules_rows()));

        let body = column![
            head, nav, scope, colours, size, shape, effects, decoration, typo, layout, hover, tail,
        ]
        .spacing(t::S4)
        .padding(Padding {
            right: t::S3,
            ..Padding::ZERO
        });
        scroll(body)
            .style(style::scroll)
            .height(Length::Fill)
            .into()
    }

    /// Does a rule override the declaration the Studio wrote last?
    fn studio_overridden(&self) -> bool {
        let (Some(sel), Some((rule, prop))) = (&self.studio.sel, &self.studio.last_edit) else {
            return false;
        };
        let mine = sel
            .insp
            .rules
            .iter()
            .rev()
            .find(|r| &r.selector == rule && r.decls.iter().any(|d| d.name == *prop));
        mine.is_some_and(|r| r.decls.iter().any(|d| d.name == *prop && !d.won))
    }

    fn rules_rows(&self) -> Vec<El<'_>> {
        let Some(sel) = &self.studio.sel else {
            return vec![];
        };
        let mut v: Vec<El> = vec![];
        for r in sel.insp.rules.iter().rev() {
            let names = r
                .decls
                .iter()
                .map(|d| {
                    format!(
                        "{}{}{}",
                        d.name,
                        if d.important { "!" } else { "" },
                        if d.won { "" } else { " (overridden)" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            v.push(
                column![
                    text(format!(
                        "{}  [{}]  ({},{},{})",
                        r.selector, r.source, r.spec.0, r.spec.1, r.spec.2
                    ))
                    .font(t::MONO)
                    .size(t::MICRO)
                    .color(t::text_secondary()),
                    mono(names),
                ]
                .spacing(2)
                .into(),
            );
        }
        v
    }

    fn num_row(&self, label: &'static str, key: Key, v: f32) -> El<'_> {
        let (lo, hi, step, unit) = key.range();
        let shown = match &self.studio.field {
            Some((k, t)) if *k == key => t.clone(),
            _ => fmt(v),
        };
        row![
            caption(label).width(120),
            slider_fn(lo..=hi, v.clamp(lo, hi), move |x| m(Msg::Num(key, x)))
                .step(step)
                .style(slide_style)
                .width(Length::Fill),
            field(&shown, move |s| Msg::NumText(key, s), 58.0),
            mono(unit).width(18),
        ]
        .spacing(t::S2)
        .align_y(Alignment::Center)
        .into()
    }

    fn color_row(&self, label: &'static str, key: Key, v: Option<[u8; 4]>) -> El<'_> {
        let shown = match &self.studio.field {
            Some((k, t)) if *k == key => t.clone(),
            _ => v.map_or_else(|| "none".to_owned(), hex_of),
        };
        let c = v.unwrap_or([0, 0, 0, 0]);
        let swatch =
            Press::new(
                container(space())
                    .width(26)
                    .height(20)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(rgb(c))),
                        border: border(t::hairline_strong(), 1.0, 4.0),
                        ..Default::default()
                    }),
            )
            .on_press(m(Msg::Expand(key)))
            .style(|_| iced::widget::button::Style::default());
        let mut col = column![
            row![
                caption(label).width(120),
                swatch,
                field(&shown, move |s| Msg::Hex(key, s), 110.0),
                action_button("Clear", false, false, false, false, m(Msg::Clear(key))),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        ]
        .spacing(t::S1);
        if self.studio.expanded == Some(key) {
            let chan = |n: &'static str, i: usize| -> El<'_> {
                row![
                    mono(n).width(14),
                    slider_fn(0.0..=255.0, f32::from(c[i]), move |x| {
                        let mut n = c;
                        n[i] = x.round() as u8;
                        m(Msg::Color(key, n))
                    })
                    .style(slide_style)
                    .width(Length::Fill),
                    mono(format!("{}", c[i])).width(26),
                ]
                .spacing(t::S2)
                .align_y(Alignment::Center)
                .into()
            };
            col = col
                .push(chan("R", 0))
                .push(chan("G", 1))
                .push(chan("B", 2))
                .push(chan("A", 3));
            col = col.push(chips(
                [
                    "accent",
                    "text",
                    "text-secondary",
                    "surface",
                    "raised",
                    "frame",
                    "edge",
                ]
                .into_iter()
                .map(|n| value_chip(n, false, false, false, m(Msg::Token(key, n))))
                .collect(),
            ));
        }
        col.into()
    }

    fn family_row(&self, current: Option<&str>) -> El<'_> {
        let st = &self.studio;
        let f = st.family_filter.to_lowercase();
        let matches: Vec<El> = st
            .fonts
            .iter()
            .filter(|n| f.is_empty() || n.to_lowercase().contains(&f))
            .take(8)
            .map(|n| {
                value_chip(
                    n,
                    false,
                    current.is_some_and(|c| c.eq_ignore_ascii_case(n)),
                    false,
                    m(Msg::Family(n.clone())),
                )
            })
            .collect();
        column![
            row![
                caption("Font").width(120),
                field(&st.family_filter, Msg::FamilyFilter, 220.0),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
            mono(format!("now: {}", current.unwrap_or("the app's font"))),
            chips(matches),
        ]
        .spacing(t::S1)
        .into()
    }
}

/// A colour value as the swatch shows it: hex, or one of the theme's own tokens resolved to what it is now.
fn token_rgba(v: &str) -> Option<[u8; 4]> {
    if let Some(c) = super::studio::parse_color(v) {
        return Some(c);
    }
    let name = v
        .trim()
        .strip_prefix("var(--nocturne-")?
        .strip_suffix(')')?;
    let c = match name {
        "accent" => t::accent(),
        "text" => t::text(),
        "text-secondary" => t::text_secondary(),
        "surface" => t::surface(),
        "raised" => t::raised(),
        "frame" => t::frame(),
        "edge" => t::edge(),
        _ => return None,
    };
    Some([c.r, c.g, c.b, c.a].map(|x| (x * 255.0).round() as u8))
}

fn choice_row<'a>(label: &'static str, key: Key, opts: &[&'static str], cur: &str) -> El<'a> {
    row![
        caption(label).width(120),
        chips(
            opts.iter()
                .map(|o| value_chip(o, false, *o == cur, false, m(Msg::Choice(key, o))))
                .collect()
        ),
    ]
    .spacing(t::S2)
    .into()
}

/// A tiny wireframe of a starter (a static illustration, not a render).
fn wire<'a>(name: &str) -> El<'a> {
    let block = |w: f32, h: f32, c: Color, r: f32| -> El<'a> {
        container(space())
            .width(w)
            .height(h)
            .style(move |_| container::Style {
                background: Some(Background::Color(c)),
                border: border(Color::TRANSPARENT, 0.0, r),
                ..Default::default()
            })
            .into()
    };
    let (a, b, c) = (t::accent(), t::raised(), t::edge());
    let inner: El<'a> = match name {
        "Spotlight" => column![
            block(60.0, 6.0, c, 3.0),
            block(104.0, 34.0, b, 2.0),
            row![block(22.0, 8.0, t::text(), 4.0), block(22.0, 8.0, c, 4.0)].spacing(4),
            row![block(3.0, 8.0, a, 1.0), block(40.0, 8.0, c, 2.0)].spacing(3),
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .into(),
        "Glass" => column![
            block(104.0, 5.0, c, 2.0),
            block(104.0, 30.0, b, 2.0),
            row![block(22.0, 8.0, t::text(), 4.0), block(22.0, 8.0, c, 4.0)].spacing(4),
            row![
                block(24.0, 22.0, c, 5.0),
                block(24.0, 22.0, c, 5.0),
                block(24.0, 22.0, c, 5.0)
            ]
            .spacing(3),
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .into(),
        "Marquee" => column![
            block(104.0, 30.0, b, 2.0),
            row![
                block(24.0, 22.0, c, 2.0),
                block(24.0, 22.0, c, 2.0),
                block(24.0, 22.0, c, 2.0),
                block(24.0, 22.0, c, 2.0)
            ]
            .spacing(3),
        ]
        .spacing(5)
        .into(),
        "Feed" => column![
            block(104.0, 18.0, b, 2.0),
            block(104.0, 18.0, b, 2.0),
            block(104.0, 18.0, b, 2.0),
        ]
        .spacing(4)
        .into(),
        _ => column![
            row![
                block(24.0, 22.0, c, 2.0),
                block(24.0, 22.0, c, 2.0),
                block(24.0, 22.0, c, 2.0),
                block(24.0, 22.0, c, 2.0)
            ]
            .spacing(3),
            row![
                block(24.0, 22.0, b, 2.0),
                block(24.0, 22.0, b, 2.0),
                block(24.0, 22.0, b, 2.0),
                block(24.0, 22.0, b, 2.0)
            ]
            .spacing(3),
        ]
        .spacing(4)
        .into(),
    };
    container(inner)
        .padding(t::S2)
        .width(124)
        .height(80)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(|_| container::Style {
            background: Some(Background::Color(t::frame())),
            border: border(t::hairline_strong(), 1.0, 6.0),
            ..Default::default()
        })
        .into()
}
