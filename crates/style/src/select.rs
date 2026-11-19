//! Selectors: type, universal, `.class`, `#id`, `[attr]` (`=`, `~=`, `^=`, `$=`, `*=`), descendant, child (`>`), next (`+`) and
//! subsequent (`~`) sibling, `:hover`, `:focus`, `:focus-visible`, `:active`, `:first-child`, `:last-child`, `:only-child`,
//! `:nth-child(an+b | odd | even)`, `:not(<compound>)`, `:root`. Pseudo-elements and everything else are rejected with a message.
use crate::doc::{Doc, Id};

pub const HOVER: u8 = 1;
pub const FOCUS: u8 = 2;
pub const ACTIVE: u8 = 4;

const MAX_LEN: usize = 300;
const MAX_PARTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Comb {
    Descendant,
    Child,
    Next,
    Subsequent,
}

#[derive(Clone, Debug)]
enum AttrOp {
    Exists,
    Eq,
    Word,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Clone, Debug)]
enum Pseudo {
    State(u8),
    FirstChild,
    LastChild,
    OnlyChild,
    Nth(i32, i32),
    Not(Box<Compound>),
    Root,
}

#[derive(Clone, Debug, Default)]
pub struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, AttrOp, String)>,
    pseudos: Vec<Pseudo>,
}

#[derive(Clone, Debug)]
pub struct Selector {
    /// Left to right; the first combinator is unused.
    parts: Vec<(Comb, Compound)>,
}

impl Selector {
    /// (ids, classes + attributes + pseudo-classes, tags) packed so that a plain `u32` compares like CSS specificity.
    pub fn specificity(&self) -> u32 {
        let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
        for (_, p) in &self.parts {
            p.add(&mut a, &mut b, &mut c);
        }
        (a.min(255) << 16) | (b.min(255) << 8) | c.min(255)
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

    pub fn matches(&self, doc: &Doc, n: Id, flags: &[u8]) -> bool {
        match_at(&self.parts, self.parts.len() - 1, doc, n, flags)
    }
}

pub enum Key {
    Id(String),
    Class(String),
    Tag(String),
    Any,
}

impl Compound {
    fn add(&self, a: &mut u32, b: &mut u32, c: &mut u32) {
        *a += self.id.is_some() as u32;
        *b += (self.classes.len() + self.attrs.len()) as u32;
        *c += self.tag.is_some() as u32;
        for p in &self.pseudos {
            match p {
                Pseudo::Not(inner) => inner.add(a, b, c),
                _ => *b += 1,
            }
        }
    }

    fn matches(&self, doc: &Doc, n: Id, flags: &[u8]) -> bool {
        let node = &doc.nodes[n];
        if self.tag.as_ref().is_some_and(|t| *t != node.tag) {
            return false;
        }
        if self
            .id
            .as_ref()
            .is_some_and(|i| node.id.as_ref() != Some(i))
        {
            return false;
        }
        if !self.classes.iter().all(|c| node.classes.contains(c)) {
            return false;
        }
        for (k, op, v) in &self.attrs {
            let Some((_, val)) = node.attrs.iter().find(|(a, _)| a == k) else {
                return false;
            };
            let ok = match op {
                AttrOp::Exists => true,
                AttrOp::Eq => val == v,
                AttrOp::Word => val.split_whitespace().any(|w| w == v),
                AttrOp::Prefix => val.starts_with(v.as_str()),
                AttrOp::Suffix => val.ends_with(v.as_str()),
                AttrOp::Contains => val.contains(v.as_str()),
            };
            if !ok {
                return false;
            }
        }
        self.pseudos.iter().all(|p| match p {
            Pseudo::State(bit) => flags.get(n).is_some_and(|f| f & bit != 0),
            Pseudo::FirstChild => node.parent.is_some() && node.index == 0,
            Pseudo::LastChild => node
                .parent
                .is_some_and(|p| node.index as usize + 1 == doc.nodes[p].children.len()),
            Pseudo::OnlyChild => node
                .parent
                .is_some_and(|p| doc.nodes[p].children.len() == 1),
            Pseudo::Nth(a, b) => {
                let i = node.index as i32 + 1;
                if *a == 0 {
                    i == *b
                } else {
                    (i - b) % a == 0 && (i - b) / a >= 0
                }
            }
            Pseudo::Not(c) => !c.matches(doc, n, flags),
            Pseudo::Root => node.parent.is_none(),
        })
    }
}

fn match_at(parts: &[(Comb, Compound)], i: usize, doc: &Doc, n: Id, flags: &[u8]) -> bool {
    if !parts[i].1.matches(doc, n, flags) {
        return false;
    }
    if i == 0 {
        return true;
    }
    let node = &doc.nodes[n];
    match parts[i].0 {
        Comb::Child => node
            .parent
            .is_some_and(|p| match_at(parts, i - 1, doc, p, flags)),
        Comb::Descendant => {
            let mut up = node.parent;
            while let Some(p) = up {
                if match_at(parts, i - 1, doc, p, flags) {
                    return true;
                }
                up = doc.nodes[p].parent;
            }
            false
        }
        Comb::Next => {
            node.index > 0
                && node.parent.is_some_and(|p| {
                    match_at(
                        parts,
                        i - 1,
                        doc,
                        doc.nodes[p].children[node.index as usize - 1],
                        flags,
                    )
                })
        }
        Comb::Subsequent => node.parent.is_some_and(|p| {
            doc.nodes[p].children[..node.index as usize]
                .iter()
                .any(|&s| match_at(parts, i - 1, doc, s, flags))
        }),
    }
}

fn ident(cs: &[char], i: &mut usize) -> String {
    let s = *i;
    while *i < cs.len() && (cs[*i].is_alphanumeric() || cs[*i] == '-' || cs[*i] == '_') {
        *i += 1;
    }
    cs[s..*i].iter().collect()
}

fn skip_ws(cs: &[char], i: &mut usize) -> bool {
    let s = *i;
    while *i < cs.len() && cs[*i].is_whitespace() {
        *i += 1;
    }
    *i > s
}

fn nth(arg: &str) -> Result<(i32, i32), String> {
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

fn compound(cs: &[char], i: &mut usize) -> Result<Compound, String> {
    let mut c = Compound::default();
    let start = *i;
    while *i < cs.len() {
        match cs[*i] {
            ch if ch.is_whitespace() || matches!(ch, '>' | '+' | '~' | ',' | ')') => break,
            '*' => *i += 1,
            '.' => {
                *i += 1;
                c.classes.push(ident(cs, i));
            }
            '#' => {
                *i += 1;
                c.id = Some(ident(cs, i));
            }
            '[' => {
                let end = cs[*i..]
                    .iter()
                    .position(|&x| x == ']')
                    .ok_or("unclosed `[`")?;
                let body: String = cs[*i + 1..*i + end].iter().collect();
                *i += end + 1;
                let body = body.trim();
                let ops = [
                    ("~=", AttrOp::Word),
                    ("^=", AttrOp::Prefix),
                    ("$=", AttrOp::Suffix),
                    ("*=", AttrOp::Contains),
                    ("=", AttrOp::Eq),
                ];
                let found = ops
                    .into_iter()
                    .find_map(|(t, op)| body.find(t).map(|p| (p, t.len(), op)));
                match found {
                    Some((p, l, op)) => c.attrs.push((
                        body[..p].trim().to_owned(),
                        op,
                        body[p + l..].trim().trim_matches(['"', '\'']).to_owned(),
                    )),
                    None => c
                        .attrs
                        .push((body.to_owned(), AttrOp::Exists, String::new())),
                }
            }
            ':' => {
                *i += 1;
                if cs.get(*i) == Some(&':') {
                    return Err(
                        "pseudo-elements (`::before`, `::after`) are not supported yet".into(),
                    );
                }
                let name = ident(cs, i).to_lowercase();
                let arg = if cs.get(*i) == Some(&'(') {
                    let mut depth = 0;
                    let s = *i + 1;
                    while *i < cs.len() {
                        match cs[*i] {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        *i += 1;
                    }
                    if *i >= cs.len() {
                        return Err("unclosed `(`".into());
                    }
                    let a: String = cs[s..*i].iter().collect();
                    *i += 1;
                    Some(a)
                } else {
                    None
                };
                c.pseudos.push(match (name.as_str(), arg) {
                    ("hover", None) => Pseudo::State(HOVER),
                    ("focus" | "focus-visible" | "focus-within", None) => Pseudo::State(FOCUS),
                    ("active", None) => Pseudo::State(ACTIVE),
                    ("first-child", None) => Pseudo::FirstChild,
                    ("last-child", None) => Pseudo::LastChild,
                    ("only-child", None) => Pseudo::OnlyChild,
                    ("root", None) => Pseudo::Root,
                    ("nth-child", Some(a)) => {
                        let (x, y) = nth(&a)?;
                        Pseudo::Nth(x, y)
                    }
                    ("not", Some(a)) => {
                        let cs2: Vec<char> = a.trim().chars().collect();
                        let mut j = 0;
                        let inner = compound(&cs2, &mut j)?;
                        if j < cs2.len() {
                            return Err("`:not()` takes one compound selector".into());
                        }
                        Pseudo::Not(Box::new(inner))
                    }
                    (n, _) => return Err(format!("`:{n}` is not supported")),
                });
            }
            ch if ch.is_alphabetic() => {
                if *i != start {
                    return Err("a type selector must come first".into());
                }
                c.tag = Some(ident(cs, i));
            }
            ch => return Err(format!("unexpected `{ch}`")),
        }
    }
    if *i == start {
        return Err("expected a selector".into());
    }
    Ok(c)
}

pub fn parse(s: &str) -> Result<Selector, String> {
    if s.len() > MAX_LEN {
        return Err("selector too long".into());
    }
    let cs: Vec<char> = s.chars().collect();
    let (mut i, mut parts, mut comb) = (0, vec![], Comb::Descendant);
    loop {
        let ws = skip_ws(&cs, &mut i);
        if i >= cs.len() {
            break;
        }
        if matches!(cs[i], '>' | '+' | '~') && !parts.is_empty() {
            comb = match cs[i] {
                '>' => Comb::Child,
                '+' => Comb::Next,
                _ => Comb::Subsequent,
            };
            i += 1;
            skip_ws(&cs, &mut i);
        } else if !parts.is_empty() && ws {
            comb = Comb::Descendant;
        }
        let c = compound(&cs, &mut i)?;
        parts.push((comb, c));
        comb = Comb::Descendant;
        if parts.len() > MAX_PARTS {
            return Err("selector has too many parts".into());
        }
    }
    if parts.is_empty() {
        return Err("empty selector".into());
    }
    Ok(Selector { parts })
}
