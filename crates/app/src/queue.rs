//! The session's music queue (port of GPUI `AudioQueue` in `ui_playlists.rs`) and its overlay.
//! The queue lives in the root for as long as the app runs and never goes to a server.
//!
//! ## Root hook
//!
//! ```ignore
//! queue: queue::AudioQueue                  // in App; Default = empty
//! // Detail `Outcome::Queue{tracks,next}`:
//! match self.queue.insert(tracks, next) { Ok(m) | Err(m) => toast(m) }
//! // Player goes away (close, sign-out, switch server): keep music for the session
//! if let Some(q) = player.music_snapshot() { self.queue = q; }
//! // Account menu "Play music queue (n)" / "Clear music queue" (n = queue.items.len()):
//! AcctAct::PlayQueue  => open the overlay:  self.queue_open = true
//! AcctAct::ClearQueue => toast(self.queue.clear())
//! // overlay: view -> `self.queue.view(win).map(Message::Queue)` above the page, keys -> `queue.key(&k)`
//! //   Message::Queue(m) => self.queue_outcome(self.queue.update(m))
//! // Outcome::Play: start `queue.items[queue.index]` with rest = items[index+1..], then once the
//! //   player exists: `player.restore_music(&self.queue)`;  Outcome::Close: queue_open = false
//! ```
//! The in-player queue panel (reorder, remove, shuffle, repeat while playing) is `player/`; this
//! overlay is the same set of operations for when nothing is playing.
use crate::keys::Key;
use crate::player::Player;
use crate::theme as t;
use crate::widget::overlay;
use crate::widget::press::Press;
use crate::widget::{action_button, mono};
use iced::widget::button::{Status, Style};
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Shadow, Size};
use nocturne_core::player_logic as pl;
use nocturne_core::source::{Kind, Media};
use nocturne_core::video::Repeat;

#[derive(Clone, Default)]
pub struct AudioQueue {
    pub items: Vec<Media>,
    pub index: usize,
    pub repeat: Option<Repeat>,
    pub shuffle: bool,
    /// The order before shuffling, so turning shuffle off restores it.
    unshuffled: Vec<Media>,
    /// Selected row of the overlay.
    pub sel: usize,
}

/// What a keypress does to the selected entry of an owned playlist (also used by the overlay).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Remove,
    Up,
    Down,
}

impl Edit {
    pub fn of(key: &str) -> Option<Self> {
        match key {
            "x" | "delete" => Some(Self::Remove),
            "[" => Some(Self::Up),
            "]" => Some(Self::Down),
            _ => None,
        }
    }

    /// The server call for entry `idx` of `len`: Some(Remove) or Some(Move to n), None at an edge.
    pub fn plan(self, idx: usize, len: usize) -> Option<Option<usize>> {
        if idx >= len {
            return None;
        }
        match self {
            Self::Remove => Some(None),
            Self::Up => idx.checked_sub(1).map(Some),
            Self::Down => (idx + 1 < len).then_some(Some(idx + 1)),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Sel(usize),
    Move(usize, bool),
    Remove(usize),
    Shuffle,
    Repeat,
    Play,
    Clear,
    Close,
}

pub enum Outcome {
    /// Start `items[index]` (rest after it) and then `Player::restore_music`.
    Play,
    Toast(&'static str),
    Close,
}

fn rng() -> impl FnMut(usize) -> usize {
    let mut s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
        | 1;
    move |n| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s % n as u64) as usize
    }
}

impl AudioQueue {
    /// Tracks after the playing one, or at the end. Nothing is sent to the server.
    pub fn insert(&mut self, tracks: Vec<Media>, next: bool) -> Result<&'static str, &'static str> {
        let tracks: Vec<Media> = tracks
            .into_iter()
            .filter(|m| m.kind == Kind::Audio)
            .collect();
        if tracks.is_empty() {
            return Err("No music to queue here.");
        }
        pl::queue_insert(&mut self.items, self.index, tracks, next);
        Ok(if next {
            "Playing next."
        } else {
            "Added to the queue."
        })
    }

    pub fn clear(&mut self) -> &'static str {
        *self = Self::default();
        "Queue cleared."
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn shift(&mut self, i: usize, up: bool) {
        if let Some(j) = pl::queue_move(&mut self.items, &mut self.index, i, up) {
            self.sel = j;
        }
    }

    fn drop_at(&mut self, i: usize) -> Option<&'static str> {
        if !pl::queue_remove(&mut self.items, &mut self.index, i) {
            return Some("That one plays first.");
        }
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        None
    }

    fn toggle_shuffle(&mut self) {
        let playing = self.items.get(self.index).map(|m| m.id.clone());
        if self.shuffle {
            let (q, at) = pl::unshuffle(
                &self.items,
                &self.unshuffled,
                |m| m.id.as_str(),
                playing.as_deref().unwrap_or(""),
            );
            (self.items, self.index) = (q, at);
            self.unshuffled.clear();
        } else {
            self.unshuffled = self.items.clone();
            pl::shuffle_after(&mut self.items, self.index, &mut rng());
        }
        self.shuffle = !self.shuffle;
    }

    fn cycle_repeat(&mut self) {
        self.repeat = Some(match self.repeat.unwrap_or(Repeat::Off) {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        });
    }

    pub fn update(&mut self, msg: Msg) -> Option<Outcome> {
        match msg {
            Msg::Sel(i) => self.sel = i.min(self.items.len().saturating_sub(1)),
            Msg::Move(i, up) => self.shift(i, up),
            Msg::Remove(i) => return self.drop_at(i).map(Outcome::Toast),
            Msg::Shuffle => self.toggle_shuffle(),
            Msg::Repeat => self.cycle_repeat(),
            Msg::Play if self.items.is_empty() => {
                return Some(Outcome::Toast("The queue is empty."));
            }
            Msg::Play => return Some(Outcome::Play),
            Msg::Clear => {
                let m = self.clear();
                return Some(Outcome::Toast(m));
            }
            Msg::Close => return Some(Outcome::Close),
        }
        None
    }

    /// Keys while the overlay is up (it owns the keyboard).
    pub fn key(&mut self, k: &Key) -> Option<Outcome> {
        let n = self.items.len();
        let sel = self.sel;
        let msg = match k.name.as_str() {
            "escape" | "q" => Msg::Close,
            "up" => Msg::Sel(sel.saturating_sub(1)),
            "down" => Msg::Sel((sel + 1).min(n.saturating_sub(1))),
            "enter" => Msg::Play,
            "s" => Msg::Shuffle,
            "r" => Msg::Repeat,
            "c" => Msg::Clear,
            other => match Edit::of(other)? {
                Edit::Remove => Msg::Remove(sel),
                Edit::Up => Msg::Move(sel, true),
                Edit::Down => Msg::Move(sel, false),
            },
        };
        self.update(msg)
    }

    pub fn view(&self, win: Size) -> Element<'_, Msg> {
        let mut body: Vec<Element<Msg>> = vec![];
        if self.items.is_empty() {
            body.push(
                text("The queue is empty. Use \"Play next\" or \"Add to queue\" on a track or album.")
                    .size(t::BODY)
                    .color(t::text_secondary())
                    .into(),
            );
        }
        for (i, m) in self.items.iter().enumerate() {
            let on = i == self.sel;
            let now = i == self.index;
            let label = row![
                mono(format!("{:>2}", i + 1)).width(24),
                text(m.title.clone())
                    .size(t::BODY)
                    .font(if now { t::semibold() } else { t::medium() })
                    .width(Length::Fill)
                    .wrapping(text::Wrapping::None),
                mono(if now { "NEXT UP" } else { "" }),
            ]
            .spacing(t::S3)
            .align_y(Alignment::Center);
            body.push(
                Press::new(label)
                    .width(Length::Fill)
                    .padding([t::S2, t::S3])
                    .on_press(Msg::Sel(i))
                    .style(move |s| Style {
                        background: if on {
                            Some(t::accent_tint().into())
                        } else if matches!(s, Status::Hovered) {
                            Some(t::GLASS.into())
                        } else {
                            None
                        },
                        text_color: if on { t::accent_text() } else { t::text() },
                        border: crate::style::border(t::NONE, 0.0, 6.0),
                        shadow: Shadow::default(),
                        snap: false,
                    })
                    .into(),
            );
        }
        let none = self.items.is_empty();
        let bar = row![
            action_button("Play", true, false, false, none, Msg::Play),
            action_button(
                if self.shuffle {
                    "Shuffle: on"
                } else {
                    "Shuffle: off"
                },
                false,
                false,
                false,
                false,
                Msg::Shuffle
            ),
            action_button(
                &format!("Repeat: {}", self.repeat.unwrap_or(Repeat::Off).label()),
                false,
                false,
                false,
                false,
                Msg::Repeat
            ),
            action_button(
                "Move up",
                false,
                false,
                false,
                none,
                Msg::Move(self.sel, true)
            ),
            action_button(
                "Move down",
                false,
                false,
                false,
                none,
                Msg::Move(self.sel, false)
            ),
            action_button("Remove", false, true, false, none, Msg::Remove(self.sel)),
            action_button("Clear", false, true, false, none, Msg::Clear),
        ]
        .spacing(t::S2)
        .wrap()
        .vertical_spacing(t::S2);
        body.push(container(bar).padding([t::S2, 0.0]).into());
        body.push(
            mono("Esc closes  \u{00B7}  Up / Down select  \u{00B7}  [ ] move  \u{00B7}  X removes  \u{00B7}  S shuffle  \u{00B7}  R repeat  \u{00B7}  Enter plays")
                .into(),
        );
        overlay::panel("Music queue", 640.0, win, body, Msg::Close)
    }
}

impl Player {
    /// The queue worth keeping for the session: only music (a video queue is rebuilt from its
    /// show).
    pub fn music_snapshot(&self) -> Option<AudioQueue> {
        (self.media.kind == Kind::Audio && self.queue.len() > 1).then(|| AudioQueue {
            items: self.queue.clone(),
            index: self.qi,
            repeat: Some(self.set.repeat),
            shuffle: self.shuffle,
            unshuffled: self.unshuffled.clone(),
            sel: self.qi,
        })
    }

    /// Put a kept queue (order, position, repeat, shuffle) back into a freshly opened player.
    pub fn restore_music(&mut self, q: &AudioQueue) {
        self.queue = q.items.clone();
        self.qi = q.index;
        if let Some(r) = q.repeat {
            self.set.repeat = r;
        }
        if q.shuffle {
            self.shuffle = true;
            self.unshuffled = q.unshuffled.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Media {
        Media::from_item(&nocturne_jellyfin::Item {
            id: id.into(),
            name: id.into(),
            kind: "Audio".into(),
            ..Default::default()
        })
    }

    fn q(n: usize) -> AudioQueue {
        let mut a = AudioQueue::default();
        a.insert((0..n).map(|i| track(&format!("t{i}"))).collect(), false)
            .unwrap();
        a
    }

    fn ids(a: &AudioQueue) -> Vec<&str> {
        a.items.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn playlist_key_edits_follow_the_list_edges() {
        assert_eq!(Edit::of("x"), Some(Edit::Remove));
        assert_eq!(Edit::of("]"), Some(Edit::Down));
        assert_eq!(Edit::of("q"), None);
        assert_eq!(Edit::Remove.plan(0, 1), Some(None));
        assert_eq!(Edit::Up.plan(0, 3), None, "first entry cannot go up");
        assert_eq!(Edit::Up.plan(2, 3), Some(Some(1)));
        assert_eq!(Edit::Down.plan(2, 3), None, "last entry cannot go down");
        assert_eq!(Edit::Down.plan(0, 3), Some(Some(1)));
        assert_eq!(Edit::Remove.plan(5, 3), None, "stale selection");
    }

    #[test]
    fn only_music_is_queued_and_play_next_goes_after_the_current() {
        let mut a = q(3);
        a.index = 1;
        let movie = Media::from_item(&nocturne_jellyfin::Item {
            id: "m".into(),
            kind: "Movie".into(),
            ..Default::default()
        });
        assert!(a.insert(vec![movie.clone()], true).is_err());
        assert_eq!(a.insert(vec![track("n"), movie], true), Ok("Playing next."));
        assert_eq!(ids(&a), ["t0", "t1", "n", "t2"]);
        assert_eq!(a.insert(vec![track("e")], false), Ok("Added to the queue."));
        assert_eq!(ids(&a).last(), Some(&"e"));
    }

    #[test]
    fn reorder_and_remove_keep_the_playing_track() {
        let mut a = q(4);
        a.index = 1;
        a.update(Msg::Move(2, true)); // t2 above the playing t1
        assert_eq!(ids(&a), ["t0", "t2", "t1", "t3"]);
        assert_eq!(a.index, 2, "index follows the playing track");
        assert!(matches!(a.update(Msg::Remove(2)), Some(Outcome::Toast(_))));
        assert_eq!(a.items.len(), 4, "the playing track cannot be removed");
        a.update(Msg::Remove(0));
        assert_eq!((ids(&a), a.index), (vec!["t2", "t1", "t3"], 1));
    }

    #[test]
    fn shuffle_keeps_the_head_and_off_restores_the_order() {
        let mut a = q(12);
        a.index = 3;
        a.update(Msg::Shuffle);
        assert!(a.shuffle);
        assert_eq!(
            &ids(&a)[..4],
            ["t0", "t1", "t2", "t3"],
            "up to the current is untouched"
        );
        a.update(Msg::Shuffle);
        let want: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        assert_eq!(ids(&a), want.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn repeat_cycles_and_keys_drive_the_overlay() {
        let mut a = q(3);
        a.update(Msg::Repeat);
        assert_eq!(a.repeat, Some(Repeat::All));
        a.update(Msg::Repeat);
        a.update(Msg::Repeat);
        assert_eq!(a.repeat, Some(Repeat::Off));
        let k = |n: &str| Key {
            name: n.into(),
            ..Default::default()
        };
        a.key(&k("down"));
        assert_eq!(a.sel, 1);
        a.key(&k("]"));
        assert_eq!((a.sel, a.items[2].id.as_str()), (2, "t1"));
        assert!(matches!(a.key(&k("enter")), Some(Outcome::Play)));
        assert!(matches!(a.key(&k("escape")), Some(Outcome::Close)));
        assert!(matches!(
            a.key(&k("c")),
            Some(Outcome::Toast("Queue cleared."))
        ));
        assert!(a.is_empty());
        assert!(matches!(a.update(Msg::Play), Some(Outcome::Toast(_))));
    }
}
