//! A tolerant, non-recursive CSS reader: comments are blanked, top-level rules are split into
//! selectors and declarations, at-rules are reported and skipped whole. It knows nothing about
//! meaning; `mod.rs` translates. Positions are char indices into the (comment-blanked) input.
use super::{MAX_DECLS, MAX_RULES, WarningKind, Warnings};

pub struct Decl {
    pub name: String,
    pub value: String,
    pub pos: usize,
}

pub struct Rule {
    /// Each selector with the char index it starts at.
    pub sels: Vec<(String, usize)>,
    pub decls: Vec<Decl>,
}

fn skip_str(c: &[char], i: usize) -> usize {
    let q = c[i];
    let mut j = i + 1;
    while j < c.len() {
        match c[j] {
            '\\' => j += 2,
            ch if ch == q => return j + 1,
            '\n' => return j,
            _ => j += 1,
        }
    }
    c.len()
}

/// First index at or after `i` holding one of `targets` outside strings and `()`/`[]` nesting.
fn find(c: &[char], mut i: usize, targets: &[char]) -> usize {
    let mut depth = 0i32;
    while i < c.len() {
        match c[i] {
            '"' | '\'' => {
                i = skip_str(c, i);
                continue;
            }
            '\\' => {
                i += 2;
                continue;
            }
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            ch if depth == 0 && targets.contains(&ch) => return i,
            _ => {}
        }
        i += 1;
    }
    c.len()
}

/// Index of the `}` matching the `{` at `open`, or the input length if it never closes.
fn block_end(c: &[char], open: usize) -> usize {
    let (mut depth, mut i) = (0i32, open);
    while i < c.len() {
        match c[i] {
            '"' | '\'' => {
                i = skip_str(c, i);
                continue;
            }
            '\\' => {
                i += 2;
                continue;
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth <= 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    c.len()
}

/// Replace every comment with spaces (newlines kept so line numbers stay right).
pub fn blank_comments(c: &mut [char], w: &mut Warnings) {
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '"' | '\'' => i = skip_str(c, i),
            '\\' => i += 2,
            '/' if c.get(i + 1) == Some(&'*') => {
                let start = i;
                i += 2;
                let mut closed = false;
                while i < c.len() {
                    if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                        i += 2;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                let end = i.min(c.len());
                for ch in &mut c[start..end] {
                    if *ch != '\n' {
                        *ch = ' ';
                    }
                }
                if !closed {
                    w.push(start, WarningKind::SyntaxError, "unterminated comment");
                }
            }
            _ => i += 1,
        }
    }
}

fn text(c: &[char], a: usize, b: usize) -> String {
    c[a.min(c.len())..b.min(c.len())].iter().collect()
}

fn first_non_ws(c: &[char], a: usize, b: usize) -> usize {
    (a..b.min(c.len()))
        .find(|&i| !c[i].is_whitespace())
        .unwrap_or(a)
}

fn decl(c: &[char], a: usize, b: usize, w: &mut Warnings) -> Option<Decl> {
    let pos = first_non_ws(c, a, b);
    let raw = text(c, a, b);
    if raw.trim().is_empty() {
        return None;
    }
    let colon = find(&c[..b], a, &[':']);
    if colon >= b {
        w.push(
            pos,
            WarningKind::SyntaxError,
            format!("expected `name: value`, found `{}`", clip(raw.trim())),
        );
        return None;
    }
    let mut name = text(c, a, colon).trim().to_string();
    if !name.starts_with("--") {
        name.make_ascii_lowercase();
    }
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_')
    {
        w.push(
            pos,
            WarningKind::SyntaxError,
            format!("`{}` is not a property name", clip(&name)),
        );
        return None;
    }
    let mut value = text(c, colon + 1, b).trim().to_string();
    if let Some(p) = value.rfind('!')
        && value[p + 1..].trim().eq_ignore_ascii_case("important")
    {
        value.truncate(p);
        value = value.trim_end().to_string();
    }
    if value.is_empty() && !name.starts_with("--") {
        w.push(
            pos,
            WarningKind::SyntaxError,
            format!("`{name}` has no value"),
        );
        return None;
    }
    Some(Decl { name, value, pos })
}

pub fn clip(s: &str) -> String {
    let mut t: String = s.chars().take(60).collect();
    if s.chars().count() > 60 {
        t.push_str("...");
    }
    t
}

fn decls(c: &[char], start: usize, end: usize, w: &mut Warnings) -> Vec<Decl> {
    let c = &c[..end.min(c.len())];
    let (mut out, mut i) = (vec![], start);
    while i < c.len() {
        let j = find(c, i, &[';', '{']);
        if j < c.len() && c[j] == '{' {
            w.push(
                first_non_ws(c, i, j),
                WarningKind::UnsupportedSelector,
                "nested rules are not supported",
            );
            i = block_end(c, j) + 1;
            continue;
        }
        out.extend(decl(c, i, j, w));
        i = j + 1;
    }
    out
}

fn selectors(c: &[char], a: usize, b: usize) -> Vec<(String, usize)> {
    let c = &c[..b];
    let (mut out, mut s) = (vec![], a);
    while s <= b {
        let e = find(c, s, &[',']);
        let from = first_non_ws(c, s, e);
        let t = text(c, from, e).trim().to_string();
        if !t.is_empty() {
            out.push((t, from));
        }
        s = e + 1;
    }
    out
}

fn at_rule(c: &[char], i: usize, w: &mut Warnings) -> usize {
    let name: String = c[i + 1..]
        .iter()
        .take_while(|ch| ch.is_alphanumeric() || **ch == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let j = find(c, i, &[';', '{']);
    if j >= c.len() || c[j] == ';' {
        match name.as_str() {
            "import" => w.push(
                i,
                WarningKind::RemoteResource,
                "`@import` is not fetched; styles from other files or sites are ignored",
            ),
            "charset" => {}
            _ => w.push(
                i,
                WarningKind::UnsupportedAtRule,
                format!("`@{name}` is not supported"),
            ),
        }
        return j + 1;
    }
    let end = block_end(c, j);
    match name.as_str() {
        "font-face" => w.push(
            i,
            WarningKind::FontNotDownloaded,
            "`@font-face` is not supported: fonts are never downloaded or loaded from a stylesheet",
        ),
        "media" | "supports" | "container" | "layer" => w.push(
            i,
            WarningKind::UnsupportedAtRule,
            format!(
                "`@{name}` blocks are skipped with the {} characters inside",
                end.saturating_sub(j)
            ),
        ),
        "keyframes" | "-webkit-keyframes" => w.push(
            i,
            WarningKind::UnsupportedAtRule,
            "`@keyframes`: animations are not supported",
        ),
        _ => w.push(
            i,
            WarningKind::UnsupportedAtRule,
            format!("`@{name}` is not supported"),
        ),
    }
    end + 1
}

pub fn parse(c: &[char], w: &mut Warnings) -> Vec<Rule> {
    let (mut rules, mut decl_total, mut i) = (vec![], 0usize, 0usize);
    loop {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        if i >= c.len() {
            break;
        }
        if rules.len() >= MAX_RULES || decl_total >= MAX_DECLS {
            w.push(
                i,
                WarningKind::LimitReached,
                "stylesheet too large: the rest was not read",
            );
            break;
        }
        match c[i] {
            '}' => {
                w.push(i, WarningKind::SyntaxError, "unmatched `}`");
                i += 1;
            }
            '@' => i = at_rule(c, i, w),
            _ => {
                let open = find(c, i, &['{']);
                if open >= c.len() {
                    w.push(i, WarningKind::SyntaxError, "rule without a `{ }` block");
                    break;
                }
                let end = block_end(c, open);
                let ds = decls(c, open + 1, end, w);
                decl_total += ds.len();
                rules.push(Rule {
                    sels: selectors(c, i, open),
                    decls: ds,
                });
                i = end + 1;
            }
        }
    }
    rules
}
