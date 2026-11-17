//! Keyboard events in the vocabulary of the core state machines (`"enter"`, `"escape"`, `"a"`,
//! `"/"`, ...): the unshifted key name plus the typed text, as the GPUI app saw them.
use iced::keyboard::{self, Key as K, key::Named};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Key {
    pub name: String,
    /// Text the press typed (`None` for control keys).
    pub text: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Key {
    pub fn plain(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift
    }

    /// Typed text for a text field: nothing for ctrl chords.
    pub fn typed(&self) -> Option<&str> {
        self.text.as_deref().filter(|_| !self.ctrl)
    }

    pub fn is_paste(&self) -> bool {
        self.ctrl && self.name == "v"
    }
}

fn named(n: Named) -> Option<&'static str> {
    Some(match n {
        Named::Enter => "enter",
        Named::Escape => "escape",
        Named::Backspace => "backspace",
        Named::Delete => "delete",
        Named::Tab => "tab",
        Named::Space => "space",
        Named::ArrowUp => "up",
        Named::ArrowDown => "down",
        Named::ArrowLeft => "left",
        Named::ArrowRight => "right",
        Named::Home => "home",
        Named::End => "end",
        Named::PageUp => "pageup",
        Named::PageDown => "pagedown",
        // Remote / media keys, in the keyboard vocabulary (players read them as Space, N, P, Esc and
        // the settings menu; elsewhere Back goes back and Menu opens the account menu).
        Named::MediaPlayPause | Named::MediaPlay | Named::MediaPause => "space",
        Named::MediaTrackNext => "n",
        Named::MediaTrackPrevious => "p",
        Named::BrowserBack | Named::GoBack => "escape",
        Named::ContextMenu => "menu",
        _ => return None,
    })
}

/// `None` for releases and keys the app has no name for (modifiers alone, function keys).
pub fn from_event(e: &keyboard::Event) -> Option<Key> {
    let keyboard::Event::KeyPressed {
        key,
        modifiers,
        text,
        ..
    } = e
    else {
        return None;
    };
    let name = match key {
        K::Named(n) => named(*n)?.to_string(),
        K::Character(c) => c.to_lowercase(),
        K::Unidentified => return None,
    };
    let text = if name == "space" {
        Some(" ".to_string())
    } else {
        text.as_ref().map(|t| t.to_string())
    };
    Some(Key {
        name,
        text,
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
        shift: modifiers.shift(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Location, Modifiers, key::Physical};

    fn ev(key: K, text: Option<&str>, m: Modifiers) -> keyboard::Event {
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: m,
            text: text.map(Into::into),
            repeat: false,
        }
    }

    #[test]
    fn names_follow_the_gpui_vocabulary() {
        let k = from_event(&ev(K::Named(Named::Enter), None, Modifiers::empty())).unwrap();
        assert_eq!(k.name, "enter");
        let k = from_event(&ev(K::Character("A".into()), Some("A"), Modifiers::SHIFT)).unwrap();
        assert_eq!(
            (k.name.as_str(), k.typed(), k.shift),
            ("a", Some("A"), true)
        );
        let k = from_event(&ev(K::Named(Named::Space), None, Modifiers::empty())).unwrap();
        assert_eq!((k.name.as_str(), k.typed()), ("space", Some(" ")));
        assert!(from_event(&ev(K::Named(Named::Shift), None, Modifiers::empty())).is_none());
    }

    #[test]
    fn remote_keys_use_the_keyboard_names() {
        let n = |k: Named| {
            from_event(&ev(K::Named(k), None, Modifiers::empty()))
                .unwrap()
                .name
        };
        assert_eq!(n(Named::MediaPlayPause), "space");
        assert_eq!(n(Named::MediaTrackNext), "n");
        assert_eq!(n(Named::MediaTrackPrevious), "p");
        assert_eq!(n(Named::BrowserBack), "escape");
        assert_eq!(n(Named::ContextMenu), "menu");
    }

    #[test]
    fn ctrl_chords_type_nothing() {
        let k = from_event(&ev(K::Character("v".into()), Some("v"), Modifiers::CTRL)).unwrap();
        assert!(k.is_paste() && k.typed().is_none());
    }
}
