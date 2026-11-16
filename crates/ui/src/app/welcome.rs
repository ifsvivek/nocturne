//! First run: no source is set up yet. Three large choices over a dim copy of the startup art.
//! This module is visual state and messages only; the root decides what each choice does.
use crate::style::{border, glow_shadow};
use crate::theme as t;
use crate::widget::{icon, label, mono, pill_button, wordmark};
use iced::widget::{column, container, mouse_area, row, space, text};
use iced::{Alignment, Background, Element, Length, Shadow};

/// What the person wants NOCTURNE to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstRun {
    Local,
    Server,
    Both,
}

#[derive(Clone, Copy, Debug)]
pub enum Msg {
    Select(usize),
    Pick(FirstRun),
    /// Leave the "coming soon" note.
    Back,
}

pub struct Choice {
    pub mode: FirstRun,
    pub icon: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

pub const CHOICES: [Choice; 3] = [
    Choice {
        mode: FirstRun::Local,
        icon: "folder",
        title: "Local library",
        body: "Your folders. No server needed.",
    },
    Choice {
        mode: FirstRun::Server,
        icon: "server",
        title: "Connect to a server",
        body: "Jellyfin servers you have an account on.",
    },
    Choice {
        mode: FirstRun::Both,
        icon: "layers",
        title: "Both",
        body: "Folders and servers together in one library.",
    },
];

pub struct Welcome {
    pub sel: usize,
    /// The Local choice was taken before the local folders screen exists.
    pub local_note: bool,
}

impl Welcome {
    pub fn new() -> Self {
        // Most people have a server; start on it.
        Self {
            sel: 1,
            local_note: false,
        }
    }

    /// Arrow keys move, Enter picks; Esc leaves the note. `Some` when a choice was made.
    pub fn key(&mut self, key: &str) -> Option<FirstRun> {
        let n = CHOICES.len();
        if self.local_note {
            if matches!(key, "escape" | "backspace") {
                self.local_note = false;
            }
            return None;
        }
        match key {
            "left" | "up" => self.sel = (self.sel + n - 1) % n,
            "right" | "down" | "tab" => self.sel = (self.sel + 1) % n,
            "home" => self.sel = 0,
            "end" => self.sel = n - 1,
            "enter" | "space" => return Some(CHOICES[self.sel].mode),
            _ => {}
        }
        None
    }

    pub fn update(&mut self, msg: Msg) -> Option<FirstRun> {
        match msg {
            Msg::Select(i) => self.sel = i.min(CHOICES.len() - 1),
            Msg::Pick(m) => return Some(m),
            Msg::Back => self.local_note = false,
        }
        None
    }
}

fn card<'a>(i: usize, c: &Choice, active: bool) -> Element<'a, Msg> {
    let badge = container(icon(
        c.icon,
        22.0,
        if active {
            t::ACCENT_TEXT
        } else {
            t::TEXT_SECONDARY
        },
    ))
    .center(44)
    .style(move |_| container::Style {
        background: Some(if active { t::ACCENT_TINT } else { t::GLASS }.into()),
        border: border(
            if active {
                t::ACCENT
            } else {
                t::HAIRLINE_STRONG
            },
            1.0,
            22.0,
        ),
        ..Default::default()
    });
    let arrow =
        text("\u{2192}")
            .size(t::BODY_LG)
            .color(if active { t::ACCENT_TEXT } else { t::NONE });
    let inner = container(
        column![
            row![
                badge,
                space().width(Length::Fill),
                mono(format!("0{}", i + 1))
            ]
            .align_y(Alignment::Start),
            space().height(Length::Fill),
            text(c.title).size(20).font(t::SEMIBOLD),
            text(c.body)
                .size(t::BODY)
                .line_height(1.45)
                .color(t::TEXT_SECONDARY),
            row![space().width(Length::Fill), arrow],
        ]
        .spacing(t::S2),
    )
    .padding(t::S6)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(if active {
            t::RAISED
        } else {
            t::SURFACE
        })),
        border: border(
            if active { t::ACCENT_TINT } else { t::HAIRLINE },
            1.0,
            t::R_PANEL,
        ),
        ..Default::default()
    });
    // Outer shell with its own hairline: the card reads as a plate in a tray.
    let shell = container(inner)
        .padding(4)
        .width(Length::Fixed(276.0))
        .height(Length::Fixed(212.0))
        .style(move |_| container::Style {
            background: Some(t::GLASS.into()),
            border: border(
                if active {
                    t::ACCENT
                } else {
                    t::HAIRLINE_STRONG
                },
                1.0,
                t::R_PANEL + 4.0,
            ),
            shadow: if active {
                glow_shadow(0.55)
            } else {
                Shadow::default()
            },
            ..Default::default()
        });
    mouse_area(shell)
        .on_enter(Msg::Select(i))
        .on_press(Msg::Pick(c.mode))
        .into()
}

pub fn view<'a>(w: &Welcome) -> Element<'a, Msg> {
    let body: Element<Msg> = if w.local_note {
        column![
            text("Local libraries are on their way")
                .size(t::HEADING)
                .font(t::BOLD),
            container(
                text("Adding folders arrives with the next update. You can connect a Jellyfin server now and add your folders when they are ready.")
                    .size(t::BODY)
                    .line_height(1.5)
                    .color(t::TEXT_SECONDARY)
                    .align_x(Alignment::Center),
            )
            .max_width(460),
            space().height(t::S2),
            row![
                container(pill_button("Connect a server", true, true, Msg::Pick(FirstRun::Server)))
                    .width(200),
                container(pill_button("Back", false, false, Msg::Back)).width(120),
            ]
            .spacing(t::S3),
        ]
        .spacing(t::S3)
        .align_x(Alignment::Center)
        .into()
    } else {
        let mut cards = row![].spacing(t::S4);
        for (i, c) in CHOICES.iter().enumerate() {
            cards = cards.push(card(i, c, i == w.sel));
        }
        column![
            text("Where does your media live?")
                .size(34)
                .font(t::BOLD),
            container(
                text("NOCTURNE plays your own files and your Jellyfin servers. Choose where to start; you can add the rest later.")
                    .size(t::BODY_LG)
                    .line_height(1.5)
                    .color(t::TEXT_SECONDARY)
                    .align_x(Alignment::Center),
            )
            .max_width(520),
            space().height(t::S6),
            cards,
            space().height(t::S4),
            mono("\u{2190} \u{2192} to choose      ENTER to continue"),
        ]
        .spacing(t::S3)
        .align_x(Alignment::Center)
        .into()
    };

    let front = column![
        wordmark(22.0),
        label("Welcome"),
        space().height(t::S6),
        body,
    ]
    .spacing(t::S2)
    .align_x(Alignment::Center);
    container(front).center(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_wrap_and_enter_picks() {
        let mut w = Welcome::new();
        assert_eq!(w.sel, 1);
        assert_eq!(w.key("right"), None);
        assert_eq!(w.sel, 2);
        w.key("right");
        assert_eq!(w.sel, 0, "wraps");
        w.key("left");
        assert_eq!(w.sel, 2);
        assert_eq!(w.key("enter"), Some(FirstRun::Both));
        w.key("home");
        assert_eq!(w.key("enter"), Some(FirstRun::Local));
    }

    #[test]
    fn the_note_swallows_keys_until_escape() {
        let mut w = Welcome::new();
        w.local_note = true;
        assert_eq!(w.key("enter"), None);
        assert!(w.local_note);
        w.key("escape");
        assert!(!w.local_note);
    }

    #[test]
    fn mouse_selects_and_picks() {
        let mut w = Welcome::new();
        assert_eq!(w.update(Msg::Select(0)), None);
        assert_eq!(w.sel, 0);
        assert_eq!(w.update(Msg::Select(99)), None);
        assert_eq!(w.sel, 2);
        assert_eq!(
            w.update(Msg::Pick(FirstRun::Server)),
            Some(FirstRun::Server)
        );
    }
}
