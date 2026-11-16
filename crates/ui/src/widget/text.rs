//! Text helpers: tracked caps labels, wordmark, mono annotations.
use crate::theme as t;
use iced::widget::{Text, text};

/// No letter-spacing in the text engine, so tracked caps are faked with hair spaces.
pub fn tracked(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.to_uppercase().chars().enumerate() {
        if c == ' ' {
            out.push('\u{2003}'); // em space: visible word gap between tracked letters
            continue;
        }
        if i > 0 && !out.ends_with('\u{2003}') {
            out.push('\u{200A}');
            out.push('\u{200A}');
        }
        out.push(c);
    }
    out
}

/// Tracked uppercase label (tagline, eyebrows).
pub fn label<'a>(s: &str) -> Text<'a> {
    text(tracked(s))
        .size(t::CAPTION)
        .font(t::MEDIUM)
        .color(t::TEXT_TERTIARY)
}

pub fn wordmark<'a>(size: f32) -> Text<'a> {
    text(tracked("Nocturne"))
        .size(size)
        .font(t::BOLD)
        .color(t::TEXT)
}

/// Tiny monospace annotation (the only text allowed below 12 px).
pub fn mono<'a>(s: impl iced::widget::text::IntoFragment<'a>) -> Text<'a> {
    text(s).font(t::MONO).size(t::MICRO).color(t::TEXT_TERTIARY)
}

pub fn initial(title: &str) -> String {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_uppercases_and_spaces() {
        assert_eq!(tracked("ab"), "A\u{200A}\u{200A}B");
        assert_eq!(tracked("a b"), "A\u{2003}B");
    }

    #[test]
    fn initial_skips_punctuation() {
        assert_eq!(initial("  \"the end"), "T");
        assert_eq!(initial("..."), "");
    }
}
