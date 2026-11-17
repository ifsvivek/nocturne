//! Gamepad input: a background thread reads `gilrs`, a subscription turns its events into
//! [`Ev`], and [`key`] maps a button into the keyboard vocabulary of `keys.rs`, so every screen
//! that works with a keyboard works with a pad (one code path). The thread only runs while Couch
//! mode is not Off.
use crate::keys::Key;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    Start,
    Select,
    LB,
    RB,
    LT,
    RT,
}

impl Btn {
    fn direction(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Left | Self::Right)
    }

    /// Short label for the on-screen hints.
    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "\u{2191}",
            Self::Down => "\u{2193}",
            Self::Left => "\u{2190}",
            Self::Right => "\u{2192}",
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Start => "Start",
            Self::Select => "Select",
            Self::LB => "LB",
            Self::RB => "RB",
            Self::LT => "LT",
            Self::RT => "RT",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    /// Gamepads connected now (drives Automatic couch mode).
    Pads(usize),
    Press(Btn),
}

fn named(name: &str) -> Key {
    Key {
        name: name.into(),
        text: (name.chars().count() == 1).then(|| name.into()),
        ..Default::default()
    }
}

/// The key a button stands for. `enter` says whether A should press Enter (menus, prompts, skip
/// chips) or play / pause (the bare player page).
pub fn key(b: Btn, in_player: bool, enter: bool) -> Option<Key> {
    let n = match (b, in_player) {
        (Btn::Up, _) => "up",
        (Btn::Down, _) => "down",
        (Btn::Left, _) => "left",
        (Btn::Right, _) => "right",
        (Btn::A, true) if !enter => "space",
        (Btn::A, _) => "enter",
        (Btn::B, _) => "escape",
        (Btn::X, true) => "t",
        (Btn::Y, true) => "v",
        (Btn::X, false) => "f",
        (Btn::Y, false) => "/",
        // Player: the settings menu. Elsewhere: the account menu.
        (Btn::Start, true) => "menu",
        (Btn::Start, false) => "a",
        (Btn::Select, true) => "i",
        (Btn::Select, false) => "?",
        (Btn::LB, true) => "p",
        (Btn::RB, true) => "n",
        (Btn::LB, false) => "pageup",
        (Btn::RB, false) => "pagedown",
        (Btn::LT, true) | (Btn::RT, true) => {
            return Some(Key {
                shift: true,
                ..named(if b == Btn::LT { "left" } else { "right" })
            });
        }
        _ => return None,
    };
    Some(named(n))
}

/// Held directions repeat: after `FIRST`, then every `EVERY`.
const FIRST: Duration = Duration::from_millis(380);
const EVERY: Duration = Duration::from_millis(85);
const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.35;

/// Press / release tracking for the direction that is held (D-pad or stick) and its repeat.
#[derive(Default)]
pub struct Repeater {
    held: Option<(Btn, Instant)>,
    stick: (f32, f32),
}

impl Repeater {
    pub fn press(&mut self, b: Btn, now: Instant) {
        if b.direction() {
            self.held = Some((b, now + FIRST));
        }
    }

    pub fn release(&mut self, b: Btn) {
        if self.held.is_some_and(|(h, _)| h == b) {
            self.held = None;
        }
    }

    /// A stick axis moved (`x`: right is positive; `y`: up is positive). Returns the direction
    /// that just became held, if any; hysteresis keeps a wobbling stick from chattering.
    pub fn axis(&mut self, x: Option<f32>, y: Option<f32>, now: Instant) -> Option<Btn> {
        self.stick = (x.unwrap_or(self.stick.0), y.unwrap_or(self.stick.1));
        let (sx, sy) = self.stick;
        let cur = self.held.map(|(b, _)| b);
        let level = |on: bool| if on { STICK_OFF } else { STICK_ON };
        let want = if sx.abs() >= sy.abs() {
            let l = level(matches!(cur, Some(Btn::Left | Btn::Right)));
            (sx.abs() >= l).then_some(if sx > 0.0 { Btn::Right } else { Btn::Left })
        } else {
            let l = level(matches!(cur, Some(Btn::Up | Btn::Down)));
            (sy.abs() >= l).then_some(if sy > 0.0 { Btn::Up } else { Btn::Down })
        };
        match (want, cur) {
            (Some(w), Some(c)) if w == c => None,
            (Some(w), _) => {
                self.press(w, now);
                Some(w)
            }
            (None, Some(_)) => {
                self.held = None;
                None
            }
            (None, None) => None,
        }
    }

    /// The held direction again, when its repeat is due.
    pub fn due(&mut self, now: Instant) -> Option<Btn> {
        let (b, at) = self.held?;
        (now >= at).then(|| {
            self.held = Some((b, now + EVERY));
            b
        })
    }

    pub fn holding(&self) -> bool {
        self.held.is_some()
    }
}

/// The subscription's stream: one thread owns `gilrs`; it ends when the stream is dropped.
pub fn events() -> impl iced::futures::Stream<Item = Ev> {
    iced::stream::channel(64, async |mut out| {
        let tx = out.clone();
        let _ = std::thread::Builder::new()
            .name("gamepad".into())
            .spawn(move || run(tx));
        // The thread does the work; this future only keeps the stream open.
        iced::futures::future::pending::<()>().await;
        let _ = out.try_send(Ev::Pads(0));
    })
}

fn run(mut tx: iced::futures::channel::mpsc::Sender<Ev>) {
    use gilrs::{Axis, Button, EventType, Gilrs};
    let Ok(mut g) = Gilrs::new() else { return };
    let pads = |g: &Gilrs| g.gamepads().filter(|(_, p)| p.is_connected()).count();
    if tx
        .try_send(Ev::Pads(pads(&g)))
        .is_err_and(|e| e.is_disconnected())
    {
        return;
    }
    let mut rep = Repeater::default();
    loop {
        // Block while idle (waking twice a second to notice a closed stream); tick while a
        // direction is held so its repeat is on time.
        let wait = if rep.holding() {
            Duration::from_millis(25)
        } else {
            Duration::from_millis(500)
        };
        let now = Instant::now();
        let mut out: Vec<Ev> = vec![];
        if let Some(e) = g.next_event_blocking(Some(wait)) {
            let btn = |b: Button| {
                Some(match b {
                    Button::DPadUp => Btn::Up,
                    Button::DPadDown => Btn::Down,
                    Button::DPadLeft => Btn::Left,
                    Button::DPadRight => Btn::Right,
                    Button::South => Btn::A,
                    Button::East => Btn::B,
                    Button::West => Btn::X,
                    Button::North => Btn::Y,
                    Button::Start => Btn::Start,
                    Button::Select => Btn::Select,
                    Button::LeftTrigger => Btn::LB,
                    Button::RightTrigger => Btn::RB,
                    Button::LeftTrigger2 => Btn::LT,
                    Button::RightTrigger2 => Btn::RT,
                    _ => return None,
                })
            };
            match e.event {
                EventType::ButtonPressed(b, _) => {
                    if let Some(b) = btn(b) {
                        rep.press(b, now);
                        out.push(Ev::Press(b));
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(b) = btn(b) {
                        rep.release(b);
                    }
                }
                EventType::AxisChanged(a, v, _) => {
                    let d = match a {
                        Axis::LeftStickX => rep.axis(Some(v), None, now),
                        Axis::LeftStickY => rep.axis(None, Some(v), now),
                        _ => None,
                    };
                    out.extend(d.map(Ev::Press));
                }
                EventType::Connected | EventType::Disconnected => out.push(Ev::Pads(pads(&g))),
                _ => {}
            }
        }
        out.extend(rep.due(Instant::now()).map(Ev::Press));
        for ev in out {
            if tx.try_send(ev).is_err_and(|e| e.is_disconnected()) {
                return;
            }
        }
        if tx.is_closed() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: Btn, p: bool, e: bool) -> (String, bool) {
        let k = key(b, p, e).unwrap();
        (k.name, k.shift)
    }

    #[test]
    fn buttons_map_into_the_keyboard_vocabulary() {
        assert_eq!(k(Btn::Down, false, false).0, "down");
        assert_eq!(k(Btn::A, false, false).0, "enter");
        assert_eq!(k(Btn::B, false, false).0, "escape");
        assert_eq!(k(Btn::Y, false, false).0, "/");
        assert_eq!(k(Btn::X, false, false).0, "f");
        assert_eq!(k(Btn::Start, false, false).0, "a");
        assert_eq!(k(Btn::LB, false, false).0, "pageup");
        assert_eq!(k(Btn::RB, false, false).0, "pagedown");
        assert!(key(Btn::LT, false, false).is_none());
        assert_eq!(
            key(Btn::Y, false, false).unwrap().text.as_deref(),
            Some("/")
        );
    }

    #[test]
    fn player_buttons_play_seek_and_open_menus() {
        assert_eq!(k(Btn::A, true, false).0, "space");
        assert_eq!(
            k(Btn::A, true, true).0,
            "enter",
            "a menu or prompt wants Enter"
        );
        assert_eq!(k(Btn::Start, true, false).0, "menu");
        assert_eq!(k(Btn::X, true, false).0, "t");
        assert_eq!(k(Btn::Y, true, false).0, "v");
        assert_eq!(k(Btn::LT, true, false), ("left".into(), true));
        assert_eq!(k(Btn::RT, true, false), ("right".into(), true));
        assert_eq!(k(Btn::RB, true, false).0, "n");
    }

    #[test]
    fn a_held_direction_repeats_after_a_delay() {
        let t0 = Instant::now();
        let mut r = Repeater::default();
        r.press(Btn::Right, t0);
        assert!(r.due(t0 + Duration::from_millis(100)).is_none());
        assert_eq!(r.due(t0 + FIRST), Some(Btn::Right));
        assert!(r.due(t0 + FIRST + Duration::from_millis(10)).is_none());
        assert_eq!(r.due(t0 + FIRST + EVERY), Some(Btn::Right));
        r.release(Btn::Right);
        assert!(r.due(t0 + Duration::from_secs(5)).is_none() && !r.holding());
        // Buttons other than directions never repeat.
        r.press(Btn::A, t0);
        assert!(!r.holding());
    }

    #[test]
    fn the_stick_acts_as_a_dpad_with_hysteresis() {
        let t0 = Instant::now();
        let mut r = Repeater::default();
        assert_eq!(r.axis(Some(0.4), None, t0), None, "inside the dead zone");
        assert_eq!(r.axis(Some(0.8), None, t0), Some(Btn::Right));
        assert_eq!(
            r.axis(Some(0.5), None, t0),
            None,
            "still held, no repeat press"
        );
        assert!(r.holding());
        assert_eq!(r.axis(Some(0.1), None, t0), None);
        assert!(!r.holding(), "released below the lower threshold");
        assert_eq!(r.axis(None, Some(-0.9), t0), Some(Btn::Down));
        assert_eq!(
            r.axis(Some(0.95), Some(-0.9), t0),
            Some(Btn::Right),
            "dominant axis wins"
        );
    }
}
