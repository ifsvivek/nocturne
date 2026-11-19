//! Selectors: type, universal, `.class`, `#id`, `[attr]` (`=`, `~=`, `|=`, `^=`, `$=`, `*=`, `i` flag), the four combinators,
//! `:hover`, `:focus`, `:focus-visible`, `:focus-within`, `:active`, `:first/last/only-child`, `:nth-child`, `:nth-last-child`,
//! the `-of-type` family, `:empty`, `:root`, `:is()`, `:where()`, `:not()` (selector lists), `:has()` (relative selectors) and the
//! `::before` / `::after` pseudo-elements. Matching is bounded: every visited node counts against a step budget so a hostile
//! selector cannot hang the app (`MatchCx::exhausted` tells the cascade it was cut off).
use crate::doc::{Doc, Gen, Id};
use std::borrow::Cow;
use std::collections::HashMap;

pub const HOVER: u8 = 1;
pub const FOCUS: u8 = 2;
pub const ACTIVE: u8 = 4;
pub const FOCUS_VISIBLE: u8 = 8;
pub const FOCUS_WITHIN: u8 = 16;

/// Limits on what a selector may cost to parse and to match.
pub const MAX_LEN: usize = 300;
/// Compounds in one selector, counting those inside `:is()`, `:not()` and `:has()`.
pub const MAX_PARTS: usize = 8;
pub const MAX_NEST: u8 = 3;
pub const MAX_LIST: usize = 16;
/// Nodes one element may visit while being matched against all rules.
pub const STEPS_PER_ELEMENT: u32 = 6_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Comb {
    Descendant,
    Child,
    Next,
    Subsequent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AttrOp {
    Exists,
    Eq,
    Word,
    Dash,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Clone, Debug)]
struct AttrSel {
    name: String,
    op: AttrOp,
    value: String,
    nocase: bool,
}

/// A complex selector, left to right; the first combinator is unused (or the leading one of a `:has()` argument).
pub type Complex = Vec<(Comb, Compound)>;

#[derive(Clone, Debug)]
enum Pseudo {
    State(u8),
    FirstChild,
    LastChild,
    OnlyChild,
    Nth {
        a: i32,
        b: i32,
        last: bool,
        of_type: bool,
    },
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Empty,
    Root,
    Not(Vec<Complex>),
    Is(Vec<Complex>),
    /// Matches like `Is` but weighs nothing.
    Where(Vec<Complex>),
    Has(Vec<Complex>),
}

#[derive(Clone, Debug, Default)]
pub struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrSel>,
    pseudos: Vec<Pseudo>,
}

/// What a selector depends on beyond the element's own tag, classes and attributes: the style-sharing cache and the
/// incremental restyle need to know (a flag set means "do not share across these differences").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deps {
    /// `:nth-*`, `:*-of-type`: the exact position matters.
    pub nth: bool,
    /// `:first-child`, `:last-child`, `:only-child`, `:empty`.
    pub edge: bool,
    /// `+` or `~`: preceding siblings matter.
    pub sibling: bool,
    /// `:has()`: the subtree and following siblings matter.
    pub has: bool,
    /// `:hover`, `:focus`, `:active`, ...
    pub state: bool,
    /// A state pseudo-class inside `:has()`: any state change can restyle ancestors.
    pub state_in_has: bool,
}

impl Deps {
    pub fn merge(&mut self, o: Deps) {
        self.nth |= o.nth;
        self.edge |= o.edge;
        self.sibling |= o.sibling;
        self.has |= o.has;
        self.state |= o.state;
        self.state_in_has |= o.state_in_has;
    }
}

#[derive(Clone, Debug)]
pub struct Selector {
    parts: Complex,
    pub pseudo: Option<Gen>,
    spec: u32,
    pub deps: Deps,
}

pub enum Key {
    Id(String),
    Class(String),
    Tag(String),
    Any,
}

/// Matching context for one cascade pass: the tree, interaction flags, a step budget and the `:has()` memo.
pub struct MatchCx<'a> {
    pub doc: &'a Doc,
    pub flags: &'a [u8],
    steps: u32,
    /// Steps over all elements (the cascade checks the clock with it).
    pub total_steps: u64,
    pub exhausted: bool,
    has_memo: HashMap<(usize, Id), bool>,
}

impl<'a> MatchCx<'a> {
    pub fn new(doc: &'a Doc, flags: &'a [u8]) -> Self {
        Self {
            doc,
            flags,
            steps: 0,
            total_steps: 0,
            exhausted: false,
            has_memo: HashMap::new(),
        }
    }

    /// Start counting for a new element.
    pub fn begin_element(&mut self) {
        self.steps = 0;
    }

    fn step(&mut self) -> bool {
        self.steps += 1;
        self.total_steps += 1;
        if self.steps > STEPS_PER_ELEMENT {
            self.exhausted = true;
            return false;
        }
        true
    }

    fn over(&self) -> bool {
        self.steps > STEPS_PER_ELEMENT
    }
}

impl Selector {
    /// (ids, classes + attributes + pseudo-classes, tags) packed so that a plain `u32` compares like CSS specificity.
    pub fn specificity(&self) -> u32 {
        self.spec
    }

    /// The most selective key of the rightmost compound, used to index rules: an id, a class or a tag.
    pub fn key(&self) -> Key {
        let c = &self.parts.last().expect("never empty").1;
        if let Some(i) = &c.id {
            Key::Id(i.clone())
        } else if let Some(k) = c.classes.first() {
            Key::Class(k.clone())
        } else if let Some(t) = &c.tag {
            Key::Tag(t.clone())
        } else {
            Key::Any
        }
    }

    /// Does the (originating) element match? For `::before`/`::after` selectors this is the element that owns the pseudo-element.
    /// Attribute names and whether ids this selector looks at: only those can make two elements style differently.
    pub fn collect_inputs(&self, attrs: &mut std::collections::HashSet<String>, ids: &mut bool) {
        fn complex(c: &Complex, attrs: &mut std::collections::HashSet<String>, ids: &mut bool) {
            for (_, p) in c {
                compound(p, attrs, ids);
            }
        }
        fn compound(c: &Compound, attrs: &mut std::collections::HashSet<String>, ids: &mut bool) {
            *ids |= c.id.is_some();
            attrs.extend(c.attrs.iter().map(|a| a.name.clone()));
            for p in &c.pseudos {
                if let Pseudo::Not(l) | Pseudo::Is(l) | Pseudo::Where(l) | Pseudo::Has(l) = p {
                    l.iter().for_each(|x| complex(x, attrs, ids));
                }
            }
        }
        complex(&self.parts, attrs, ids);
    }

    pub fn matches(&self, cx: &mut MatchCx, n: Id) -> bool {
        match_complex(&self.parts, self.parts.len() - 1, cx, n)
    }
}

fn pack(a: u32, b: u32, c: u32) -> u32 {
    (a.min(255) << 16) | (b.min(255) << 8) | c.min(255)
}

fn unpack(s: u32) -> (u32, u32, u32) {
    (s >> 16, (s >> 8) & 255, s & 255)
}

fn max_spec(lists: &[Complex]) -> u32 {
    lists.iter().map(complex_spec).max().unwrap_or(0)
}

fn complex_spec(c: &Complex) -> u32 {
    let (mut a, mut b, mut t) = (0, 0, 0);
    for (_, p) in c {
        let s = p.spec();
        let (x, y, z) = unpack(s);
        a += x;
        b += y;
        t += z;
    }
    pack(a, b, t)
}

impl Compound {
    fn spec(&self) -> u32 {
        let (mut a, mut b, mut c) = (self.id.is_some() as u32, 0u32, self.tag.is_some() as u32);
        b += (self.classes.len() + self.attrs.len()) as u32;
        for p in &self.pseudos {
            match p {
                Pseudo::Is(l) | Pseudo::Not(l) | Pseudo::Has(l) => {
                    let (x, y, z) = unpack(max_spec(l));
                    a += x;
                    b += y;
                    c += z;
                }
                Pseudo::Where(_) => {}
                _ => b += 1,
            }
        }
        pack(a, b, c)
    }
}

fn anb(a: i32, b: i32, pos: i64) -> bool {
    let (a, b) = (a as i64, b as i64);
    if a == 0 {
        pos == b
    } else {
        let d = pos - b;
        d % a == 0 && d / a >= 0
    }
}

fn attr_ok(a: &AttrSel, val: &str) -> bool {
    let (v, w): (Cow<str>, Cow<str>) = if a.nocase {
        (
            Cow::Owned(val.to_ascii_lowercase()),
            Cow::Owned(a.value.to_ascii_lowercase()),
        )
    } else {
        (Cow::Borrowed(val), Cow::Borrowed(a.value.as_str()))
    };
    let (v, w) = (&*v, &*w);
    match a.op {
        AttrOp::Exists => true,
        AttrOp::Eq => v == w,
        AttrOp::Word => v.split_whitespace().any(|x| x == w),
        AttrOp::Dash => v == w || v.strip_prefix(w).is_some_and(|r| r.starts_with('-')),
        AttrOp::Prefix => !w.is_empty() && v.starts_with(w),
        AttrOp::Suffix => !w.is_empty() && v.ends_with(w),
        AttrOp::Contains => !w.is_empty() && v.contains(w),
    }
}

fn compound_matches(c: &Compound, cx: &mut MatchCx, n: Id) -> bool {
    if !cx.step() {
        return false;
    }
    let doc = cx.doc;
    let node = &doc.nodes[n];
    if c.tag.as_ref().is_some_and(|t| *t != node.tag) {
        return false;
    }
    if c.id.as_ref().is_some_and(|i| node.id.as_ref() != Some(i)) {
        return false;
    }
    if !c.classes.iter().all(|k| node.classes.contains(k)) {
        return false;
    }
    for a in &c.attrs {
        let Some((_, val)) = node.attrs.iter().find(|(k, _)| *k == a.name) else {
            return false;
        };
        if !attr_ok(a, val) {
            return false;
        }
    }
    for pseudo in &c.pseudos {
        let ok = match pseudo {
            Pseudo::State(bit) => cx.flags.get(n).is_some_and(|f| f & bit != 0),
            Pseudo::FirstChild => node.parent.is_some() && node.index == 0,
            Pseudo::LastChild => node
                .parent
                .is_some_and(|p| node.index as usize + 1 == doc.nodes[p].children.len()),
            Pseudo::OnlyChild => node
                .parent
                .is_some_and(|p| doc.nodes[p].children.len() == 1),
            Pseudo::Nth {
                a,
                b,
                last,
                of_type,
            } => match node.parent {
                None => false,
                Some(p) => {
                    let sib = &doc.nodes[p].children;
                    let pos = if *of_type {
                        let range = if *last {
                            &sib[node.index as usize..]
                        } else {
                            &sib[..=node.index as usize]
                        };
                        let mut k = 0i64;
                        for &s in range {
                            if !cx.step() {
                                return false;
                            }
                            if doc.nodes[s].tag == node.tag {
                                k += 1;
                            }
                        }
                        k
                    } else if *last {
                        (sib.len() - node.index as usize) as i64
                    } else {
                        node.index as i64 + 1
                    };
                    anb(*a, *b, pos)
                }
            },
            Pseudo::FirstOfType | Pseudo::LastOfType | Pseudo::OnlyOfType => match node.parent {
                None => false,
                Some(p) => {
                    let sib = &doc.nodes[p].children;
                    let (mut before, mut after) = (false, false);
                    for (k, &s) in sib.iter().enumerate() {
                        if !cx.step() {
                            return false;
                        }
                        if s != n && doc.nodes[s].tag == node.tag {
                            if k < node.index as usize {
                                before = true;
                            } else {
                                after = true;
                            }
                        }
                    }
                    match pseudo {
                        Pseudo::FirstOfType => !before,
                        Pseudo::LastOfType => !after,
                        _ => !before && !after,
                    }
                }
            },
            Pseudo::Empty => node.children.is_empty() && node.text.is_none(),
            Pseudo::Root => node.parent.is_none(),
            Pseudo::Is(l) | Pseudo::Where(l) => {
                l.iter().any(|s| match_complex(s, s.len() - 1, cx, n))
            }
            Pseudo::Not(l) => !l.iter().any(|s| match_complex(s, s.len() - 1, cx, n)),
            Pseudo::Has(l) => has_match(l, cx, n),
        };
        if !ok {
            return false;
        }
        if cx.over() {
            return false;
        }
    }
    true
}

fn match_complex(parts: &[(Comb, Compound)], i: usize, cx: &mut MatchCx, n: Id) -> bool {
    if !compound_matches(&parts[i].1, cx, n) {
        return false;
    }
    if i == 0 {
        return true;
    }
    let doc = cx.doc;
    let node = &doc.nodes[n];
    match parts[i].0 {
        Comb::Child => node
            .parent
            .is_some_and(|p| match_complex(parts, i - 1, cx, p)),
        Comb::Descendant => {
            let mut up = node.parent;
            while let Some(p) = up {
                if !cx.step() {
                    return false;
                }
                if match_complex(parts, i - 1, cx, p) {
                    return true;
                }
                if cx.over() {
                    return false;
                }
                up = doc.nodes[p].parent;
            }
            false
        }
        Comb::Next => doc
            .prev_sibling(n)
            .is_some_and(|s| match_complex(parts, i - 1, cx, s)),
        Comb::Subsequent => {
            let Some(p) = node.parent else { return false };
            for k in 0..node.index as usize {
                if !cx.step() {
                    return false;
                }
                let s = doc.nodes[p].children[k];
                if match_complex(parts, i - 1, cx, s) {
                    return true;
                }
            }
            false
        }
    }
}

/// `:has(<relative selector list>)`: any argument that reaches a matching node from `anchor` makes it true. Memoised per
/// (argument, anchor) for the whole pass.
fn has_match(list: &[Complex], cx: &mut MatchCx, anchor: Id) -> bool {
    for rel in list {
        let key = (rel.as_ptr() as usize, anchor);
        if let Some(&v) = cx.has_memo.get(&key) {
            if v {
                return true;
            }
            continue;
        }
        let v = has_one(rel, cx, anchor);
        // A cut-off evaluation is not a fact worth remembering.
        if !cx.over() {
            cx.has_memo.insert(key, v);
        }
        if v {
            return true;
        }
    }
    false
}

fn has_one(rel: &Complex, cx: &mut MatchCx, anchor: Id) -> bool {
    let doc = cx.doc;
    let mut frontier = vec![anchor];
    for (comb, comp) in rel {
        let mut next: Vec<Id> = Vec::new();
        for &n in &frontier {
            match comb {
                Comb::Child => {
                    for &c in &doc.nodes[n].children {
                        if compound_matches(comp, cx, c) {
                            next.push(c);
                        }
                    }
                }
                Comb::Descendant => {
                    let mut stack: Vec<Id> = doc.nodes[n].children.iter().rev().copied().collect();
                    while let Some(d) = stack.pop() {
                        if cx.over() {
                            return false;
                        }
                        if compound_matches(comp, cx, d) {
                            next.push(d);
                        }
                        stack.extend(doc.nodes[d].children.iter().rev());
                    }
                }
                Comb::Next => {
                    if let Some(s) = doc.next_sibling(n)
                        && compound_matches(comp, cx, s)
                    {
                        next.push(s);
                    }
                }
                Comb::Subsequent => {
                    let mut s = doc.next_sibling(n);
                    while let Some(x) = s {
                        if cx.over() {
                            return false;
                        }
                        if compound_matches(comp, cx, x) {
                            next.push(x);
                        }
                        s = doc.next_sibling(x);
                    }
                }
            }
            if cx.over() {
                return false;
            }
        }
        next.sort_unstable();
        next.dedup();
        if next.is_empty() {
            return false;
        }
        frontier = next;
    }
    true
}

// ---- parsing ----

struct Parser<'a> {
    cs: &'a [char],
    i: usize,
    compounds: usize,
    deps: Deps,
    in_has: bool,
}

type PResult<T> = Result<T, String>;

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || (c as u32) > 0x7f
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.cs.get(self.i).copied()
    }

    fn ident(&mut self) -> String {
        let s = self.i;
        while self.i < self.cs.len() && is_ident(self.cs[self.i]) {
            self.i += 1;
        }
        self.cs[s..self.i].iter().collect()
    }

    fn ws(&mut self) -> bool {
        let s = self.i;
        while self.i < self.cs.len() && self.cs[self.i].is_whitespace() {
            self.i += 1;
        }
        self.i > s
    }

    /// The text between a `(` already consumed and its matching `)`.
    fn paren_body(&mut self) -> PResult<String> {
        let (mut depth, s) = (1, self.i);
        while self.i < self.cs.len() {
            match self.cs[self.i] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let t: String = self.cs[s..self.i].iter().collect();
                        self.i += 1;
                        return Ok(t);
                    }
                }
                _ => {}
            }
            self.i += 1;
        }
        Err("unclosed `(`".into())
    }

    fn attr(&mut self) -> PResult<AttrSel> {
        let end = self.cs[self.i..]
            .iter()
            .position(|&x| x == ']')
            .ok_or("unclosed `[`")?;
        let body: String = self.cs[self.i + 1..self.i + end].iter().collect();
        self.i += end + 1;
        let body = body.trim();
        let ops = [
            ("~=", AttrOp::Word),
            ("|=", AttrOp::Dash),
            ("^=", AttrOp::Prefix),
            ("$=", AttrOp::Suffix),
            ("*=", AttrOp::Contains),
            ("=", AttrOp::Eq),
        ];
        let found = ops
            .into_iter()
            .find_map(|(t, op)| body.find(t).map(|p| (p, t.len(), op)));
        let (name, op, mut value) = match found {
            Some((p, l, op)) => (body[..p].trim(), op, body[p + l..].trim().to_owned()),
            None => (body, AttrOp::Exists, String::new()),
        };
        let mut nocase = false;
        // `[a="b" i]`: a trailing flag after the quoted value (or after a bare word).
        if let Some(stripped) = value
            .strip_suffix(" i")
            .or_else(|| value.strip_suffix(" I"))
        {
            nocase = true;
            value = stripped.trim_end().to_owned();
        }
        if name.is_empty() || !name.chars().all(|c| is_ident(c) || c == ':') {
            return Err(format!("`[{body}]` is not an attribute selector"));
        }
        Ok(AttrSel {
            name: name.to_owned(),
            op,
            value: value.trim_matches(['"', '\'']).to_owned(),
            nocase,
        })
    }

    fn count_compound(&mut self) -> PResult<()> {
        self.compounds += 1;
        if self.compounds > MAX_PARTS {
            return Err("selector is too complex".into());
        }
        Ok(())
    }

    /// One compound. With `top` set, a trailing `::before` / `::after` is accepted and returned.
    fn compound(&mut self, nest: u8, top: bool) -> PResult<(Compound, Option<Gen>)> {
        let mut c = Compound::default();
        let mut pgen = None;
        let start = self.i;
        self.count_compound()?;
        while let Some(ch) = self.peek() {
            if pgen.is_some() {
                break;
            }
            match ch {
                ch if ch.is_whitespace() || matches!(ch, '>' | '+' | '~' | ',' | ')') => break,
                '*' => self.i += 1,
                '.' => {
                    self.i += 1;
                    let k = self.ident();
                    if k.is_empty() {
                        return Err("expected a class name".into());
                    }
                    c.classes.push(k);
                }
                '#' => {
                    self.i += 1;
                    let k = self.ident();
                    if k.is_empty() {
                        return Err("expected an id".into());
                    }
                    c.id = Some(k);
                }
                '[' => {
                    let a = self.attr()?;
                    c.attrs.push(a);
                }
                ':' => {
                    self.i += 1;
                    let double = self.peek() == Some(':');
                    if double {
                        self.i += 1;
                    }
                    let name = self.ident().to_lowercase();
                    let arg = if self.peek() == Some('(') {
                        self.i += 1;
                        Some(self.paren_body()?)
                    } else {
                        None
                    };
                    if matches!(name.as_str(), "before" | "after") && arg.is_none() {
                        if !top || self.in_has {
                            return Err(
                                "a pseudo-element is only allowed at the end of a plain selector"
                                    .into(),
                            );
                        }
                        pgen = Some(if name == "before" {
                            Gen::Before
                        } else {
                            Gen::After
                        });
                        continue;
                    }
                    if double {
                        return Err(format!("`::{name}` is not supported"));
                    }
                    let p = self.pseudo(&name, arg, nest)?;
                    c.pseudos.push(p);
                }
                ch if is_ident(ch) => {
                    if self.i != start {
                        return Err("a type selector must come first".into());
                    }
                    c.tag = Some(self.ident().to_lowercase());
                }
                ch => return Err(format!("unexpected `{ch}`")),
            }
        }
        if self.i == start {
            return Err("expected a selector".into());
        }
        Ok((c, pgen))
    }

    fn pseudo(&mut self, name: &str, arg: Option<String>, nest: u8) -> PResult<Pseudo> {
        let state = |s: &mut Self, bit: u8| {
            s.deps.state = true;
            if s.in_has {
                s.deps.state_in_has = true;
            }
            Pseudo::State(bit)
        };
        Ok(match (name, arg) {
            ("hover", None) => state(self, HOVER),
            ("focus", None) => state(self, FOCUS),
            ("focus-visible", None) => state(self, FOCUS_VISIBLE),
            ("focus-within", None) => state(self, FOCUS_WITHIN),
            ("active", None) => state(self, ACTIVE),
            ("first-child", None) => {
                self.deps.edge = true;
                Pseudo::FirstChild
            }
            ("last-child", None) => {
                self.deps.edge = true;
                Pseudo::LastChild
            }
            ("only-child", None) => {
                self.deps.edge = true;
                Pseudo::OnlyChild
            }
            ("first-of-type", None) => {
                self.deps.nth = true;
                Pseudo::FirstOfType
            }
            ("last-of-type", None) => {
                self.deps.nth = true;
                Pseudo::LastOfType
            }
            ("only-of-type", None) => {
                self.deps.nth = true;
                Pseudo::OnlyOfType
            }
            ("empty", None) => {
                self.deps.edge = true;
                Pseudo::Empty
            }
            ("root", None) => Pseudo::Root,
            ("nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type", Some(a)) => {
                self.deps.nth = true;
                let (x, y) = nth(&a)?;
                Pseudo::Nth {
                    a: x,
                    b: y,
                    last: name.contains("last"),
                    of_type: name.ends_with("of-type"),
                }
            }
            ("is" | "where" | "matches" | "-webkit-any" | "not", Some(a)) => {
                if nest >= MAX_NEST {
                    return Err("selector lists nest too deeply".into());
                }
                let list = self.sub_list(&a, nest + 1, false)?;
                match name {
                    "not" => Pseudo::Not(list),
                    "where" => Pseudo::Where(list),
                    _ => Pseudo::Is(list),
                }
            }
            ("has", Some(a)) => {
                if self.in_has {
                    return Err("`:has()` cannot nest".into());
                }
                if nest >= MAX_NEST {
                    return Err("selector lists nest too deeply".into());
                }
                self.deps.has = true;
                let list = self.sub_list(&a, nest + 1, true)?;
                Pseudo::Has(list)
            }
            (n, _) => return Err(format!("`:{n}` is not supported")),
        })
    }

    /// Parse the text of a functional pseudo-class argument as a selector list (or relative selector list).
    fn sub_list(&mut self, text: &str, nest: u8, relative: bool) -> PResult<Vec<Complex>> {
        let cs: Vec<char> = text.chars().collect();
        let mut p = Parser {
            cs: &cs,
            i: 0,
            compounds: self.compounds,
            deps: Deps::default(),
            in_has: self.in_has || relative,
        };
        let mut out = vec![];
        loop {
            p.ws();
            if p.i >= cs.len() {
                return Err("empty selector list".into());
            }
            out.push(p.complex(nest, relative, false)?.0);
            if out.len() > MAX_LIST {
                return Err("selector list is too long".into());
            }
            p.ws();
            match p.peek() {
                Some(',') => p.i += 1,
                None => break,
                Some(c) => return Err(format!("unexpected `{c}`")),
            }
        }
        self.compounds = p.compounds;
        self.deps.merge(p.deps);
        Ok(out)
    }

    /// A complex selector up to `,`, `)` or the end. With `relative`, a leading combinator is allowed.
    fn complex(&mut self, nest: u8, relative: bool, top: bool) -> PResult<(Complex, Option<Gen>)> {
        let mut parts: Complex = vec![];
        let mut comb = Comb::Descendant;
        let mut pgen = None;
        loop {
            let ws = self.ws();
            match self.peek() {
                None | Some(',' | ')') => break,
                Some(c @ ('>' | '+' | '~')) => {
                    if parts.is_empty() && !relative {
                        return Err(format!("unexpected `{c}`"));
                    }
                    comb = match c {
                        '>' => Comb::Child,
                        '+' => Comb::Next,
                        _ => Comb::Subsequent,
                    };
                    if matches!(comb, Comb::Next | Comb::Subsequent) {
                        self.deps.sibling = true;
                    }
                    self.i += 1;
                    self.ws();
                }
                _ if !parts.is_empty() && ws => comb = Comb::Descendant,
                _ => {}
            }
            if matches!(self.peek(), None | Some(',' | ')')) {
                return Err("a combinator needs a selector after it".into());
            }
            let (c, g) = self.compound(nest, top)?;
            parts.push((comb, c));
            comb = Comb::Descendant;
            if g.is_some() {
                pgen = g;
                self.ws();
                if !matches!(self.peek(), None | Some(',' | ')')) {
                    return Err("nothing may follow a pseudo-element".into());
                }
                break;
            }
        }
        if parts.is_empty() {
            return Err("empty selector".into());
        }
        Ok((parts, pgen))
    }
}

fn nth(arg: &str) -> PResult<(i32, i32)> {
    let a: String = arg.split_whitespace().collect::<String>().to_lowercase();
    let bad = || format!("`:nth-child({arg})` is not supported");
    match a.as_str() {
        "odd" => return Ok((2, 1)),
        "even" => return Ok((2, 0)),
        _ => {}
    }
    if let Some((x, y)) = a.split_once('n') {
        let x = match x {
            "" | "+" => 1,
            "-" => -1,
            v => v.parse().map_err(|_| bad())?,
        };
        let y = if y.is_empty() {
            0
        } else {
            y.parse().map_err(|_| bad())?
        };
        Ok((x, y))
    } else {
        Ok((0, a.parse().map_err(|_| bad())?))
    }
}

pub fn parse(s: &str) -> Result<Selector, String> {
    if s.len() > MAX_LEN {
        return Err("selector too long".into());
    }
    let cs: Vec<char> = s.chars().collect();
    let mut p = Parser {
        cs: &cs,
        i: 0,
        compounds: 0,
        deps: Deps::default(),
        in_has: false,
    };
    p.ws();
    let (parts, pseudo) = p.complex(0, false, true)?;
    p.ws();
    if p.i < cs.len() {
        return Err(format!("unexpected `{}`", cs[p.i]));
    }
    let mut spec = complex_spec(&parts);
    if pseudo.is_some() {
        // A pseudo-element adds one type-level weight.
        let (a, b, c) = unpack(spec);
        spec = pack(a, b, c + 1);
    }
    Ok(Selector {
        parts,
        pseudo,
        spec,
        deps: p.deps,
    })
}
