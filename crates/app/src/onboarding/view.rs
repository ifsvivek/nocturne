//! Rendering of the walkthrough: plates in trays (hairline shell, raised core), one accent, mono
//! micro-labels, generous spacing. State lives in `mod.rs`; nothing here decides anything.
use super::{Item, Msg, Onboarding, Step, count};
use crate::motion::ease_out;
use crate::style::{border, fill, glow_shadow};
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{action_button, icon, label, mono, pulse, rule, switch, wordmark};
use iced::widget::button::{Status, Style};
use iced::widget::{Container, column, container, mouse_area, row, space, stack, text};
use iced::{Alignment, Background, Color, Element, Length, Padding, Shadow, mouse};
use std::time::Instant;

const COPY_W: f32 = 520.0;

impl Onboarding {
    pub fn view(&self, now: Instant) -> Element<'_, Msg> {
        let e = ease_out(self.settle(now));
        let off = self.dir * (1.0 - e) * 28.0;
        let slid = container(self.body(now))
            .center(Length::Fill)
            .padding(Padding {
                left: off.max(0.0) * 2.0,
                right: (-off).max(0.0) * 2.0,
                ..Padding::ZERO
            });
        // No way to fade a subtree: lay a frame-coloured veil over it that thins out.
        let content: Element<Msg> = if e < 1.0 {
            stack![
                slid,
                container(space())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(fill(Color {
                        a: (1.0 - e) * 0.9,
                        ..t::frame()
                    })),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            slid.into()
        };
        column![content, self.footer()].into()
    }

    fn body(&self, now: Instant) -> Element<'_, Msg> {
        let inner: Element<Msg> = match self.step {
            Step::Welcome => self.welcome(),
            Step::Where => self.where_step(),
            Step::SetUp => self.setup(now),
            Step::Behave => self.behave(),
            Step::Ready => self.ready(),
            Step::News1 => self.news1(),
            Step::News2 => self.news2(),
        };
        if self.win.height < 680.0 {
            iced::widget::scrollable(
                container(inner)
                    .center_x(Length::Fill)
                    .padding([t::S6, 0.0]),
            )
            .style(crate::style::scroll)
            .height(Length::Fill)
            .into()
        } else {
            inner
        }
    }

    // ---- shared pieces ----

    /// Wordmark, step eyebrow, title and one calm paragraph.
    fn head<'a>(&self, title: &'a str, sub: &'a str) -> Element<'a, Msg> {
        column![
            self.mark(),
            label(&format!("Step {} of {}", self.index() + 1, self.total())),
            text(title).size(34).font(t::bold()),
            container(
                text(sub)
                    .size(t::BODY_LG)
                    .line_height(1.5)
                    .color(t::text_secondary())
                    .align_x(Alignment::Center)
            )
            .max_width(COPY_W),
        ]
        .spacing(t::S2)
        .align_x(Alignment::Center)
        .into()
    }

    /// Small wordmark above the eyebrow (dropped in short windows).
    fn mark<'a>(&self) -> Element<'a, Msg> {
        if self.compact() {
            space().into()
        } else {
            column![wordmark(15.0), space().height(t::S3)].into()
        }
    }

    fn page<'a>(
        &self,
        head: Element<'a, Msg>,
        body: Element<'a, Msg>,
        hint: &'a str,
    ) -> Element<'a, Msg> {
        column![
            head,
            space().height(if self.compact() { t::S3 } else { t::S6 }),
            body,
            space().height(t::S2),
            mono(hint)
        ]
        .spacing(t::S3)
        .align_x(Alignment::Center)
        .into()
    }

    /// Short windows (min size is 800x560) trade air for fit.
    fn compact(&self) -> bool {
        self.win.height < 760.0
    }

    fn card_w(&self) -> f32 {
        ((self.win.width - 2.0 * t::PAGE_PAD - 2.0 * t::S4) / 3.0).clamp(220.0, 300.0)
    }

    fn focused_at(&self, item: Item) -> bool {
        self.items().get(self.focus) == Some(&item)
    }

    fn at(&self, item: Item) -> usize {
        self.items().iter().position(|i| *i == item).unwrap_or(0)
    }

    fn next_cta<'a>(&self, text: &str) -> Element<'a, Msg> {
        let i = self.at(Item::Next);
        cta(text, true, self.focused_at(Item::Next), Msg::Press(i))
    }

    // ---- steps ----

    fn welcome(&self) -> Element<'_, Msg> {
        column![
            wordmark(44.0),
            label("Your media. Your night."),
            space().height(t::S6),
            container(
                text("A native media center for your local folders and Jellyfin servers, in one library.")
                    .size(t::BODY_LG + 2.0)
                    .line_height(1.55)
                    .color(t::text_secondary())
                    .align_x(Alignment::Center)
            )
            .max_width(460),
            space().height(t::S8),
            self.next_cta("Get started"),
            space().height(t::S2),
            mono("ENTER to begin      S to skip"),
        ]
        .spacing(t::S2)
        .align_x(Alignment::Center)
        .into()
    }

    fn where_step(&self) -> Element<'_, Msg> {
        const CHOICES: [(&str, &str, &str); 3] = [
            (
                "folder",
                "Local folders",
                "Movies, shows, music and photos already on this computer. No account needed.",
            ),
            (
                "server",
                "A Jellyfin server",
                "Sign in to a Jellyfin server you run, or one a friend shares with you.",
            ),
            (
                "layers",
                "Both",
                "Folders and servers side by side, shown together as one library.",
            ),
        ];
        let w = self.card_w();
        let mut cards = row![].spacing(t::S4);
        for (i, (ic, title, body)) in CHOICES.iter().enumerate() {
            let active = self.focus == i;
            let inner = column![
                row![
                    badge(ic, active),
                    space().width(Length::Fill),
                    mono(format!("0{}", i + 1))
                ]
                .align_y(Alignment::Start),
                space().height(t::S3),
                text(*title).size(20).font(t::semibold()),
                text(*body)
                    .size(t::BODY)
                    .line_height(1.45)
                    .color(t::text_secondary()),
                space().height(Length::Fill),
                row![
                    space().width(Length::Fill),
                    text("\u{2192}").size(t::BODY_LG).color(if active {
                        t::accent_text()
                    } else {
                        t::NONE
                    })
                ],
            ]
            .spacing(t::S2);
            cards = cards.push(
                mouse_area(plate(inner, w.into(), 232.0.into(), active))
                    .on_enter(Msg::Hover(i))
                    .on_press(Msg::Press(i))
                    .interaction(mouse::Interaction::Pointer),
            );
        }
        let head = column![
            self.mark(),
            label(&format!("Step {} of {}", self.index() + 1, self.total())),
            text("Where is your media?").size(34).font(t::bold()),
            container(
                text("Start with one; you can add the others at any time.")
                    .size(t::BODY_LG)
                    .color(t::text_secondary())
            )
            .max_width(COPY_W),
        ]
        .spacing(t::S2)
        .align_x(Alignment::Center);
        self.page(
            head.into(),
            cards.into(),
            "\u{2190} \u{2192} to choose      ENTER to continue      ESC to go back",
        )
    }

    fn setup(&self, now: Instant) -> Element<'_, Msg> {
        let kinds = self
            .items()
            .into_iter()
            .filter(|i| matches!(i, Item::AddFolders | Item::AddServer))
            .collect::<Vec<_>>();
        let (title, sub) = match (kinds.as_slice(), self.choice) {
            ([Item::AddFolders], _) => (
                "Add your folders",
                "Pick the folders that hold your media. NOCTURNE reads them where they are.",
            ),
            ([Item::AddServer], _) => (
                "Connect a server",
                "Enter your server\u{2019}s address, then sign in with your account.",
            ),
            _ => (
                "Set up your library",
                "Add a server and some folders. Do them in either order; more can come later.",
            ),
        };
        let w = if kinds.len() > 1 {
            ((self.win.width - 2.0 * t::PAGE_PAD - t::S4) / 2.0).clamp(300.0, 400.0)
        } else {
            (self.win.width - 2.0 * t::PAGE_PAD).clamp(300.0, 520.0)
        };
        let mut panels = row![].spacing(t::S4);
        for k in kinds {
            panels = panels.push(self.source_panel(k, w, now));
        }
        let go = if self.world.folders.is_empty() && self.world.servers.is_empty() {
            "Skip for now"
        } else {
            "Continue"
        };
        let body = column![panels, space().height(t::S2), self.next_cta(go)]
            .spacing(t::S4)
            .align_x(Alignment::Center);
        self.page(
            self.head(title, sub),
            body.into(),
            "ENTER to add      TAB to move      ESC to go back",
        )
    }

    fn source_panel(&self, kind: Item, w: f32, now: Instant) -> Element<'_, Msg> {
        let focused = self.focused_at(kind);
        let folders = kind == Item::AddFolders;
        let done = if folders {
            !self.world.folders.is_empty()
        } else {
            !self.world.servers.is_empty()
        };
        let mut lines = column![].spacing(t::S2);
        if folders {
            if self.world.folders.is_empty() {
                lines = lines.push(text("No folders yet.").color(t::text_tertiary()));
            }
            for f in self.world.folders.iter().take(4) {
                lines = lines.push(
                    row![
                        icon("folder", 15.0, t::text_secondary()),
                        text(f.clone()).size(t::BODY).wrapping(text::Wrapping::None)
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                );
            }
            if self.world.folders.len() > 4 {
                lines = lines.push(mono(format!("+ {} more", self.world.folders.len() - 4)));
            }
            if let Some(n) = self.world.indexing {
                let a = if crate::motion::reduced() {
                    1.0
                } else {
                    pulse(now, 1200)
                };
                lines = lines.push(
                    row![
                        text("\u{25CF}")
                            .size(t::CAPTION)
                            .color(Color { a, ..t::accent() }),
                        text(format!("Indexing\u{2026} {} files", count(n)))
                            .size(t::BODY)
                            .color(t::accent_text()),
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                );
                lines = lines.push(mono("Carries on in the background"));
            } else if self.world.files > 0 {
                lines = lines.push(
                    text(format!("{} files found", count(self.world.files)))
                        .size(t::BODY)
                        .color(t::text_secondary()),
                );
            }
        } else {
            if self.world.servers.is_empty() {
                lines = lines.push(text("Not connected yet.").color(t::text_tertiary()));
            }
            for s in self.world.servers.iter().take(4) {
                lines = lines.push(
                    row![
                        icon("check", 15.0, t::accent_text()),
                        text(format!("Connected to {} as {}", s.name, s.user))
                            .size(t::BODY)
                            .wrapping(text::Wrapping::None)
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                );
            }
            if self.world.servers.len() > 4 {
                lines = lines.push(mono(format!("+ {} more", self.world.servers.len() - 4)));
            }
        }
        let (ic, name, add) = if folders {
            (
                "folder",
                "Local folders",
                if done {
                    "Add another folder"
                } else {
                    "Add folders"
                },
            )
        } else {
            (
                "server",
                "Servers",
                if done {
                    "Connect another server"
                } else {
                    "Connect a server"
                },
            )
        };
        let i = self.at(kind);
        let inner = column![
            row![
                badge(ic, focused || done),
                text(name).size(18).font(t::semibold())
            ]
            .spacing(t::S3)
            .align_y(Alignment::Center),
            container(lines).height(Length::Fill).padding([t::S2, 0.0]),
            action_button(add, !done, false, focused, false, Msg::Press(i)),
        ]
        .spacing(t::S3);
        plate(inner, w.into(), 270.0.into(), focused).into()
    }

    fn behave(&self) -> Element<'_, Msg> {
        let w = self.card_w();
        let play = if self.world.embedded_video {
            "Video plays inside this window, with hardware decoding where your graphics card allows it."
        } else {
            "On this desktop, video opens in its own mpv window, with hardware decoding where available."
        };
        let cards = [
            (
                "lock",
                "Read-only by default",
                "NOCTURNE never changes anything on your server until you allow it.",
            ),
            (
                "files",
                "Your files stay untouched",
                "NOCTURNE never moves, renames or deletes anything in your folders.",
            ),
            ("play", "Playback", play),
        ];
        let mut r = row![].spacing(t::S4);
        for (ic, title, body) in cards {
            let inner = column![
                badge(ic, false),
                space().height(t::S3),
                text(title).size(18).font(t::semibold()),
                text(body)
                    .size(t::BODY)
                    .line_height(1.45)
                    .color(t::text_secondary()),
            ]
            .spacing(t::S2);
            r = r.push(plate(
                inner,
                w.into(),
                (if self.compact() { 216.0 } else { 250.0 }).into(),
                false,
            ));
        }
        let mut body = column![r].spacing(t::S4).align_x(Alignment::Center);
        if self.server_in_play() {
            body = body.push(self.switch_panel(w * 3.0 + 2.0 * t::S4));
        }
        body = body.push(self.next_cta("Continue"));
        self.page(
            self.head("How NOCTURNE behaves", "Three promises, in plain words."),
            body.into(),
            "SPACE to switch      ENTER to continue      ESC to go back",
        )
    }

    fn switch_panel(&self, w: f32) -> Element<'_, Msg> {
        let writes = self.world.writes;
        let line = |title: &'static str, hint: &'static str, dim: bool, on: bool, item: Item| {
            let i = self.at(item);
            row![
                column![
                    text(title).size(t::BODY).font(t::semibold()).color(if dim {
                        t::text_tertiary()
                    } else {
                        t::text()
                    }),
                    text(hint)
                        .size(t::CAPTION + 1.0)
                        .line_height(1.4)
                        .color(t::text_secondary()),
                ]
                .spacing(2)
                .width(Length::Fill),
                if dim {
                    // Inert while server changes are off: the state stays visible, a press does nothing.
                    row![mono("NEEDS THE SWITCH ABOVE")].into()
                } else {
                    switch(on, self.focused_at(item), Msg::Press(i))
                },
            ]
            .spacing(t::S6)
            .align_y(Alignment::Center)
        };
        let inner = column![
            line(
                "Allow changes on the server",
                "Off by default. Needed for watched marks, favourites, progress sync and admin changes. Change it any time in Settings.",
                false,
                writes,
                Item::Writes,
            ),
            rule(),
            line(
                "Sync watch progress",
                "Continue Watching follows you between devices.",
                !writes,
                self.world.sync && writes,
                Item::Sync,
            ),
        ]
        .spacing(t::S3);
        container(inner)
            .padding([t::S4, t::S6])
            .width(w)
            .style(crate::style::panel)
            .into()
    }

    fn ready(&self) -> Element<'_, Msg> {
        let tips: [(Vec<&str>, &str, &str); 4] = [
            (
                vec!["/", "Ctrl K"],
                "Search everything",
                "One search across every folder and server.",
            ),
            (
                vec!["?"],
                "All the shortcuts",
                "A cheat sheet is one key away, on any screen.",
            ),
            (
                vec!["Settings", "Sources"],
                "Add more any time",
                "Connect another server or add a folder whenever you like.",
            ),
            (
                vec!["Versions and sources"],
                "Duplicates stay tidy",
                "The same title in two places shows once. Choose the copy on its page.",
            ),
        ];
        let w = ((self.win.width - 2.0 * t::PAGE_PAD - t::S4) / 2.0).clamp(280.0, 360.0);
        let mut grid = column![].spacing(t::S4);
        for pair in tips.chunks(2) {
            let mut r = row![].spacing(t::S4);
            for (keys, title, body) in pair {
                let mut ks = row![].spacing(t::S2).align_y(Alignment::Center);
                for (n, k) in keys.iter().enumerate() {
                    if n > 0 && keys.len() == 2 && keys[0] == "Settings" {
                        ks = ks.push(text("\u{203A}").color(t::text_tertiary()));
                    } else if n > 0 {
                        ks = ks.push(mono("or"));
                    }
                    ks = ks.push(keycap(k));
                }
                let inner = column![
                    ks,
                    space().height(t::S3),
                    text(*title).size(t::BODY_LG).font(t::semibold()),
                    text(*body)
                        .size(t::BODY)
                        .line_height(1.45)
                        .color(t::text_secondary()),
                ]
                .spacing(t::S2);
                r = r.push(plate(inner, w.into(), 196.0.into(), false));
            }
            grid = grid.push(r);
        }
        let body = column![grid, space().height(t::S2), self.next_cta("Start watching")]
            .spacing(t::S4)
            .align_x(Alignment::Center);
        self.page(
            self.head("You\u{2019}re all set", "A few things worth knowing."),
            body.into(),
            "ENTER to start",
        )
    }

    fn news_head<'a>(&self, title: &'a str, sub: &'a str) -> Element<'a, Msg> {
        column![
            self.mark(),
            label("What\u{2019}s new in 0.3"),
            text(title).size(34).font(t::bold()),
            container(
                text(sub)
                    .size(t::BODY_LG)
                    .line_height(1.5)
                    .color(t::text_secondary())
                    .align_x(Alignment::Center)
            )
            .max_width(COPY_W),
        ]
        .spacing(t::S2)
        .align_x(Alignment::Center)
        .into()
    }

    fn news1(&self) -> Element<'_, Msg> {
        let w = ((self.win.width - 2.0 * t::PAGE_PAD - t::S4) / 2.0).clamp(280.0, 360.0);
        let items = [
            (
                "01",
                "A new look",
                "Rebuilt from the ground up: quicker to open, calmer to browse, the same dark, quiet feel.",
            ),
            (
                "02",
                "Local folders and several servers",
                "Add folders from this computer and as many servers as you like. They all share one library.",
            ),
        ];
        let mut r = row![].spacing(t::S4);
        for (n, title, body) in items {
            let inner = column![
                mono(n),
                space().height(t::S3),
                text(title).size(20).font(t::semibold()),
                text(body)
                    .size(t::BODY)
                    .line_height(1.45)
                    .color(t::text_secondary()),
            ]
            .spacing(t::S2);
            r = r.push(plate(inner, w.into(), 250.0.into(), false));
        }
        let body = column![r, space().height(t::S2), self.next_cta("Next")]
            .spacing(t::S4)
            .align_x(Alignment::Center);
        self.page(
            self.news_head(
                "Welcome to 0.3",
                "Your server and your account carry over as they were.",
            ),
            body.into(),
            "ENTER to continue      S to skip",
        )
    }

    fn news2(&self) -> Element<'_, Msg> {
        let w =
            ((self.win.width - 2.0 * t::PAGE_PAD - t::S4) / 2.0).clamp(280.0, 360.0) * 2.0 + t::S4;
        let inner = row![
            badge("layers", false),
            column![
                text("One entry, every copy").size(20).font(t::semibold()),
                text("When the same film or episode lives in more than one place, it shows up once. Open it to see every copy and choose which one plays.")
                    .size(t::BODY)
                    .line_height(1.45)
                    .color(t::text_secondary()),
            ]
            .spacing(t::S2)
        ]
        .spacing(t::S4);
        let (fi, ni) = (self.at(Item::NewsFolders), self.at(Item::Next));
        let buttons = row![
            cta(
                "Add local folders",
                false,
                self.focused_at(Item::NewsFolders),
                Msg::Press(fi)
            ),
            cta(
                "Continue",
                true,
                self.focused_at(Item::Next),
                Msg::Press(ni)
            ),
        ]
        .spacing(t::S3);
        let body = column![
            plate(inner, w.into(), 150.0.into(), false),
            space().height(t::S2),
            buttons
        ]
        .spacing(t::S4)
        .align_x(Alignment::Center);
        self.page(
            self.news_head("Versions and sources", "One title, every copy."),
            body.into(),
            "\u{2190} \u{2192} to choose      ENTER to continue      ESC to go back",
        )
    }

    // ---- footer ----

    fn footer(&self) -> Element<'_, Msg> {
        let back: Element<Msg> = if self.can_back() {
            quiet("\u{2190}  Back", Msg::Back)
        } else {
            space().width(120).into()
        };
        let skip: Element<Msg> = if self.can_skip() {
            quiet("Skip setup", Msg::Skip)
        } else {
            space().width(120).into()
        };
        let mut dots = row![].spacing(t::S2).align_y(Alignment::Center);
        for i in 0..self.total() {
            let (w, c) = match i.cmp(&self.index()) {
                std::cmp::Ordering::Equal => (22.0, t::accent()),
                std::cmp::Ordering::Less => (6.0, t::text_tertiary()),
                std::cmp::Ordering::Greater => (6.0, t::hairline_strong()),
            };
            dots =
                dots.push(
                    container(space().width(w).height(6)).style(move |_| container::Style {
                        background: Some(c.into()),
                        border: border(t::NONE, 0.0, 3.0),
                        ..Default::default()
                    }),
                );
        }
        container(
            row![
                container(back).width(120).align_x(Alignment::Start),
                space().width(Length::Fill),
                dots,
                space().width(Length::Fill),
                container(skip).width(120).align_x(Alignment::End),
            ]
            .align_y(Alignment::Center),
        )
        .padding([t::S6, t::PAGE_PAD])
        .into()
    }
}

// ---- parts ----

fn badge<'a, M: 'a>(name: &str, active: bool) -> Container<'a, M> {
    container(icon(
        name,
        22.0,
        if active {
            t::accent_text()
        } else {
            t::text_secondary()
        },
    ))
    .center(44)
    .style(move |_| container::Style {
        background: Some(if active { t::accent_tint() } else { t::GLASS }.into()),
        border: border(
            if active {
                t::accent()
            } else {
                t::hairline_strong()
            },
            1.0,
            22.0,
        ),
        ..Default::default()
    })
}

/// A plate in a tray: hairline shell, raised core with its own hairline.
fn plate<'a, M: 'a>(
    inner: impl Into<Element<'a, M>>,
    w: Length,
    h: Length,
    active: bool,
) -> Container<'a, M> {
    let core = container(inner)
        .padding(t::S6)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(if active {
                t::raised()
            } else {
                t::surface()
            })),
            border: border(
                if active {
                    t::accent_tint()
                } else {
                    t::hairline()
                },
                1.0,
                t::r_panel(),
            ),
            ..Default::default()
        });
    container(core)
        .padding(4)
        .width(w)
        .height(h)
        .style(move |_| container::Style {
            background: Some(t::GLASS.into()),
            border: border(
                if active {
                    t::accent()
                } else {
                    t::hairline_strong()
                },
                1.0,
                t::r_panel() + 4.0,
            ),
            shadow: if active {
                glow_shadow(0.5)
            } else {
                Shadow::default()
            },
            ..Default::default()
        })
}

fn keycap<'a, M: 'a>(s: &str) -> Element<'a, M> {
    container(
        text(s.to_string())
            .font(t::MONO)
            .size(t::CAPTION)
            .color(t::text()),
    )
    .padding([3.0, t::S2])
    .style(|_| container::Style {
        background: Some(t::control().into()),
        border: border(t::edge(), 1.0, t::r_xs() + 1.0),
        ..Default::default()
    })
    .into()
}

/// The big pill with its arrow in a nested circle.
fn cta<'a>(label: &str, primary: bool, focused: bool, msg: Msg) -> Element<'a, Msg> {
    let ink = if primary { t::on_accent() } else { t::text() };
    let ring = container(icon("arrow_right", 14.0, ink))
        .center(28)
        .style(move |_| container::Style {
            background: Some(Color { a: 0.14, ..ink }.into()),
            border: border(t::NONE, 0.0, 14.0),
            ..Default::default()
        });
    let r = row![
        text(label.to_string())
            .shaping(iced::widget::text::Shaping::Advanced)
            .size(t::BODY)
            .font(t::semibold()),
        ring
    ]
    .spacing(t::S4)
    .align_y(Alignment::Center);
    Press::new(container(r).center_y(Length::Fill))
        .width(Length::Shrink)
        .height(48)
        .padding(Padding {
            left: t::S6,
            right: 10.0,
            ..Padding::ZERO
        })
        .on_press(msg)
        .style(move |s| {
            let hot = matches!(s, Status::Hovered | Status::Pressed);
            let (bg, edge) = if primary {
                (
                    Some(if focused || hot {
                        t::accent_hover()
                    } else {
                        t::accent()
                    }),
                    t::accent(),
                )
            } else {
                (
                    if hot {
                        Some(t::GLASS)
                    } else if focused {
                        Some(t::accent_tint())
                    } else {
                        None
                    },
                    if focused {
                        t::accent()
                    } else if hot {
                        t::text_tertiary()
                    } else {
                        t::edge()
                    },
                )
            };
            Style {
                background: bg.map(Background::Color),
                text_color: ink,
                border: border(edge, 1.0, 24.0),
                shadow: if focused {
                    glow_shadow(0.7)
                } else {
                    Shadow::default()
                },
                snap: false,
            }
        })
        .into()
}

/// Back / Skip: no outline, brightens on hover.
fn quiet<'a>(s: &str, msg: Msg) -> Element<'a, Msg> {
    Press::new(
        container(text(s.to_string()).size(t::CAPTION + 1.0).font(t::medium()))
            .center_y(Length::Fill),
    )
    .width(Length::Shrink)
    .height(32)
    .padding([0.0, t::S2])
    .on_press(msg)
    .style(|s| Style {
        background: None,
        text_color: if matches!(s, Status::Hovered | Status::Pressed) {
            t::text()
        } else {
            t::text_tertiary()
        },
        border: border(t::NONE, 0.0, 6.0),
        shadow: Shadow::default(),
        snap: false,
    })
    .into()
}
