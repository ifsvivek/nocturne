//! Expandable descriptions: collapsed to a few lines by default, toggled by click or Enter.
//! Iced has no line clamp, so the clamp is done here on an estimate of characters per line.

use std::collections::HashSet;

/// Which descriptions are open, by item key (`origin:id`). Collapsed is the default.
#[derive(Clone, Debug, Default)]
pub struct Expanded(HashSet<String>);

impl Expanded {
    pub fn is_open(&self, key: &str) -> bool {
        self.0.contains(key)
    }

    /// Flip one description; returns whether it is now open.
    pub fn toggle(&mut self, key: &str) -> bool {
        if self.0.remove(key) {
            false
        } else {
            self.0.insert(key.to_string());
            true
        }
    }

    /// Any description open (pauses the hero carousel).
    pub fn any_open(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// `text` cut to about `lines` lines of `cols` characters at a word boundary with an ellipsis,
/// and whether anything was cut (the UI shows the toggle only then). Each paragraph starts a line.
pub fn clamp(text: &str, cols: usize, lines: usize) -> (String, bool) {
    let (cols, lines) = (cols.max(1), lines.max(1));
    let (mut out, mut line, mut used) = (String::new(), 0, 0);
    for (n, para) in text.trim().split('\n').enumerate() {
        if n > 0 {
            used += 1;
            line = 0;
            if used >= lines {
                return (finish(out), true);
            }
            out.push('\n');
        }
        for word in para.split_whitespace() {
            let w = word.chars().count();
            let wrap = line > 0 && line + 1 + w > cols;
            if wrap {
                used += 1;
                line = 0;
                if used >= lines {
                    return (finish(out), true);
                }
            }
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
                line += usize::from(line > 0);
            }
            out.push_str(word);
            line += w;
        }
    }
    (out, false)
}

fn finish(s: String) -> String {
    format!(
        "{}…",
        s.trim_end()
            .trim_end_matches(|c: char| c.is_ascii_punctuation())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_per_item_and_starts_collapsed() {
        let mut e = Expanded::default();
        assert!(!e.is_open("1:a") && !e.any_open());
        assert!(e.toggle("1:a"));
        assert!(e.is_open("1:a") && !e.is_open("1:b") && e.any_open());
        assert!(!e.toggle("1:a"));
        assert!(!e.any_open());
        e.toggle("1:b");
        e.clear();
        assert!(!e.any_open());
    }

    #[test]
    fn short_text_is_not_clamped() {
        assert_eq!(
            clamp("A short line.", 40, 3),
            ("A short line.".into(), false)
        );
        assert_eq!(clamp("", 40, 3), (String::new(), false));
    }

    #[test]
    fn long_text_is_cut_at_a_word_with_an_ellipsis() {
        let t = "one two three four five six seven eight nine ten eleven twelve";
        let (s, cut) = clamp(t, 14, 2);
        assert!(cut);
        assert_eq!(s, "one two three four five six…");
        assert!(s.chars().count() <= 14 * 2 + 1);
        let (all, cut) = clamp(t, 14, 10);
        assert!(!cut);
        assert_eq!(all, t);
    }

    #[test]
    fn paragraphs_count_as_lines() {
        let (s, cut) = clamp("first\nsecond\nthird", 40, 2);
        assert!(cut);
        assert_eq!(s, "first\nsecond…");
        assert_eq!(
            clamp("first\nsecond", 40, 2),
            ("first\nsecond".into(), false)
        );
    }
}
