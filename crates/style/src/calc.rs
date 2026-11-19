//! `calc()`, `min()`, `max()` and `clamp()`: parsed once into an expression tree, type-checked (numbers, lengths, percentages;
//! a percentage cannot be mixed with a length because the containing block is only known to the layout engine) and evaluated
//! when applied, against the element's font size and the window.
use crate::props::{Ctx, Issue, Len, len};
use crate::warn::Kind;
use std::rc::Rc;

#[derive(Debug, PartialEq)]
pub enum Expr {
    Num(f32),
    Atom(Len),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Min(Vec<Expr>),
    Max(Vec<Expr>),
    Clamp(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum K {
    Num,
    Px,
    Pct,
}

/// Deepest nesting and most tokens accepted.
const MAX_DEPTH: u8 = 12;
const MAX_TOKENS: usize = 200;

impl Expr {
    /// Pixels for a length expression, percent points for a percentage expression.
    pub fn eval(&self, font: f32, cx: Ctx) -> f32 {
        let v = match self {
            Expr::Num(n) => *n,
            Expr::Atom(Len::Pct(p)) => *p,
            Expr::Atom(l) => match l {
                Len::Px(p) => *p,
                Len::Em(e) => e * font,
                Len::Vw(v) => v * cx.vw / 100.0,
                Len::Vh(v) => v * cx.vh / 100.0,
                Len::Vmin(v) => v * cx.vw.min(cx.vh) / 100.0,
                Len::Vmax(v) => v * cx.vw.max(cx.vh) / 100.0,
                _ => 0.0,
            },
            Expr::Add(a, b) => a.eval(font, cx) + b.eval(font, cx),
            Expr::Sub(a, b) => a.eval(font, cx) - b.eval(font, cx),
            Expr::Mul(a, b) => a.eval(font, cx) * b.eval(font, cx),
            Expr::Div(a, b) => {
                let d = b.eval(font, cx);
                if d == 0.0 { 0.0 } else { a.eval(font, cx) / d }
            }
            Expr::Min(v) => v
                .iter()
                .map(|e| e.eval(font, cx))
                .fold(f32::INFINITY, f32::min),
            Expr::Max(v) => v
                .iter()
                .map(|e| e.eval(font, cx))
                .fold(f32::NEG_INFINITY, f32::max),
            Expr::Clamp(lo, v, hi) => {
                let (lo, v, hi) = (lo.eval(font, cx), v.eval(font, cx), hi.eval(font, cx));
                v.max(lo).min(hi.max(lo))
            }
        };
        if v.is_finite() { v } else { 0.0 }
    }

    pub fn uses_viewport(&self) -> bool {
        match self {
            Expr::Num(_) => false,
            Expr::Atom(l) => l.is_viewport(),
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                a.uses_viewport() || b.uses_viewport()
            }
            Expr::Min(v) | Expr::Max(v) => v.iter().any(Expr::uses_viewport),
            Expr::Clamp(a, b, c) => a.uses_viewport() || b.uses_viewport() || c.uses_viewport(),
        }
    }

    fn kind(&self) -> Result<K, String> {
        let mixed = || "calc() cannot mix % with lengths here".to_string();
        Ok(match self {
            Expr::Num(_) => K::Num,
            Expr::Atom(Len::Pct(_)) => K::Pct,
            Expr::Atom(Len::Auto) => return Err("`auto` in calc()".into()),
            Expr::Atom(_) => K::Px,
            Expr::Add(a, b) | Expr::Sub(a, b) => {
                let (x, y) = (a.kind()?, b.kind()?);
                if x != y {
                    return Err(mixed());
                }
                x
            }
            Expr::Mul(a, b) => match (a.kind()?, b.kind()?) {
                (K::Num, k) | (k, K::Num) => k,
                _ => return Err("calc(): one side of `*` must be a number".into()),
            },
            Expr::Div(a, b) => match (a.kind()?, b.kind()?) {
                (k, K::Num) => k,
                _ => return Err("calc(): the right side of `/` must be a number".into()),
            },
            Expr::Min(v) | Expr::Max(v) => {
                let mut it = v.iter().map(Expr::kind);
                let first = it.next().ok_or("empty function")??;
                for k in it {
                    if k? != first {
                        return Err(mixed());
                    }
                }
                first
            }
            Expr::Clamp(a, b, c) => {
                let (x, y, z) = (a.kind()?, b.kind()?, c.kind()?);
                if x != y || y != z {
                    return Err(mixed());
                }
                x
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Val(String),
    Func(String),
    Open,
    Close,
    Comma,
    Op(char),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let (mut i, mut out) = (0, vec![]);
    while i < cs.len() {
        let c = cs[i];
        if out.len() > MAX_TOKENS {
            return Err("expression too long".into());
        }
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '*' | '/' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            '+' | '-' | '.' | '0'..='9' => {
                let prev_value = matches!(out.last(), Some(Tok::Val(_) | Tok::Close));
                let sign = matches!(c, '+' | '-');
                let starts_number = !sign
                    || cs
                        .get(i + 1)
                        .is_some_and(|n| n.is_ascii_digit() || *n == '.');
                if sign && prev_value {
                    // Binary operator (CSS requires spaces around it).
                    out.push(Tok::Op(c));
                    i += 1;
                } else if starts_number {
                    let s = i;
                    i += 1;
                    while i < cs.len()
                        && (cs[i].is_ascii_alphanumeric() || cs[i] == '.' || cs[i] == '%')
                    {
                        i += 1;
                    }
                    out.push(Tok::Val(cs[s..i].iter().collect()));
                } else {
                    return Err(format!("unexpected `{c}`"));
                }
            }
            c if c.is_alphabetic() || c == '-' => {
                let s = i;
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '-' || cs[i] == '_') {
                    i += 1;
                }
                let name: String = cs[s..i].iter().collect::<String>().to_ascii_lowercase();
                if cs.get(i) == Some(&'(') {
                    i += 1;
                    out.push(Tok::Func(name));
                } else {
                    // A bare keyword such as `pi` or `infinity` is not supported.
                    return Err(format!("`{name}` is not supported in calc()"));
                }
            }
            c => return Err(format!("unexpected `{c}`")),
        }
    }
    Ok(out)
}

struct P {
    t: Vec<Tok>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn sum(&mut self, d: u8) -> Result<Expr, String> {
        if d > MAX_DEPTH {
            return Err("expression nests too deeply".into());
        }
        let mut l = self.prod(d)?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.prod(d)?;
            l = if c == '+' {
                Expr::Add(Box::new(l), Box::new(r))
            } else {
                Expr::Sub(Box::new(l), Box::new(r))
            };
        }
        Ok(l)
    }

    fn prod(&mut self, d: u8) -> Result<Expr, String> {
        let mut l = self.atom(d)?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.atom(d)?;
            l = if c == '*' {
                Expr::Mul(Box::new(l), Box::new(r))
            } else {
                Expr::Div(Box::new(l), Box::new(r))
            };
        }
        Ok(l)
    }

    fn args(&mut self, d: u8) -> Result<Vec<Expr>, String> {
        let mut v = vec![self.sum(d + 1)?];
        while self.peek() == Some(&Tok::Comma) {
            self.i += 1;
            v.push(self.sum(d + 1)?);
        }
        if self.peek() != Some(&Tok::Close) {
            return Err("expected `)`".into());
        }
        self.i += 1;
        Ok(v)
    }

    fn atom(&mut self, d: u8) -> Result<Expr, String> {
        match self.t.get(self.i).cloned() {
            Some(Tok::Open) => {
                self.i += 1;
                let e = self.sum(d + 1)?;
                if self.peek() != Some(&Tok::Close) {
                    return Err("expected `)`".into());
                }
                self.i += 1;
                Ok(e)
            }
            Some(Tok::Func(f)) => {
                self.i += 1;
                match f.as_str() {
                    "calc" | "-webkit-calc" => {
                        let e = self.sum(d + 1)?;
                        if self.peek() != Some(&Tok::Close) {
                            return Err("expected `)`".into());
                        }
                        self.i += 1;
                        Ok(e)
                    }
                    "min" => Ok(Expr::Min(self.args(d)?)),
                    "max" => Ok(Expr::Max(self.args(d)?)),
                    "clamp" => {
                        let mut a = self.args(d)?;
                        if a.len() != 3 {
                            return Err("clamp() takes three values".into());
                        }
                        let hi = a.pop().unwrap_or(Expr::Num(0.0));
                        let v = a.pop().unwrap_or(Expr::Num(0.0));
                        let lo = a.pop().unwrap_or(Expr::Num(0.0));
                        Ok(Expr::Clamp(Box::new(lo), Box::new(v), Box::new(hi)))
                    }
                    f => Err(format!("`{f}()` is not supported")),
                }
            }
            Some(Tok::Val(v)) => {
                self.i += 1;
                match v.parse::<f32>() {
                    Ok(n) if n.is_finite() => Ok(Expr::Num(n)),
                    _ => match len(&v) {
                        Ok(l) => Ok(Expr::Atom(l)),
                        Err(e) => Err(e.msg),
                    },
                }
            }
            _ => Err("unexpected end of expression".into()),
        }
    }
}

/// Is this text a function-call expression we parse here?
pub fn is_expr(v: &str) -> bool {
    let v = v.trim_start();
    ["calc(", "min(", "max(", "clamp(", "-webkit-calc("]
        .iter()
        .any(|p| v.to_ascii_lowercase().starts_with(p))
}

/// `env(safe-area-inset-bottom, 0px)` is the fallback (a desktop window has no safe area), or `0px`.
pub fn strip_env(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("env(") {
        out.push_str(&rest[..i]);
        let args = &rest[i + 4..];
        let (mut d, mut end) = (1, None);
        for (k, c) in args.char_indices() {
            match c {
                '(' => d += 1,
                ')' => {
                    d -= 1;
                    if d == 0 {
                        end = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push_str(&rest[i..]);
            return out;
        };
        let fb = args[..end].split_once(',').map_or("0px", |(_, f)| f.trim());
        out.push_str(fb);
        rest = &args[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn parse(v: &str) -> Result<Len, Issue> {
    let bad = |m: String| {
        Err(Issue {
            kind: Kind::BadValue,
            msg: format!("`{v}`: {m}"),
        })
    };
    let toks = match lex(v) {
        Ok(t) => t,
        Err(e) => return bad(e),
    };
    let mut p = P { t: toks, i: 0 };
    let e = match p.sum(0) {
        Ok(e) => e,
        Err(e) => return bad(e),
    };
    if p.i != p.t.len() {
        return bad("unexpected text after the expression".into());
    }
    match e.kind() {
        Ok(K::Px) => Ok(Len::Expr(Rc::new(e), false)),
        Ok(K::Pct) => Ok(Len::Expr(Rc::new(e), true)),
        Ok(K::Num) => bad("a number is not a length".into()),
        Err(m) => bad(m),
    }
}
