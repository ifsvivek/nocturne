//! Jellyfin web compatibility by selector translation (`docs/dev/THEME_COMPAT.md`). A stylesheet written for jellyfin-web is rewritten, rule by rule, into selectors and custom properties of NOCTURNE's trees before the engine reads it: classes and class chains are mapped (`compat_map.rs`), wrappers with no element are dropped from chains, `[data-type=Movie]` becomes `[data-kind=movie]`, Jellyfin and MUI variables are mirrored onto the `--nocturne-*` tokens. Selectors that have no counterpart are reported, grouped by reason, never dropped silently. Text in, text out: the engine's own reader does the rest, and NOCTURNE's own sheets never go through here.
use crate::compat_map::{self as map, To};
use crate::select;
use crate::sheet::{block_end, block_start, skip_comment, skip_string};
use crate::warn::{Kind, Report};
use nocturne_core::customize::value::split_top;
use std::collections::{BTreeMap, HashSet};

/// Examples kept per reason.
const DETAILS: usize = 400;

/// One reason selectors were not translated, with what was lost (a class name, an attribute, a tag) and how often.
#[derive(Clone, Debug, Default)]
pub struct Lost {
    pub count: usize,
    pub details: BTreeMap<String, usize>,
}

/// A reason, how many selectors it cost, and the names behind it (most frequent first).
pub type Group<'a> = (&'a str, usize, Vec<(&'a str, usize)>);

/// What translating one stylesheet did.
#[derive(Clone, Debug, Default)]
pub struct Compat {
    /// Selectors in the sheet (each member of a selector list counts), and how many were translated.
    pub selectors: usize,
    pub translated: usize,
    /// Rules, and the rules left with at least one selector.
    pub rules: usize,
    pub rules_kept: usize,
    /// Declarations (once per rule) and those in a rule that kept a selector.
    pub decls: usize,
    pub decls_reachable: usize,
    /// Jellyfin-only constructs met: below one means the sheet is not a Jellyfin theme and was left as it is.
    pub hits: usize,
    /// `>` that became a descendant combinator because NOCTURNE nests the two elements differently.
    pub child_to_descendant: usize,
    /// Custom properties mirrored onto NOCTURNE tokens, and defaults added for variables the sheet only reads.
    pub vars_mirrored: usize,
    pub vars_defaulted: usize,
    pub lost: BTreeMap<String, Lost>,
    /// The sheet was recognised as Jellyfin's and rewritten.
    pub applied: bool,
}

impl Compat {
    pub fn percent_selectors(&self) -> f32 {
        pct(self.translated, self.selectors)
    }

    /// Selectors lost because the name is not in the map (candidates for the map), as against lost by design (a role NOCTURNE does not have).
    pub fn unmapped(&self) -> usize {
        self.lost
            .iter()
            .filter(|(k, _)| {
                k.starts_with("class with no entry") || k.starts_with("ids with no entry")
            })
            .map(|(_, l)| l.count)
            .sum()
    }

    pub fn percent_decls(&self) -> f32 {
        pct(self.decls_reachable, self.decls)
    }

    /// Reasons, most frequent first, with their top details.
    pub fn groups(&self) -> Vec<Group<'_>> {
        let mut v: Vec<_> = self
            .lost
            .iter()
            .map(|(k, l)| {
                let mut d: Vec<_> = l.details.iter().map(|(k, n)| (k.as_str(), *n)).collect();
                d.sort_by_key(|(k, n)| (std::cmp::Reverse(*n), *k));
                (k.as_str(), l.count, d)
            })
            .collect();
        v.sort_by_key(|(k, n, _)| (std::cmp::Reverse(*n), *k));
        v
    }

    /// Put the groups into a sheet's warning report (kind `Compat`: informational, never a failure).
    pub fn add_to(&self, rep: &mut Report) {
        if !self.applied {
            return;
        }
        for (why, n, details) in self.groups() {
            let names: Vec<&str> = details.iter().take(4).map(|(k, _)| *k).collect();
            rep.add_n(Kind::Compat, n, format!("{why} ({})", names.join(", ")));
        }
    }

    fn lose(&mut self, f: Fail) {
        let l = self.lost.entry(f.group).or_default();
        l.count += 1;
        if l.details.len() < DETAILS || l.details.contains_key(&f.detail) {
            *l.details.entry(f.detail).or_default() += 1;
        }
    }
}

fn pct(a: usize, b: usize) -> f32 {
    if b == 0 {
        100.0
    } else {
        100.0 * a as f32 / b as f32
    }
}

#[derive(Debug)]
struct Fail {
    group: String,
    detail: String,
}

fn fail<T>(group: &str, detail: impl Into<String>) -> Result<T, Fail> {
    Err(Fail {
        group: group.into(),
        detail: detail.into(),
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum C {
    Desc,
    Child,
    Next,
    Sub,
}

impl C {
    fn sym(self) -> &'static str {
        match self {
            C::Desc => " ",
            C::Child => " > ",
            C::Next => " + ",
            C::Sub => " ~ ",
        }
    }

    fn sibling(self) -> bool {
        matches!(self, C::Next | C::Sub)
    }
}

// ---- reading selectors ----

#[derive(Clone, Copy, PartialEq, Debug)]
enum K {
    Univ,
    Tag,
    Id,
    Class,
    Attr,
    Pseudo,
    PseudoEl,
}

struct Sm<'a> {
    k: K,
    raw: &'a str,
    name: &'a str,
    arg: Option<&'a str>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

fn ident_end(s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        let c = s[i..].chars().next().unwrap_or(' ');
        if !is_ident(c) {
            break;
        }
        i += c.len_utf8();
    }
    i.min(s.len())
}

/// Index just past the bracket or parenthesis that opens at `i` (quotes respected).
fn close_at(s: &str, i: usize) -> usize {
    let b = s.as_bytes();
    let (mut depth, mut j) = (0i32, i);
    while j < b.len() {
        match b[j] {
            b'"' | b'\'' => {
                j = skip_string(b, j);
                continue;
            }
            b'\\' => {
                j += 2;
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len()
}

fn simples(s: &str) -> Result<Vec<Sm<'_>>, Fail> {
    let (mut out, mut i) = (vec![], 0);
    while i < s.len() {
        let c = s[i..].chars().next().unwrap_or(' ');
        match c {
            '*' => {
                out.push(Sm {
                    k: K::Univ,
                    raw: &s[i..i + 1],
                    name: "*",
                    arg: None,
                });
                i += 1;
            }
            '#' | '.' => {
                let e = ident_end(s, i + 1);
                out.push(Sm {
                    k: if c == '#' { K::Id } else { K::Class },
                    raw: &s[i..e],
                    name: &s[i + 1..e],
                    arg: None,
                });
                i = e;
            }
            '[' => {
                let e = close_at(s, i);
                out.push(Sm {
                    k: K::Attr,
                    raw: &s[i..e],
                    name: s[i + 1..e.saturating_sub(1).max(i + 1)].trim(),
                    arg: None,
                });
                i = e;
            }
            ':' => {
                let el = s[i + 1..].starts_with(':');
                let st = i + if el { 2 } else { 1 };
                let ne = ident_end(s, st);
                let (e, arg) = if s[ne..].starts_with('(') {
                    let e = close_at(s, ne);
                    (e, Some(&s[ne + 1..e.saturating_sub(1).max(ne + 1)]))
                } else {
                    (ne, None)
                };
                out.push(Sm {
                    k: if el { K::PseudoEl } else { K::Pseudo },
                    raw: &s[i..e],
                    name: &s[st..ne],
                    arg,
                });
                i = e;
            }
            c if is_ident(c) || c == '\\' => {
                let e = ident_end(s, i);
                out.push(Sm {
                    k: K::Tag,
                    raw: &s[i..e],
                    name: &s[i..e],
                    arg: None,
                });
                i = e;
            }
            c => return fail("selector syntax", format!("`{c}`")),
        }
    }
    Ok(out)
}

/// Split at top-level combinators: `(combinator before it, compound)`; the first compound's combinator is `None`.
fn split_complex(s: &str) -> Vec<(Option<C>, &str)> {
    let b = s.as_bytes();
    let (mut out, mut start, mut i, mut pending) = (vec![], 0usize, 0usize, None::<C>);
    let mut first = true;
    let mut depth = 0i32;
    fn flush<'a>(
        s: &'a str,
        out: &mut Vec<(Option<C>, &'a str)>,
        range: (usize, usize),
        pending: &mut Option<C>,
        first: &mut bool,
    ) {
        if range.1 > range.0 {
            out.push((
                if *first {
                    None
                } else {
                    Some(pending.take().unwrap_or(C::Desc))
                },
                &s[range.0..range.1],
            ));
            *first = false;
        }
    }
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                i = skip_string(b, i);
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = (depth - 1).max(0),
            c if depth == 0 && (c.is_ascii_whitespace() || matches!(c, b'>' | b'+' | b'~')) => {
                flush(s, &mut out, (start, i), &mut pending, &mut first);
                match c {
                    b'>' => pending = Some(C::Child),
                    b'+' => pending = Some(C::Next),
                    b'~' => pending = Some(C::Sub),
                    _ => {}
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    flush(
        s,
        &mut out,
        (start, b.len().min(i)),
        &mut pending,
        &mut first,
    );
    out
}

// ---- translating one selector ----

#[derive(Default)]
struct Cx {
    hits: usize,
    degraded: usize,
}

enum Piece {
    Chain(Vec<(C, String)>),
    /// A compound made only of wrapper classes: it has no element.
    Transparent(String),
}

fn push_atom(parts: &mut Vec<String>, a: &str) {
    if !parts.iter().any(|p| p == a) {
        parts.push(a.to_owned());
    }
}

fn function_list(arg: &str, cx: &mut Cx, relative: bool) -> (Vec<String>, Option<Fail>) {
    let (mut ok, mut first) = (vec![], None);
    for s in split_top(arg, Some(',')) {
        match complex(s.trim(), cx, relative) {
            Ok(t) => ok.push(t),
            Err(f) => {
                first.get_or_insert(f);
            }
        }
    }
    (ok, first)
}

fn compound(text: &str, cx: &mut Cx) -> Result<Piece, Fail> {
    let (mut prefix, mut parts, mut skipped): (Vec<(C, String)>, Vec<String>, Vec<&str>) =
        (vec![], vec![], vec![]);
    let (mut tag, mut mapped_class, mut star, mut last_comb) =
        (None::<&str>, false, false, C::Desc);
    for s in simples(text)? {
        match s.k {
            K::Univ => star = true,
            K::Tag => match s.name.to_ascii_lowercase().as_str() {
                "html" | "body" => push_atom(&mut parts, ":root"),
                "button" => tag = Some("a"),
                "h1" | "h2" | "h3" => {
                    cx.hits += 1;
                    mapped_class = true;
                    push_atom(
                        &mut parts,
                        match s.name {
                            "h1" => ":is(.detail-title, .hero-title)",
                            "h2" => ":is(.row-title, .section-title)",
                            _ => ".header-name",
                        },
                    );
                }
                t if map::keep_tag(t) => tag = Some(s.name),
                _ => return fail("tags with no element", format!("<{}>", s.name)),
            },
            K::Id | K::Class => {
                let found = if s.k == K::Id {
                    map::id(s.name)
                } else {
                    map::CLASSES
                        .iter()
                        .find(|(n, _)| *n == s.name)
                        .map(|(_, t)| *t)
                };
                match found {
                    Some(To::Sel(t)) | Some(To::Same(t)) => {
                        cx.hits += usize::from(!matches!(found, Some(To::Same(_))));
                        mapped_class = true;
                        let chain = split_complex(t);
                        let last = chain.len() - 1;
                        if last > 0 {
                            if !prefix.is_empty() {
                                return fail("conflicting classes on one element", s.raw);
                            }
                            for (c, x) in &chain[..last] {
                                prefix.push((c.unwrap_or(C::Desc), (*x).to_owned()));
                            }
                            last_comb = chain[last].0.unwrap_or(C::Desc);
                        }
                        for a in simples(chain[last].1)? {
                            push_atom(&mut parts, a.raw);
                        }
                    }
                    Some(To::Skip(why)) => skipped.push(why),
                    Some(To::Never(why)) => {
                        cx.hits += 1;
                        return fail(why, s.raw);
                    }
                    None if s.k == K::Class && map::is_native_class(s.name) => {
                        push_atom(&mut parts, s.raw)
                    }
                    None if s.k == K::Id => return fail("ids with no entry in the map", s.raw),
                    None if s.name.starts_with("css-") => {
                        return fail(
                            "generated (emotion) class names have no stable meaning",
                            s.raw,
                        );
                    }
                    None if s.name.starts_with("Mui") => {
                        cx.hits += 1;
                        return fail("MUI widgets with no counterpart", s.raw);
                    }
                    None if s.name.starts_with("headroom--") => skipped.push("header scroll state"),
                    None => return fail("class with no entry in the map", s.raw),
                }
            }
            K::Attr => {
                let (n, op, v) = parse_attr(s.name);
                match (n, op.as_str(), v.trim_matches(|c| c == '"' || c == '\'')) {
                    // Every link and button of the trees is an `a`.
                    ("href", "exists", _) => tag = Some("a"),
                    ("data-role", "=", "button") => {
                        cx.hits += 1;
                        mapped_class = true;
                        push_atom(&mut parts, ".button");
                    }
                    _ => attr_atom(n, &op, v, s.raw, &mut parts, cx)?,
                }
            }
            K::PseudoEl => {
                if matches!(s.name, "before" | "after") {
                    push_atom(&mut parts, s.raw);
                } else {
                    return fail("pseudo-elements with no counterpart", s.raw);
                }
            }
            K::Pseudo => pseudo(&s, &mut parts, cx)?,
        }
    }
    if parts.is_empty() && !star && tag.is_none() {
        return Ok(Piece::Transparent(
            skipped.first().copied().unwrap_or("wrapper").to_owned(),
        ));
    }
    let mut last = String::new();
    if let Some(t) = tag.filter(|_| !mapped_class) {
        last += t;
    } else if star && parts.is_empty() {
        last += "*";
    }
    // `::before` stays last.
    parts.sort_by_key(|p| p.starts_with("::"));
    last += &parts.concat();
    prefix.push((last_comb, last));
    Ok(Piece::Chain(prefix))
}

/// `name`, operator (`exists`, `=`, `~=`...) and value of the text inside `[...]`.
fn parse_attr(inner: &str) -> (&str, String, &str) {
    let Some(p) = inner.find('=') else {
        return (inner, "exists".into(), "");
    };
    let (op, np) = match inner.as_bytes().get(p.wrapping_sub(1)) {
        Some(c @ (b'~' | b'|' | b'^' | b'$' | b'*')) if p > 0 => {
            (format!("{}=", *c as char), p - 1)
        }
        _ => ("=".to_string(), p),
    };
    let v = inner[p + 1..].trim();
    let v = v
        .strip_suffix(" i")
        .or_else(|| v.strip_suffix(" s"))
        .unwrap_or(v);
    (inner[..np].trim(), op, v)
}

fn attr_atom(
    n: &str,
    op: &str,
    v: &str,
    raw: &str,
    parts: &mut Vec<String>,
    cx: &mut Cx,
) -> Result<(), Fail> {
    if let Some(a) = map::attr(n, op, v) {
        cx.hits += 1;
        push_atom(parts, &a);
        return Ok(());
    }
    if map::is_native_attr(n) {
        // The values of Jellyfin's own `data-action` (resume, link, menu...) mean nothing here.
        let native_action = |v: &str| {
            matches!(
                v.trim_matches(|c| c == '"' || c == '\''),
                "play"
                    | "play-all"
                    | "shuffle"
                    | "watched"
                    | "favourite"
                    | "previous"
                    | "next"
                    | "series"
                    | "more"
                    | "view"
                    | "versions"
                    | "prev"
                    | "back"
                    | "fwd"
                    | "queue"
                    | "subtitles"
                    | "audio"
                    | "mute"
                    | "settings"
                    | "pause"
            )
        };
        if n != "data-action" || op == "exists" || native_action(v) {
            push_atom(parts, raw);
            return Ok(());
        }
        return {
            cx.hits += 1;
            fail("jellyfin data-action values with no counterpart", raw)
        };
    }
    fail("attribute selectors with no counterpart", format!("[{n}]"))
}

fn pseudo(s: &Sm, parts: &mut Vec<String>, cx: &mut Cx) -> Result<(), Fail> {
    let n = s.name.to_ascii_lowercase();
    match n.as_str() {
        _ if map::pass_pseudo(&n) => push_atom(parts, s.raw),
        _ if map::always_pseudo(&n) => {}
        "not" => {
            let (ok, _) = function_list(s.arg.unwrap_or(""), cx, false);
            // A member that cannot match makes its `:not` true: only the translatable members remain.
            if !ok.is_empty() {
                push_atom(parts, &format!(":not({})", ok.join(", ")));
            }
        }
        "is" | "where" | "matches" | "-webkit-any" | "-moz-any" => {
            let (ok, first) = function_list(s.arg.unwrap_or(""), cx, false);
            if ok.is_empty() {
                return Err(first.unwrap_or(Fail {
                    group: "empty selector list".into(),
                    detail: s.raw.into(),
                }));
            }
            push_atom(
                parts,
                &format!(
                    ":{}({})",
                    if n == "where" { "where" } else { "is" },
                    ok.join(", ")
                ),
            );
        }
        "has" => {
            let (ok, first) = function_list(s.arg.unwrap_or(""), cx, true);
            if ok.is_empty() {
                return Err(first.unwrap_or(Fail {
                    group: "empty selector list".into(),
                    detail: s.raw.into(),
                }));
            }
            push_atom(parts, &format!(":has({})", ok.join(", ")));
        }
        _ => return fail("state pseudo-classes with no counterpart", s.raw),
    }
    Ok(())
}

fn merge_comb(prev: C, next: C) -> Result<C, Fail> {
    Ok(match (prev.sibling(), next.sibling()) {
        (false, false) => C::Desc,
        (true, true) => prev,
        (false, true) => prev,
        (true, false) => return fail("sibling chains through a wrapper", "wrapper"),
    })
}

/// One complex selector. `relative`: the text of a `:has()` argument, which may lead with a combinator.
fn complex(sel: &str, cx: &mut Cx, relative: bool) -> Result<String, Fail> {
    let sel = sel.trim();
    let (lead, body) = match sel.chars().next() {
        Some('>') if relative => (Some(C::Child), &sel[1..]),
        Some('+') if relative => (Some(C::Next), &sel[1..]),
        Some('~') if relative => (Some(C::Sub), &sel[1..]),
        _ => (None, sel),
    };
    let items = split_complex(body);
    if items.is_empty() {
        return fail("empty selector", sel);
    }
    let mut out: Vec<(C, String)> = vec![];
    let mut carry: Option<C> = None;
    for (i, (comb, txt)) in items.iter().enumerate() {
        let comb = comb.unwrap_or(C::Desc);
        let eff = match carry.take() {
            None => comb,
            Some(prev) => merge_comb(prev, comb)?,
        };
        match compound(txt, cx)? {
            Piece::Transparent(why) => {
                if i == items.len() - 1 {
                    return fail(
                        "rules on wrappers or behaviour classes with no element",
                        why,
                    );
                }
                carry = Some(eff);
            }
            Piece::Chain(chain) => {
                for (k, (c, t)) in chain.into_iter().enumerate() {
                    let c = if k == 0 { eff } else { c };
                    if out.is_empty() {
                        out.push((C::Desc, t));
                        continue;
                    }
                    // Two Jellyfin compounds that are one NOCTURNE element (`.cardBox > .cardScalable`).
                    if matches!(c, C::Desc | C::Child) && out.last().is_some_and(|(_, l)| *l == t) {
                        continue;
                    }
                    let c = if c == C::Child && k == 0 {
                        cx.degraded += 1;
                        C::Desc
                    } else {
                        c
                    };
                    out.push((c, t));
                }
            }
        }
    }
    let mut s = String::new();
    if let Some(l) = lead {
        s += l.sym().trim();
        s += " ";
    }
    for (i, (c, t)) in out.iter().enumerate() {
        if i > 0 {
            s += c.sym();
        }
        s += t;
    }
    Ok(s)
}

/// A top-level selector: translated, then padded so that its specificity is not lower than the original's (a dropped wrapper class would otherwise let an earlier, weaker rule win).
fn top(sel: &str, cx: &mut Cx) -> Result<String, Fail> {
    let mut t = complex(sel, cx, false)?;
    if !t.contains("::")
        && let (Ok(a), Ok(mut b)) = (select::parse(sel), select::parse(&t))
    {
        let mut pad = 0;
        while b.specificity() < a.specificity() && pad < 6 {
            t += ":not(.jf-pad)";
            pad += 1;
            match select::parse(&t) {
                Ok(x) => b = x,
                Err(_) => break,
            }
        }
    }
    Ok(t)
}

// ---- walking a stylesheet ----

fn strip_comments(css: &str) -> String {
    let b = css.as_bytes();
    let (mut out, mut i, mut from) = (String::with_capacity(css.len()), 0, 0);
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => i = skip_string(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => {
                out += &css[from..i];
                i = skip_comment(b, i);
                from = i;
            }
            _ => i += 1,
        }
    }
    out += &css[from.min(css.len())..];
    out
}

struct State {
    c: Compat,
    defined: HashSet<String>,
}

fn rule(prelude: &str, body: &str, out: &mut String, st: &mut State) {
    let sels = split_top(prelude, Some(','));
    let decls: Vec<&str> = split_top(body, Some(';'))
        .into_iter()
        .filter(|d| d.contains(':'))
        .collect();
    st.c.rules += 1;
    st.c.selectors += sels.len();
    st.c.decls += decls.len();
    let mut kept = vec![];
    for s in sels {
        let mut cx = Cx::default();
        let r = top(s.trim(), &mut cx);
        st.c.hits += cx.hits;
        match r {
            Ok(t) => {
                st.c.translated += 1;
                st.c.child_to_descendant += cx.degraded;
                kept.push(t);
            }
            Err(f) => st.c.lose(f),
        }
    }
    let mut alias = String::new();
    for d in &decls {
        let Some((n, v)) = d.split_once(':') else {
            continue;
        };
        let n = n.trim();
        if n.starts_with("--") {
            st.defined.insert(n.to_owned());
            if let Some((_, toks)) = map::VARS.iter().find(|(k, _)| *k == n) {
                st.c.hits += 1;
                for t in *toks {
                    alias += &format!("{t}:{v};");
                    st.c.vars_mirrored += 1;
                }
            }
        }
    }
    if kept.is_empty() {
        return;
    }
    st.c.rules_kept += 1;
    st.c.decls_reachable += decls.len();
    *out += &kept.join(", ");
    *out += "{";
    *out += body.trim_end_matches(|c: char| c.is_whitespace() || c == ';');
    *out += ";";
    *out += &alias;
    *out += "}\n";
}

fn walk(src: &str, out: &mut String, st: &mut State, depth: u8) {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let head = block_start(b, i);
        if head >= b.len() || b[head] == b';' {
            // A statement (`@import`, `@charset`) or trailing text: kept as it is.
            *out += &src[i..(head + 1).min(src.len())];
            *out += "\n";
            i = head + 1;
            continue;
        }
        let prelude = src[i..head].trim();
        let end = block_end(b, head).min(src.len());
        let body = &src[head + 1..end];
        let name: String = prelude
            .strip_prefix('@')
            .map(|r| {
                r.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect::<String>()
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        if prelude.starts_with('@') {
            if matches!(name.as_str(), "media" | "supports" | "layer" | "container") && depth < 4 {
                *out += prelude;
                *out += "{\n";
                walk(body, out, st, depth + 1);
                *out += "}\n";
            } else {
                *out += &src[i..(end + 1).min(src.len())];
                *out += "\n";
            }
        } else {
            rule(prelude, body, out, st);
        }
        i = end + 1;
    }
}

/// Names of the Jellyfin variables a sheet reads with `var(--x)`.
fn referenced(css: &str) -> Vec<String> {
    let mut v = vec![];
    for part in css.split("var(").skip(1) {
        let n: String = part
            .trim_start()
            .chars()
            .take_while(|c| *c == '-' || c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if n.starts_with("--") && !v.contains(&n) {
            v.push(n);
        }
    }
    v
}

/// Rewrite a jellyfin-web stylesheet for the engine. A sheet with nothing Jellyfin-specific in it (a NOCTURNE sheet) comes back unchanged with `applied == false`.
pub fn translate(css: &str) -> (String, Compat) {
    let clean = strip_comments(css);
    let mut st = State {
        c: Compat::default(),
        defined: HashSet::new(),
    };
    let mut body = String::with_capacity(clean.len());
    walk(&clean, &mut body, &mut st, 0);
    let mut c = st.c;
    if c.hits == 0 {
        // Not a Jellyfin sheet: nothing was lost, so report nothing.
        return (
            css.to_owned(),
            Compat {
                applied: false,
                ..Compat::default()
            },
        );
    }
    c.applied = true;
    let mut pre = String::new();
    for n in referenced(&clean) {
        if st.defined.contains(&n) {
            continue;
        }
        if let Some((_, toks)) = map::VARS.iter().find(|(k, _)| *k == n) {
            pre += &format!("{n}: var({});", toks[0]);
            c.vars_defaulted += 1;
        }
    }
    let text = if pre.is_empty() {
        body
    } else {
        format!(":root{{{pre}}}\n{body}")
    };
    (text, c)
}
