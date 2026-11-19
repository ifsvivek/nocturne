//! A parsed stylesheet: rules with their selectors parsed and declarations compiled once, indexed by the rightmost selector key.
//! `@media` and `@supports` blocks are read (not skipped): `@supports` is decided while parsing (is the property or selector one
//! the engine supports), `@media` is kept as a condition on each rule and evaluated against the window when styling.
//! `@layer` blocks are flattened. Everything else is reported and skipped by the shared reader in `nocturne-core`.
use crate::props::{self, Op, Outcome};
use crate::select::{self, Deps, Key, Selector};
use crate::warn::{Kind, Report};
use nocturne_core::customize::{MAX_BYTES, read_rules};
use std::collections::HashMap;
use std::rc::Rc;

pub const MAX_RULES: usize = 5_000;
pub const MAX_DECLS: usize = 50_000;
const MAX_BLOCK_DEPTH: u8 = 4;

/// Origins order the cascade: the app's own default sheet loses to the user's.
pub const DEFAULT: u8 = 0;
pub const USER: u8 = 1;
/// `style=""` set by the app from data.
pub const INLINE: u8 = 2;

#[derive(Debug)]
pub enum Val {
    Ops(Vec<Op>),
    /// Contains `var()`: substituted and compiled when styling an element.
    Dynamic(String),
    /// A custom property (`--x`).
    Custom(String),
}

#[derive(Debug)]
pub struct Decl {
    pub name: String,
    pub important: bool,
    pub val: Val,
}

pub struct Rule {
    pub sel: Selector,
    pub spec: u32,
    pub order: u32,
    pub origin: u8,
    /// Index into the sheet's media conditions (0 = always).
    pub media: u32,
    pub decls: Rc<[Decl]>,
    /// The selector as written, for messages.
    pub text: Rc<str>,
}

// ---- media queries ----

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmp {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
}

#[derive(Clone, Debug, PartialEq)]
enum Feat {
    Width(Cmp, f32),
    Height(Cmp, f32),
    Landscape(bool),
    Const(bool),
}

#[derive(Clone, Debug, PartialEq)]
struct Query {
    negate: bool,
    type_ok: bool,
    feats: Vec<Feat>,
}

/// AND of OR lists (nested `@media` blocks add one list each).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    all_of: Vec<Vec<Query>>,
}

fn cmp(c: Cmp, a: f32, b: f32) -> bool {
    match c {
        Cmp::Ge => a >= b,
        Cmp::Le => a <= b,
        Cmp::Gt => a > b,
        Cmp::Lt => a < b,
        Cmp::Eq => (a - b).abs() < 0.5,
    }
}

impl Query {
    fn eval(&self, vp: (f32, f32)) -> bool {
        let ok = self.type_ok
            && self.feats.iter().all(|f| match f {
                Feat::Width(c, v) => cmp(*c, vp.0, *v),
                Feat::Height(c, v) => cmp(*c, vp.1, *v),
                Feat::Landscape(l) => (vp.0 >= vp.1) == *l,
                Feat::Const(b) => *b,
            });
        ok != self.negate
    }
}

impl Media {
    pub fn eval(&self, vp: (f32, f32)) -> bool {
        self.all_of
            .iter()
            .all(|alts| alts.iter().any(|q| q.eval(vp)))
    }
}

fn media_len(t: &str) -> Option<f32> {
    let t = t.trim();
    if let Some(v) = t.strip_suffix("px") {
        return v.trim().parse().ok();
    }
    if let Some(v) = t.strip_suffix("rem").or_else(|| t.strip_suffix("em")) {
        return v.trim().parse::<f32>().ok().map(|x| x * 16.0);
    }
    (t == "0").then_some(0.0)
}

/// The text inside one `( ... )` of a media query.
fn feature(body: &str) -> Result<Feat, String> {
    let b = body.trim().to_ascii_lowercase();
    if let Some((name, val)) = b.split_once(':') {
        let (name, val) = (name.trim(), val.trim());
        return Ok(match name {
            "min-width" => Feat::Width(Cmp::Ge, media_len(val).ok_or("bad length")?),
            "max-width" => Feat::Width(Cmp::Le, media_len(val).ok_or("bad length")?),
            "width" => Feat::Width(Cmp::Eq, media_len(val).ok_or("bad length")?),
            "min-height" => Feat::Height(Cmp::Ge, media_len(val).ok_or("bad length")?),
            "max-height" => Feat::Height(Cmp::Le, media_len(val).ok_or("bad length")?),
            "height" => Feat::Height(Cmp::Eq, media_len(val).ok_or("bad length")?),
            "orientation" => Feat::Landscape(match val {
                "landscape" => true,
                "portrait" => false,
                _ => return Err(format!("orientation: {val}")),
            }),
            // A desktop app with a mouse and a dark theme.
            "prefers-color-scheme" => Feat::Const(val == "dark"),
            "prefers-reduced-motion" => Feat::Const(val == "no-preference"),
            "prefers-contrast" => Feat::Const(val == "no-preference"),
            "hover" | "any-hover" => Feat::Const(val == "hover"),
            "pointer" | "any-pointer" => Feat::Const(val == "fine"),
            "display-mode" => Feat::Const(val == "standalone"),
            n => return Err(format!("media feature `{n}`")),
        });
    }
    // Range syntax: `width >= 600px`, `600px <= width <= 900px`.
    let toks: Vec<&str> = b.split_whitespace().collect();
    let op = |s: &str| match s {
        ">=" => Some(Cmp::Ge),
        "<=" => Some(Cmp::Le),
        ">" => Some(Cmp::Gt),
        "<" => Some(Cmp::Lt),
        "=" => Some(Cmp::Eq),
        _ => None,
    };
    let flip = |c: Cmp| match c {
        Cmp::Ge => Cmp::Le,
        Cmp::Le => Cmp::Ge,
        Cmp::Gt => Cmp::Lt,
        Cmp::Lt => Cmp::Gt,
        Cmp::Eq => Cmp::Eq,
    };
    match toks.as_slice() {
        [n @ ("width" | "height"), o, v] => {
            let (c, v) = (op(o).ok_or("range")?, media_len(v).ok_or("bad length")?);
            Ok(if *n == "width" {
                Feat::Width(c, v)
            } else {
                Feat::Height(c, v)
            })
        }
        [v, o, n @ ("width" | "height")] => {
            let (c, v) = (
                flip(op(o).ok_or("range")?),
                media_len(v).ok_or("bad length")?,
            );
            Ok(if *n == "width" {
                Feat::Width(c, v)
            } else {
                Feat::Height(c, v)
            })
        }
        _ => Err(format!("media feature `{b}`")),
    }
}

/// Split at top-level spaces (outside parentheses).
fn words(s: &str) -> Vec<&str> {
    nocturne_core::customize::value::split_top(s, None)
}

fn query(q: &str, unknown: &mut Vec<String>) -> Query {
    let mut out = Query {
        negate: false,
        type_ok: true,
        feats: vec![],
    };
    for w in words(q) {
        let lw = w.to_ascii_lowercase();
        match lw.as_str() {
            "not" => out.negate = true,
            "only" | "and" => {}
            "all" | "screen" => {}
            "print" | "speech" | "tty" | "tv" | "projection" | "handheld" | "braille"
            | "embossed" | "aural" => out.type_ok = false,
            _ if w.starts_with('(') && w.ends_with(')') => match feature(&w[1..w.len() - 1]) {
                Ok(f) => out.feats.push(f),
                Err(e) => {
                    unknown.push(e);
                    out.feats.push(Feat::Const(false));
                }
            },
            _ => {
                // A range written with spaces outside parentheses is not valid; treat anything else as unknown.
                unknown.push(w.to_owned());
                out.feats.push(Feat::Const(false));
            }
        }
    }
    out
}

fn media_alts(prelude: &str, unknown: &mut Vec<String>) -> Vec<Query> {
    nocturne_core::customize::value::split_top(prelude, Some(','))
        .into_iter()
        .map(|q| query(q, unknown))
        .collect()
}

// ---- @supports ----

fn supports_decl(name: &str, value: &str) -> bool {
    if name.starts_with("--") || value.contains("var(") {
        return true;
    }
    matches!(
        props::compile(&name.to_ascii_lowercase(), value),
        Ok(Outcome::Ops(_) | Outcome::NoEffect | Outcome::Approx(..))
    )
}

fn supports(text: &str, depth: u8) -> bool {
    let t = text.trim();
    if depth > 6 {
        return false;
    }
    if let Some(rest) = t.strip_prefix("not ").or_else(|| t.strip_prefix("not(")) {
        // `not(` lost its parenthesis in the prefix strip.
        let rest = if t.starts_with("not(") { &t[3..] } else { rest };
        return !supports(rest, depth + 1);
    }
    let ws = words(t);
    if ws.len() > 1 {
        // `a and b and c` / `a or b`.
        let (mut result, mut op) = (None::<bool>, None::<&str>);
        for w in ws {
            match w {
                "and" | "or" => op = Some(w),
                _ => {
                    let v = supports(w, depth + 1);
                    result = Some(match (result, op) {
                        (None, _) => v,
                        (Some(r), Some("or")) => r || v,
                        (Some(r), _) => r && v,
                    });
                }
            }
        }
        return result.unwrap_or(false);
    }
    if let Some(sel) = t
        .strip_prefix("selector(")
        .and_then(|x| x.strip_suffix(')'))
    {
        return select::parse(sel).is_ok();
    }
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        let inner = inner.trim();
        if inner.starts_with('(') || inner.starts_with("not ") || inner.starts_with("selector(") {
            return supports(inner, depth + 1);
        }
        return match inner.split_once(':') {
            Some((n, v)) => supports_decl(n.trim(), v.trim()),
            None => false,
        };
    }
    false
}

// ---- scanning a stylesheet for blocks ----

pub(crate) fn skip_string(b: &[u8], i: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            b'\n' => return j,
            _ => j += 1,
        }
    }
    b.len()
}

pub(crate) fn skip_comment(b: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < b.len() {
        if b[j] == b'*' && b[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    b.len()
}

/// First `{` or `;` at or after `i`, outside strings, comments and parentheses.
pub(crate) fn block_start(b: &[u8], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                i = skip_string(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = skip_comment(b, i);
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = (depth - 1).max(0),
            b'{' | b';' if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// Index of the `}` matching the `{` at `open`.
pub(crate) fn block_end(b: &[u8], open: usize) -> usize {
    let (mut depth, mut i) = (0i32, open);
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                i = skip_string(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = skip_comment(b, i);
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth <= 0 {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

#[derive(Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    by_id: HashMap<String, Vec<u32>>,
    by_class: HashMap<String, Vec<u32>>,
    by_tag: HashMap<String, Vec<u32>>,
    any: Vec<u32>,
    /// Rules for `::before` and `::after`, in source order.
    pub pseudo_rules: Vec<u32>,
    medias: Vec<Media>,
    /// Problems and notes from reading the sheet, grouped by kind.
    pub report: Report,
    /// What the selectors depend on (for style sharing and incremental restyle).
    pub deps: Deps,
    /// Any user-origin rule was read (a theme is in play: the essential floor checks sizes).
    pub has_user: bool,
    /// Attribute names any selector reads (elements that differ only in other attributes share a style).
    pub attr_names: std::collections::HashSet<String>,
    /// Does any selector use an `#id`?
    pub uses_ids: bool,
    /// Does the result depend on the window size (`@media`, `vw`/`vh`, `calc()` with them)?
    pub uses_viewport: bool,
    /// A pseudo-element rule whose selector has a state pseudo-class.
    pub pseudo_dynamic: bool,
    order: u32,
    decls: usize,
}

struct Build<'a> {
    sheet: &'a mut Sheet,
    origin: u8,
}

impl Sheet {
    /// A user stylesheet.
    pub fn parse(css: &str) -> Sheet {
        let mut s = Sheet::default();
        s.add(css, USER);
        s
    }

    /// The app's own default sheet (lowest priority).
    pub fn parse_default(css: &str) -> Sheet {
        let mut s = Sheet::default();
        s.add(css, DEFAULT);
        s
    }

    /// Several sheets in cascade order (later wins), as one.
    pub fn layered(layers: &[(&str, u8)]) -> Sheet {
        let mut s = Sheet::default();
        for (css, origin) in layers {
            s.add(css, *origin);
        }
        s
    }

    /// Add a stylesheet text with the given origin. Over the size limit nothing is read.
    pub fn add(&mut self, css: &str, origin: u8) {
        if css.len() > MAX_BYTES {
            self.report.add(
                Kind::Limit,
                format!(
                    "stylesheet is {} bytes; the limit is {MAX_BYTES}",
                    css.len()
                ),
            );
            return;
        }
        if self.medias.is_empty() {
            self.medias.push(Media::default());
        }
        let mut b = Build {
            sheet: self,
            origin,
        };
        b.scan(css, 0, 0);
        if origin >= USER && !self.rules.is_empty() {
            self.has_user = true;
        }
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Is the rule's media condition true for this window?
    pub fn media_active(&self, vp: (f32, f32)) -> Vec<bool> {
        self.medias.iter().map(|m| m.eval(vp)).collect()
    }

    pub fn id_rules(&self, id: &str) -> &[u32] {
        self.by_id.get(id).map_or(&[], Vec::as_slice)
    }

    pub fn class_rules(&self, c: &str) -> &[u32] {
        self.by_class.get(c).map_or(&[], Vec::as_slice)
    }

    pub fn tag_rules(&self, t: &str) -> &[u32] {
        self.by_tag.get(t).map_or(&[], Vec::as_slice)
    }

    pub fn any_rules(&self) -> &[u32] {
        &self.any
    }
}

impl Build<'_> {
    fn scan(&mut self, src: &str, media: u32, depth: u8) {
        let b = src.as_bytes();
        let (mut i, mut plain_from) = (0, 0);
        let mut depth_paren = 0i32;
        while i < b.len() {
            match b[i] {
                b'"' | b'\'' => {
                    i = skip_string(b, i);
                    continue;
                }
                b'/' if b.get(i + 1) == Some(&b'*') => {
                    i = skip_comment(b, i);
                    continue;
                }
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'{' => {
                    // A style rule: skip its body whole.
                    i = block_end(b, i) + 1;
                    continue;
                }
                b'(' => depth_paren += 1,
                b')' => depth_paren = (depth_paren - 1).max(0),
                b'@' if depth_paren == 0 => {
                    let name: String = src[i + 1..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .collect::<String>()
                        .to_ascii_lowercase();
                    let head = block_start(b, i);
                    let has_block = head < b.len() && b[head] == b'{';
                    let handled = match name.as_str() {
                        "media" | "supports" | "layer" if has_block => true,
                        "layer" => {
                            // `@layer a, b;` only orders layers: nothing to do, drop the statement.
                            self.flush(&src[plain_from..i], media);
                            plain_from = head + 1;
                            i = head + 1;
                            continue;
                        }
                        _ => false,
                    };
                    if !handled {
                        // Cut the at-rule out so the shared reader never sees it (it is reported here, once).
                        self.flush(&src[plain_from..i], media);
                        self.at_rule_note(&name, has_block);
                        i = if has_block {
                            block_end(b, head) + 1
                        } else {
                            head + 1
                        };
                        i = i.min(src.len());
                        plain_from = i;
                        continue;
                    }
                    self.flush(&src[plain_from..i], media);
                    let end = block_end(b, head);
                    let prelude = src[i + 1 + name.len()..head].trim();
                    let body = &src[(head + 1).min(src.len())..end.min(src.len())];
                    if depth >= MAX_BLOCK_DEPTH {
                        self.sheet
                            .report
                            .add(Kind::Limit, format!("`@{name}` blocks nest too deeply"));
                    } else {
                        match name.as_str() {
                            "media" => {
                                let m = self.media_id(media, prelude);
                                self.scan(body, m, depth + 1);
                            }
                            "supports" => {
                                if supports(prelude, 0) {
                                    self.scan(body, media, depth + 1);
                                }
                            }
                            _ => self.scan(body, media, depth + 1),
                        }
                    }
                    i = end + 1;
                    plain_from = i.min(src.len());
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        self.flush(&src[plain_from.min(src.len())..], media);
    }

    fn at_rule_note(&mut self, name: &str, _block: bool) {
        let (kind, msg) = match name {
            "charset" | "namespace" => return,
            "import" => (
                Kind::UnsupportedAtRule,
                "`@import` is not fetched here".to_string(),
            ),
            "font-face" => (
                Kind::UnsupportedAtRule,
                "`@font-face` is not loaded from a stylesheet (fonts are registered by the app)"
                    .into(),
            ),
            "keyframes" | "-webkit-keyframes" => (
                Kind::Planned,
                "`@keyframes` animations are planned but not drawn yet".into(),
            ),
            n => (Kind::UnsupportedAtRule, format!("`@{n}` is not supported")),
        };
        self.sheet.report.add(kind, msg);
    }

    fn media_id(&mut self, parent: u32, prelude: &str) -> u32 {
        let mut unknown = vec![];
        let alts = media_alts(prelude, &mut unknown);
        for u in unknown {
            self.sheet.report.add(
                Kind::UnsupportedAtRule,
                format!("@media: `{u}` is not supported (treated as false)"),
            );
        }
        let mut m = self.sheet.medias[parent as usize].clone();
        m.all_of.push(alts);
        self.sheet.uses_viewport = true;
        // Identical conditions share an id.
        if let Some(p) = self.sheet.medias.iter().position(|x| *x == m) {
            return p as u32;
        }
        self.sheet.medias.push(m);
        (self.sheet.medias.len() - 1) as u32
    }

    fn flush(&mut self, plain: &str, media: u32) {
        if plain.trim().is_empty() {
            return;
        }
        let (rules, problems) = match read_rules(plain) {
            Ok(r) => r,
            Err(e) => {
                self.sheet.report.add(Kind::Limit, e.to_string());
                return;
            }
        };
        if rules.len() >= MAX_RULES {
            self.sheet.report.add(
                Kind::Limit,
                format!("more than {MAX_RULES} rules: the rest was not read"),
            );
        }
        if problems > 0 {
            self.sheet.report.add_n(
                Kind::Syntax,
                problems,
                format!("{problems} problems while reading the stylesheet"),
            );
        }
        for r in rules {
            if self.sheet.rules.len() >= MAX_RULES || self.sheet.decls >= MAX_DECLS {
                self.sheet.report.add(
                    Kind::Limit,
                    format!("more than {MAX_RULES} rules or {MAX_DECLS} declarations: the rest was not read"),
                );
                return;
            }
            self.sheet.decls += r.decls.len();
            let decls: Rc<[Decl]> = r
                .decls
                .into_iter()
                .filter_map(|(n, v, imp)| self.compile(n, v, imp))
                .collect();
            for text in r.selectors {
                match select::parse(&text) {
                    Ok(sel) => self.add_rule(sel, text, decls.clone(), media),
                    Err(e) => self
                        .sheet
                        .report
                        .add(Kind::UnsupportedSelector, format!("`{text}`: {e}")),
                }
            }
        }
    }

    fn compile(&mut self, name: String, value: String, important: bool) -> Option<Decl> {
        let rep = &mut self.sheet.report;
        if name.starts_with("--") {
            if ["vw", "vh", "vmin", "vmax", "env("]
                .iter()
                .any(|u| value.contains(u))
            {
                self.sheet.uses_viewport = true;
            }
            return Some(Decl {
                name,
                important,
                val: Val::Custom(value),
            });
        }
        if value.contains("var(") {
            if ["vw", "vh", "vmin", "vmax", "env("]
                .iter()
                .any(|u| value.contains(u))
            {
                self.sheet.uses_viewport = true;
            }
            return Some(Decl {
                name,
                important,
                val: Val::Dynamic(value),
            });
        }
        match props::compile(&name, &value) {
            Ok(Outcome::Ops(ops)) => {
                if ops.iter().any(Op::uses_viewport) {
                    self.sheet.uses_viewport = true;
                }
                Some(Decl {
                    name,
                    important,
                    val: Val::Ops(ops),
                })
            }
            Ok(Outcome::Approx(ops, why)) => {
                rep.add(Kind::Approximated, why);
                Some(Decl {
                    name,
                    important,
                    val: Val::Ops(ops),
                })
            }
            Ok(Outcome::NoEffect) => {
                rep.add(Kind::NoEffect, name);
                None
            }
            Err(issue) => {
                rep.add(issue.kind, issue.msg);
                None
            }
        }
    }

    fn add_rule(&mut self, sel: Selector, text: String, decls: Rc<[Decl]>, media: u32) {
        let s = &mut *self.sheet;
        let idx = s.rules.len() as u32;
        s.deps.merge(sel.deps);
        sel.collect_inputs(&mut s.attr_names, &mut s.uses_ids);
        if sel.pseudo.is_some() {
            s.pseudo_rules.push(idx);
            if sel.deps.state {
                s.pseudo_dynamic = true;
            }
        } else {
            match sel.key() {
                Key::Id(k) => s.by_id.entry(k).or_default().push(idx),
                Key::Class(k) => s.by_class.entry(k).or_default().push(idx),
                Key::Tag(k) => s.by_tag.entry(k).or_default().push(idx),
                Key::Any => s.any.push(idx),
            }
        }
        s.order += 1;
        s.rules.push(Rule {
            spec: sel.specificity(),
            sel,
            order: s.order,
            origin: self.origin,
            media,
            decls,
            text: Rc::from(text),
        });
    }
}

/// Declarations of a `style=""` attribute: only the first rule of the text counts, so a stray `}` cannot inject a rule.
pub fn parse_inline(css: &str) -> Vec<Decl> {
    let Ok((rules, _)) = read_rules(&format!("x{{{css}}}")) else {
        return vec![];
    };
    let Some(r) = rules.into_iter().next() else {
        return vec![];
    };
    r.decls
        .into_iter()
        .filter_map(|(name, value, important)| {
            let val = if name.starts_with("--") {
                Val::Custom(value)
            } else if value.contains("var(") {
                Val::Dynamic(value)
            } else {
                match props::compile(&name, &value) {
                    Ok(Outcome::Ops(o) | Outcome::Approx(o, _)) => Val::Ops(o),
                    _ => return None,
                }
            };
            Some(Decl {
                name,
                important,
                val,
            })
        })
        .collect()
}
