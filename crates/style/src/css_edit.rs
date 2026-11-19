//! Editing a theme's CSS text one declaration at a time, for the Theme Studio: a rule per selector, kept tidy, merged and ordered by
//! specificity. Only top-level style rules are touched; comments, `@import`, `@font-face`, `@media` and everything the parser does
//! not understand are kept byte for byte, and a text that is not edited prints exactly as it was read.
use crate::select;
use crate::sheet::{block_end, block_start, skip_comment};
use nocturne_core::customize::value::split_top;

#[derive(Clone, Debug, PartialEq)]
enum Chunk {
    /// Comments, at-rules, whitespace and anything not understood.
    Raw(String),
    Rule(Rule),
}

#[derive(Clone, Debug, PartialEq)]
struct Rule {
    /// Whitespace before the rule, kept when the rule is rewritten.
    lead: String,
    sel: String,
    decls: Vec<(String, String)>,
    /// The text as read, while the rule is untouched.
    orig: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Css {
    chunks: Vec<Chunk>,
}

fn norm(sel: &str) -> String {
    sel.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i, mut from) = (String::new(), 0, 0);
    while i + 1 < b.len() {
        if b[i] == b'/' && b[i + 1] == b'*' {
            out.push_str(s.get(from..i).unwrap_or(""));
            i = skip_comment(b, i);
            from = i;
        } else {
            i += 1;
        }
    }
    out.push_str(s.get(from..).unwrap_or(""));
    out
}

fn decls_of(body: &str) -> Vec<(String, String)> {
    let body = strip_comments(body);
    split_top(&body, Some(';'))
        .into_iter()
        .filter_map(|d| {
            let (n, v) = d.split_once(':')?;
            let (n, v) = (n.trim(), v.trim());
            (!n.is_empty() && !v.is_empty()).then(|| (prop_name(n), v.to_owned()))
        })
        .collect()
}

/// Property names are case-insensitive, custom properties are not.
fn prop_name(s: &str) -> String {
    if s.starts_with("--") {
        s.to_owned()
    } else {
        s.to_ascii_lowercase()
    }
}

fn split_important(v: &str) -> (&str, bool) {
    let t = v.trim_end();
    match t.strip_suffix("important") {
        Some(r) if r.trim_end().ends_with('!') => {
            (r.trim_end().trim_end_matches('!').trim_end(), true)
        }
        _ => (t, false),
    }
}

impl Rule {
    fn render(&self) -> String {
        let mut s = format!("{}{} {{\n", self.lead, self.sel);
        for (n, v) in &self.decls {
            s.push_str(&format!("  {n}: {v};\n"));
        }
        s.push('}');
        s
    }
}

impl Css {
    pub fn parse(text: &str) -> Css {
        let b = text.as_bytes();
        let mut chunks = vec![];
        let (mut start, mut i) = (0usize, 0usize);
        let raw = |chunks: &mut Vec<Chunk>, from: usize, to: usize| {
            if let Some(t) = text.get(from..to.min(text.len()))
                && !t.is_empty()
            {
                chunks.push(Chunk::Raw(t.to_owned()));
            }
        };
        while i < b.len() {
            if b[i].is_ascii_whitespace() {
                i += 1;
                continue;
            }
            if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                i = skip_comment(b, i);
                raw(&mut chunks, start, i);
                start = i;
                continue;
            }
            let j = block_start(b, i);
            if j >= b.len() {
                break;
            }
            if b[j] == b';' {
                raw(&mut chunks, start, j + 1);
                (start, i) = (j + 1, j + 1);
                continue;
            }
            let end = (block_end(b, j) + 1).min(b.len());
            let (Some(prelude), Some(body), Some(whole)) = (
                text.get(i..j),
                text.get(j + 1..end.saturating_sub(1).max(j + 1)),
                text.get(start..end),
            ) else {
                break;
            };
            let prelude = prelude.trim();
            if prelude.starts_with('@') || prelude.is_empty() {
                raw(&mut chunks, start, end);
            } else {
                chunks.push(Chunk::Rule(Rule {
                    lead: text[start..i].to_owned(),
                    sel: norm(prelude),
                    decls: decls_of(body),
                    orig: Some(whole.to_owned()),
                }));
            }
            (start, i) = (end, end);
        }
        raw(&mut chunks, start, text.len());
        Css { chunks }
    }

    pub fn to_css(&self) -> String {
        let mut out = String::new();
        for c in &self.chunks {
            match c {
                Chunk::Raw(t) => out.push_str(t),
                Chunk::Rule(r) => match &r.orig {
                    Some(o) => out.push_str(o),
                    None => out.push_str(&r.render()),
                },
            }
        }
        out
    }

    fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.chunks.iter().filter_map(|c| match c {
            Chunk::Rule(r) => Some(r),
            Chunk::Raw(_) => None,
        })
    }

    /// The value (without `!important`) the last top-level rule for exactly this selector gives the property.
    pub fn get(&self, sel: &str, prop: &str) -> Option<String> {
        let sel = norm(sel);
        self.rules()
            .filter(|r| r.sel == sel)
            .flat_map(|r| r.decls.iter())
            .filter(|(n, _)| n == prop)
            .last()
            .map(|(_, v)| split_important(v).0.to_owned())
    }

    /// Does any top-level rule use exactly this selector?
    pub fn has(&self, sel: &str) -> bool {
        let sel = norm(sel);
        self.rules().any(|r| r.sel == sel)
    }

    /// Set a property in the rule of this selector, making the rule (in specificity order) when there is none.
    pub fn set(&mut self, sel: &str, prop: &str, value: &str, important: bool) {
        let sel = norm(sel);
        let prop = prop_name(prop);
        let value = if important {
            format!("{} !important", value.trim())
        } else {
            value.trim().to_owned()
        };
        let at = self
            .chunks
            .iter()
            .rposition(|c| matches!(c, Chunk::Rule(r) if r.sel == sel));
        if let Some(at) = at {
            let Chunk::Rule(r) = &mut self.chunks[at] else {
                return;
            };
            r.decls.retain(|(n, _)| *n != prop);
            r.decls.push((prop, value));
            r.orig = None;
            return;
        }
        let spec = select::parse(&sel).map_or(0, |s| s.specificity());
        // After the last rule that is not stronger, so that an equal rule written earlier loses.
        let pos = self
            .chunks
            .iter()
            .rposition(|c| {
                matches!(c, Chunk::Rule(r) if select::parse(&r.sel).map_or(0, |s| s.specificity()) <= spec)
            })
            .map_or_else(
                || {
                    self.chunks
                        .iter()
                        .position(|c| matches!(c, Chunk::Rule(_)))
                        .unwrap_or(self.chunks.len())
                },
                |p| p + 1,
            );
        let rule = Rule {
            lead: "\n".into(),
            sel,
            decls: vec![(prop, value)],
            orig: None,
        };
        self.chunks.insert(pos, Chunk::Rule(rule));
        self.tidy_edges(pos);
    }

    /// A rule inserted at the very end must start on its own line and the file should end with a newline.
    fn tidy_edges(&mut self, pos: usize) {
        if pos + 1 == self.chunks.len() {
            self.chunks.push(Chunk::Raw("\n".into()));
        }
        if pos > 0
            && let Chunk::Raw(t) = &self.chunks[pos - 1]
            && !t.ends_with('\n')
            && !t.is_empty()
            && let Chunk::Rule(r) = &mut self.chunks[pos]
        {
            r.lead = "\n\n".into();
        }
    }

    /// Drop a property from the rule of this selector (the rule goes when it becomes empty).
    pub fn remove(&mut self, sel: &str, prop: &str) -> bool {
        let sel = norm(sel);
        let mut changed = false;
        for c in &mut self.chunks {
            if let Chunk::Rule(r) = c
                && r.sel == sel
            {
                let n = r.decls.len();
                r.decls.retain(|(k, _)| k != prop);
                if r.decls.len() != n {
                    r.orig = None;
                    changed = true;
                }
            }
        }
        self.chunks
            .retain(|c| !matches!(c, Chunk::Rule(r) if r.sel == sel && r.decls.is_empty() && r.orig.is_none()));
        changed
    }

    /// Remove every top-level rule written for this selector and its state and pseudo-element variants (`sel:hover`, `sel::before`,
    /// `sel > x` is a different element and stays).
    pub fn reset(&mut self, sel: &str) -> bool {
        let sel = norm(sel);
        let n = self.chunks.len();
        self.chunks.retain(|c| match c {
            Chunk::Rule(r) => {
                r.sel != sel && !(r.sel.starts_with(&sel) && r.sel[sel.len()..].starts_with(':'))
            }
            Chunk::Raw(_) => true,
        });
        self.chunks.len() != n
    }

    /// The selectors of the top-level rules, in order.
    pub fn selectors(&self) -> Vec<String> {
        self.rules().map(|r| r.sel.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "/* mine */\n@import url(\"x.css\");\n.card { color: red; }\n\n@media (min-width: 600px) { .card { color: blue } }\n.card:hover{transform:scale(1.05)}\n";

    #[test]
    fn untouched_text_round_trips_exactly() {
        assert_eq!(Css::parse(SRC).to_css(), SRC);
        for s in [
            "",
            "   ",
            ".a{",
            ".a { color: red",
            "}}{{",
            "@import 'x';",
            "é { x: é }",
        ] {
            assert_eq!(Css::parse(s).to_css(), s);
        }
    }

    #[test]
    fn reads_values() {
        let c = Css::parse(SRC);
        assert_eq!(c.get(".card", "color").as_deref(), Some("red"));
        assert_eq!(
            c.get(".card:hover", "transform").as_deref(),
            Some("scale(1.05)")
        );
        assert_eq!(c.get(".card", "width"), None);
        // The @media rule is not a top-level rule.
        assert_eq!(c.selectors(), [".card", ".card:hover"]);
        let i = Css::parse(".a { color: red !important }");
        assert_eq!(i.get(".a", "color").as_deref(), Some("red"));
    }

    #[test]
    fn set_edits_in_place_and_keeps_the_rest() {
        let mut c = Css::parse(SRC);
        c.set(".card", "color", "#fff", false);
        let out = c.to_css();
        assert!(
            out.starts_with("/* mine */\n@import url(\"x.css\");\n.card {\n  color: #fff;\n}"),
            "{out}"
        );
        assert!(out.contains("@media (min-width: 600px) { .card { color: blue } }"));
        assert!(out.ends_with(".card:hover{transform:scale(1.05)}\n"));
        assert_eq!(
            Css::parse(&out).get(".card", "color").as_deref(),
            Some("#fff")
        );
    }

    #[test]
    fn set_merges_into_one_rule_per_selector() {
        let mut c = Css::default();
        c.set(".card", "color", "red", false);
        c.set(".card", "width", "10px", false);
        c.set(".card", "color", "blue", false);
        assert_eq!(c.to_css(), "\n.card {\n  width: 10px;\n  color: blue;\n}\n");
        assert_eq!(c.selectors().len(), 1);
    }

    #[test]
    fn new_rules_follow_specificity_order() {
        let mut c = Css::parse(".a { x: 1 }\n#id { x: 2 }\n");
        c.set(".b", "color", "red", false);
        assert_eq!(c.selectors(), [".a", ".b", "#id"]);
        c.set("p", "color", "red", false);
        assert_eq!(c.selectors()[0], "p");
        // Equal specificity: the new rule goes after the old one so that it wins.
        let mut c = Css::parse(".a { x: 1 }\n.c { x: 3 }\n");
        c.set(".b", "color", "red", false);
        assert_eq!(c.selectors(), [".a", ".c", ".b"]);
    }

    #[test]
    fn remove_and_reset() {
        let mut c = Css::parse(SRC);
        assert!(c.remove(".card", "color"));
        assert!(!c.has(".card"));
        assert!(c.has(".card:hover"));
        let mut c = Css::parse(SRC);
        c.set(".card", "width", "5px", false);
        assert!(c.reset(".card"));
        assert_eq!(c.selectors(), Vec::<String>::new());
        assert!(c.to_css().contains("@media"), "at-rules survive a reset");
        assert!(!c.reset(".card"));
    }

    #[test]
    fn important_and_pseudo_selectors() {
        let mut c = Css::default();
        c.set(".a:hover", "transform", "scale(1.1)", true);
        assert!(c.to_css().contains("transform: scale(1.1) !important;"));
        assert_eq!(
            c.get(".a:hover", "transform").as_deref(),
            Some("scale(1.1)")
        );
    }

    #[test]
    fn selectors_are_compared_by_normalised_text() {
        let mut c = Css::parse(".a  >   .b { color: red }");
        c.set(".a > .b", "width", "1px", false);
        assert_eq!(c.selectors().len(), 1);
    }
}
