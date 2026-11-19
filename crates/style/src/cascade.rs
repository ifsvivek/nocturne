//! The cascade: a parsed stylesheet, an index of its rules by the rightmost selector key, and one forward pass over the tree
//! that matches, orders (`!important`, specificity, source order), resolves custom properties and applies declarations.
use crate::doc::Doc;
use crate::props::{self, Computed, Ctx};
use crate::select::{self, Key, Selector};
use std::collections::HashMap;
use std::rc::Rc;

/// Inline (`style=""`) beats every selector.
const INLINE: u32 = 1 << 24;

struct Decl {
    name: String,
    value: String,
    important: bool,
}

struct Rule {
    sel: Selector,
    spec: u32,
    order: u32,
    decls: Rc<[Decl]>,
}

#[derive(Default)]
pub struct Sheet {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<u32>>,
    by_class: HashMap<String, Vec<u32>>,
    by_tag: HashMap<String, Vec<u32>>,
    any: Vec<u32>,
    /// Selectors or sections that were skipped, with the reason.
    pub warnings: Vec<String>,
}

fn decls(css: &str) -> Vec<Decl> {
    // The inline attribute and rule bodies go through the same reader as stylesheets.
    match nocturne_core::customize::read_rules(&format!("x{{{css}}}")) {
        Ok((rules, _)) => rules
            .into_iter()
            .flat_map(|r| r.decls)
            .map(|(name, value, important)| Decl {
                name,
                value,
                important,
            })
            .collect(),
        Err(_) => vec![],
    }
}

impl Sheet {
    pub fn parse(css: &str) -> Self {
        let mut s = Sheet::default();
        let (rules, skipped) = match nocturne_core::customize::read_rules(css) {
            Ok(r) => r,
            Err(e) => {
                s.warnings.push(e.to_string());
                return s;
            }
        };
        if skipped > 0 {
            s.warnings
                .push(format!("{skipped} problems while reading the stylesheet"));
        }
        for r in rules {
            let ds: Rc<[Decl]> = r
                .decls
                .into_iter()
                .map(|(name, value, important)| Decl {
                    name,
                    value,
                    important,
                })
                .collect();
            for text in r.selectors {
                match select::parse(&text) {
                    Ok(sel) => {
                        let i = s.rules.len() as u32;
                        match sel.key() {
                            Key::Id(k) => s.by_id.entry(k).or_default().push(i),
                            Key::Class(k) => s.by_class.entry(k).or_default().push(i),
                            Key::Tag(k) => s.by_tag.entry(k).or_default().push(i),
                            Key::Any => s.any.push(i),
                        }
                        s.rules.push(Rule {
                            spec: sel.specificity(),
                            sel,
                            order: i,
                            decls: ds.clone(),
                        });
                    }
                    Err(e) => s.warnings.push(format!("`{text}`: {e}")),
                }
            }
        }
        s
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// (important, specificity, source order, index in rule, declaration)
type Hit<'a> = (bool, u32, u32, u32, &'a Decl);

fn collect<'a>(
    sheet: &'a Sheet,
    list: &[u32],
    doc: &Doc,
    i: usize,
    flags: &[u8],
    hits: &mut Vec<Hit<'a>>,
) {
    for &ri in list {
        let r = &sheet.rules[ri as usize];
        if r.sel.matches(doc, i, flags) {
            for (k, d) in r.decls.iter().enumerate() {
                hits.push((d.important, r.spec, r.order, k as u32, d));
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct Report {
    pub unsupported: usize,
    pub messages: Vec<String>,
}

/// Style every node. `flags` is one `HOVER | FOCUS | ACTIVE` byte per node (the app owns interaction state; see `hover_flags`).
pub fn style_tree(
    doc: &Doc,
    sheet: &Sheet,
    flags: &[u8],
    cx: Ctx,
    base: &Computed,
    report: &mut Report,
) -> Vec<Computed> {
    let mut out: Vec<Computed> = Vec::with_capacity(doc.len());
    // (important, specificity, order, index in rule) -> declaration; reused for every node.
    // Many cards share the same inline text (or a few variants): parse each distinct string once.
    let mut parsed: HashMap<&str, Rc<Vec<Decl>>> = HashMap::new();
    let inline_store: Vec<Option<Rc<Vec<Decl>>>> = doc
        .nodes
        .iter()
        .map(|n| {
            let t = n.inline.as_deref()?;
            Some(parsed.entry(t).or_insert_with(|| Rc::new(decls(t))).clone())
        })
        .collect();
    let mut hits: Vec<Hit> = Vec::new();
    for (i, node) in doc.nodes.iter().enumerate() {
        hits.clear();
        let mut cand = |list: &[u32]| collect(sheet, list, doc, i, flags, &mut hits);
        if let Some(id) = &node.id
            && let Some(l) = sheet.by_id.get(id)
        {
            cand(l);
        }
        for c in &node.classes {
            if let Some(l) = sheet.by_class.get(c) {
                cand(l);
            }
        }
        if let Some(l) = sheet.by_tag.get(&node.tag) {
            cand(l);
        }
        cand(&sheet.any);
        if let Some(ds) = &inline_store[i] {
            for (k, d) in ds.iter().enumerate() {
                hits.push((d.important, INLINE, 0, k as u32, d));
            }
        }
        hits.sort_unstable_by_key(|h| (h.0, h.1, h.2, h.3));

        let mut c = match node.parent {
            Some(p) => Computed::child_of(&out[p]),
            None => base.clone(),
        };
        // Custom properties first, so any `var()` sees the final value whatever the declaration order.
        if hits.iter().any(|h| h.4.name.starts_with("--")) {
            let mut vars = (*c.vars).clone();
            for h in &hits {
                if h.4.name.starts_with("--") {
                    vars.insert(h.4.name.clone(), h.4.value.clone());
                }
            }
            c.vars = Rc::new(vars);
        }
        for h in &hits {
            let d = h.4;
            if d.name.starts_with("--") {
                continue;
            }
            let v = match props::substitute(&d.value, &c.vars, 0) {
                Ok(v) => v,
                Err(e) => {
                    note(report, format!("{}: {e}", d.name));
                    continue;
                }
            };
            if let Err(e) = props::apply(&mut c, &d.name, &v, cx) {
                note(report, e);
            }
        }
        out.push(c);
    }
    out
}

fn note(r: &mut Report, m: String) {
    r.unsupported += 1;
    if r.messages.len() < 20 && !r.messages.contains(&m) {
        r.messages.push(m);
    }
}

/// Mark the hovered node and its ancestors (`:hover` matches the chain), and the focused node.
pub fn flags(doc: &Doc, hover: Option<usize>, focus: Option<usize>) -> Vec<u8> {
    let mut f = vec![0u8; doc.len()];
    let mut up = hover;
    while let Some(n) = up {
        f[n] |= select::HOVER;
        up = doc.nodes[n].parent;
    }
    if let Some(n) = focus {
        f[n] |= select::FOCUS;
    }
    f
}
