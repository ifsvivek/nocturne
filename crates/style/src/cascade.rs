//! The cascade. Elements with the same matching context share one computed style (a style-sharing cache keyed on tag, classes,
//! attributes, interaction state, the parent's style group and whatever the sheet's selectors can see beyond that), so ten
//! thousand identical cards run the cascade once. A hover or focus change restyles only the elements it can reach.
//! Every pass has a time budget and every selector match a step budget: a hostile sheet ends in a [`Failure`], never a hang.
use crate::doc::{Doc, Gen, Id};
use crate::props::{Computed, Ctx, Op, Outcome};
use crate::select::{self, MatchCx};
use crate::sheet::{Decl, Rule, Sheet, USER, Val, parse_inline};
use crate::warn::{Kind, Report};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::time::Instant;

/// Generated `::before` / `::after` leaves allowed per screen.
pub const MAX_GENERATED: usize = 2_000;

/// Why a styling pass was abandoned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The time budget ran out.
    Overrun,
    /// A panic inside the engine (caught by the caller).
    Panic(String),
}

/// Wall-clock budget of one pass, checked every few hundred elements.
pub struct Budget {
    deadline: Option<Instant>,
    n: u32,
}

impl Budget {
    pub fn new(deadline: Option<Instant>) -> Self {
        Self { deadline, n: 0 }
    }

    pub fn unlimited() -> Self {
        Self::new(None)
    }

    pub fn exceeded(&mut self) -> bool {
        self.n += 1;
        match self.deadline {
            Some(d) if self.n % 64 == 0 => Instant::now() > d,
            _ => false,
        }
    }
}

/// Interaction state the app owns.
#[derive(Clone, Copy, Debug, Default)]
pub struct Interaction {
    pub hover: Option<Id>,
    pub focus: Option<Id>,
    pub active: Option<Id>,
    /// The focus came from the keyboard (so `:focus-visible` matches).
    pub focus_visible: bool,
}

/// One state byte per node.
pub fn flags(doc: &Doc, it: &Interaction) -> Vec<u8> {
    let mut f = vec![0u8; doc.len()];
    let mut up = it.hover.filter(|&n| n < doc.len());
    while let Some(n) = up {
        f[n] |= select::HOVER;
        up = doc.nodes[n].parent;
    }
    let mut up = it.active.filter(|&n| n < doc.len());
    while let Some(n) = up {
        f[n] |= select::ACTIVE;
        up = doc.nodes[n].parent;
    }
    if let Some(n) = it.focus.filter(|&n| n < doc.len()) {
        f[n] |= select::FOCUS;
        if it.focus_visible {
            f[n] |= select::FOCUS_VISIBLE;
        }
        let mut up = doc.nodes[n].parent;
        while let Some(p) = up {
            f[p] |= select::FOCUS_WITHIN;
            up = doc.nodes[p].parent;
        }
    }
    f
}

/// What every pass needs to know.
pub struct Cx<'a> {
    pub sheet: &'a Sheet,
    pub vp: (f32, f32),
    pub budget: Budget,
}

impl Cx<'_> {
    fn ctx(&self) -> Ctx {
        Ctx {
            vw: self.vp.0,
            vh: self.vp.1,
        }
    }
}

struct GenStyle {
    content: Rc<str>,
    style: Rc<Computed>,
}

struct GroupData {
    style: Rc<Computed>,
    pgen: [Option<Rc<GenStyle>>; 2],
}

/// The result of a restyle.
#[derive(Debug, Default)]
pub struct Restyle {
    /// Nodes whose computed style changed, with the style they had before.
    pub changed: Vec<(Id, Rc<Computed>)>,
    /// A generated element appeared or vanished: the caller must rebuild the scene.
    pub structural: bool,
}

#[derive(Default)]
pub struct Styler {
    pub styles: Vec<Rc<Computed>>,
    groups: Vec<u32>,
    data: Vec<GroupData>,
    cache: HashMap<(u64, u64), u32>,
    sib: Vec<u64>,
    sub: Vec<u64>,
    fs: Vec<u64>,
    prot: Vec<u8>,
    flags: Vec<u8>,
    inline: HashMap<String, Rc<[Decl]>>,
    dyn_memo: HashMap<(String, String), Rc<Result<Vec<Op>, (Kind, String)>>>,
    seen: HashSet<(u32, u32)>,
    cache_vp: (u32, u32),
    pub report: Report,
}

/// (important, origin, specificity, source order, index in rule)
type Key5 = (bool, u8, u32, u32, u32);

struct Hit<'a> {
    key: Key5,
    rule: Option<u32>,
    decl: &'a Decl,
    k: u32,
}

fn hash2<T: Hash>(v: &T) -> (u64, u64) {
    let mut a = DefaultHasher::new();
    v.hash(&mut a);
    let mut b = DefaultHasher::new();
    0xA5A5u16.hash(&mut b);
    v.hash(&mut b);
    (a.finish(), b.finish())
}

fn floor_violation(op: &Op) -> Option<&'static str> {
    match op {
        Op::Display(d) if *d == taffy::style::Display::None => Some("hide"),
        Op::Opacity(o) if *o < 0.4 => Some("make transparent"),
        Op::Visible(false) => Some("hide"),
        _ => None,
    }
}

impl Styler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn flags(&self) -> &[u8] {
        &self.flags
    }

    pub fn style(&self, i: Id) -> &Rc<Computed> {
        &self.styles[i]
    }

    /// Essential bookkeeping and the `:has()` signatures: both need a look at the whole tree.
    fn prepare(&mut self, doc: &Doc, sheet: &Sheet, flags: &[u8]) {
        let n = doc.len();
        self.prot.clear();
        self.prot.resize(n, 0);
        for i in (0..n).rev() {
            if doc.nodes[i].essential {
                self.prot[i] |= 1;
            }
            if self.prot[i] != 0
                && let Some(p) = doc.nodes[i].parent
            {
                self.prot[p] |= 2;
            }
        }
        if sheet.deps.has {
            self.sub.clear();
            self.sub.resize(n, 0);
            self.fs.clear();
            self.fs.resize(n, 0);
            for i in (0..n).rev() {
                let node = &doc.nodes[i];
                let mut h = DefaultHasher::new();
                (
                    &node.tag,
                    &node.id,
                    &node.classes,
                    &node.attrs,
                    flags.get(i),
                    node.text.is_some(),
                )
                    .hash(&mut h);
                for &c in &node.children {
                    self.sub[c].hash(&mut h);
                    self.fs[c].hash(&mut h);
                }
                self.sub[i] = h.finish();
            }
            for i in (0..n).rev() {
                self.fs[i] = match doc.next_sibling(i) {
                    Some(s) => {
                        let mut h = DefaultHasher::new();
                        (self.sub[s], self.fs[s]).hash(&mut h);
                        h.finish()
                    }
                    None => 0,
                };
            }
        }
    }

    fn share_key(&self, doc: &Doc, sheet: &Sheet, i: Id, flags: &[u8]) -> (u64, u64) {
        let node = &doc.nodes[i];
        let d = sheet.deps;
        let parent_group = node.parent.map_or(u32::MAX, |p| self.groups[p]);
        let siblings = node.parent.map_or(0, |p| doc.nodes[p].children.len());
        let structure = (
            if d.nth { node.index as i64 } else { -1 },
            if d.edge {
                (
                    node.index == 0,
                    node.index as usize + 1 == siblings,
                    siblings == 1,
                    node.children.is_empty() && node.text.is_none(),
                )
            } else {
                (false, false, false, false)
            },
            if d.sibling { self.sib[i] } else { 0 },
            if d.has { (self.sub[i], self.fs[i]) } else { (0, 0) },
        );
        let attrs: Vec<&(String, String)> = node
            .attrs
            .iter()
            .filter(|(k, _)| sheet.attr_names.contains(k))
            .collect();
        hash2(&(
            &node.tag,
            if sheet.uses_ids { node.id.as_ref() } else { None },
            &node.classes,
            attrs,
            &node.inline,
            flags.get(i).copied().unwrap_or(0),
            parent_group,
            self.prot[i],
            structure,
        ))
    }

    fn inline_decls(&mut self, text: &str) -> Rc<[Decl]> {
        if let Some(d) = self.inline.get(text) {
            return d.clone();
        }
        let d: Rc<[Decl]> = parse_inline(text).into();
        self.inline.insert(text.to_owned(), d.clone());
        d
    }

    fn gather<'a>(
        sheet: &'a Sheet,
        mcx: &mut MatchCx,
        doc: &Doc,
        i: Id,
        media: &[bool],
        pseudo: Option<Gen>,
        inline: Option<&'a Rc<[Decl]>>,
        out: &mut Vec<Hit<'a>>,
    ) {
        out.clear();
        mcx.begin_element();
        let node = &doc.nodes[i];
        let test = |ri: u32, out: &mut Vec<Hit<'a>>, mcx: &mut MatchCx| {
            let r: &'a Rule = &sheet.rules[ri as usize];
            if !media.get(r.media as usize).copied().unwrap_or(true) || r.sel.pseudo != pseudo {
                return;
            }
            if r.sel.matches(mcx, i) {
                for (k, d) in r.decls.iter().enumerate() {
                    out.push(Hit {
                        key: (d.important, r.origin, r.spec, r.order, k as u32),
                        rule: Some(ri),
                        decl: d,
                        k: k as u32,
                    });
                }
            }
        };
        if pseudo.is_some() {
            for &ri in &sheet.pseudo_rules {
                test(ri, out, mcx);
            }
        } else {
            if let Some(id) = &node.id {
                for &ri in sheet.id_rules(id) {
                    test(ri, out, mcx);
                }
            }
            for c in &node.classes {
                for &ri in sheet.class_rules(c) {
                    test(ri, out, mcx);
                }
            }
            for &ri in sheet.tag_rules(&node.tag) {
                test(ri, out, mcx);
            }
            for &ri in sheet.any_rules() {
                test(ri, out, mcx);
            }
            if let Some(ds) = inline {
                for (k, d) in ds.iter().enumerate() {
                    out.push(Hit {
                        key: (d.important, crate::sheet::INLINE, 0, 0, k as u32),
                        rule: None,
                        decl: d,
                        k: k as u32,
                    });
                }
            }
        }
        out.sort_unstable_by_key(|h| h.key);
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_hits(
        &mut self,
        c: &mut Computed,
        hits: &[Hit],
        sheet: &Sheet,
        cx: Ctx,
        floor: Option<&str>,
    ) {
        // Custom properties first, so any `var()` sees the final value whatever the declaration order.
        if hits.iter().any(|h| matches!(h.decl.val, Val::Custom(_))) {
            let mut vars = (*c.vars).clone();
            for h in hits {
                if let Val::Custom(v) = &h.decl.val {
                    vars.insert(h.decl.name.clone(), v.clone());
                }
            }
            c.vars = Rc::new(vars);
        }
        for h in hits {
            let ops: Rc<Result<Vec<Op>, (Kind, String)>>;
            let slice: &[Op] = match &h.decl.val {
                Val::Custom(_) => continue,
                Val::Ops(o) => o,
                Val::Dynamic(text) => {
                    let sub = match crate::props::substitute(text, &c.vars, 0) {
                        Ok(s) => s,
                        Err(e) => {
                            self.note(h, Kind::Variable, format!("{}: {e}", h.decl.name));
                            continue;
                        }
                    };
                    let key = (h.decl.name.clone(), sub);
                    ops = match self.dyn_memo.get(&key) {
                        Some(r) => r.clone(),
                        None => {
                            let r = Rc::new(match crate::props::compile(&key.0, &key.1) {
                                Ok(Outcome::Ops(o) | Outcome::Approx(o, _)) => Ok(o),
                                Ok(Outcome::NoEffect) => Ok(vec![]),
                                Err(i) => Err((i.kind, i.msg)),
                            });
                            if self.dyn_memo.len() < 4096 {
                                self.dyn_memo.insert(key, r.clone());
                            }
                            r
                        }
                    };
                    match &*ops {
                        Ok(o) => o,
                        Err((k, m)) => {
                            self.note(h, *k, m.clone());
                            continue;
                        }
                    }
                }
            };
            let user = h.key.1 >= USER;
            for op in slice {
                if let (Some(desc), true) = (floor, user)
                    && let Some(what) = floor_violation(op)
                {
                    let sel = h.rule.map_or("style attribute", |r| &*sheet.rules[r as usize].text);
                    self.report.add(
                        Kind::Floor,
                        format!("`{sel}` tried to {what} {desc}, an essential control; kept visible"),
                    );
                    continue;
                }
                op.apply(c, cx);
            }
        }
    }

    fn note(&mut self, h: &Hit, kind: Kind, msg: String) {
        // The same declaration reports once, however many elements it reaches.
        if let Some(r) = h.rule
            && !self.seen.insert((r, h.k))
        {
            return;
        }
        self.report.add(kind, msg);
    }

    fn compute(
        &mut self,
        cx: &mut Cx,
        mcx: &mut MatchCx,
        doc: &Doc,
        i: Id,
        media: &[bool],
    ) -> Result<GroupData, Failure> {
        let sheet = cx.sheet;
        let ctx = cx.ctx();
        let node = &doc.nodes[i];
        #[cfg(test)]
        if node.tag == "panic-test" {
            panic!("test panic");
        }
        let inline = node.inline.as_deref().map(|t| self.inline_decls(t));
        let mut hits = Vec::new();
        Self::gather(sheet, mcx, doc, i, media, None, inline.as_ref(), &mut hits);
        if mcx.exhausted {
            mcx.exhausted = false;
            self.report.add(
                Kind::Limit,
                format!("a selector was too expensive on `{}` and was cut off", doc.describe(i)),
            );
        }
        let mut c = match node.parent {
            Some(p) => Computed::child_of(&self.styles[p]),
            None => Computed::root(),
        };
        let floor = (self.prot[i] != 0).then(|| format!("`{}`", doc.describe(i)));
        self.apply_hits(&mut c, &hits, sheet, ctx, floor.as_deref());
        if c.outline_color.is_none()
            && let Some(v) = c.vars.get("--nocturne-focus")
        {
            c.outline_color = nocturne_core::customize::value::color(v).ok();
        }
        // Pseudo-elements inherit from the element's final style.
        let mut pgen: [Option<Rc<GenStyle>>; 2] = [None, None];
        if !sheet.pseudo_rules.is_empty() {
            for (slot, which) in [(0, Gen::Before), (1, Gen::After)] {
                Self::gather(sheet, mcx, doc, i, media, Some(which), None, &mut hits);
                if hits.is_empty() {
                    continue;
                }
                let mut g = Computed::child_of(&c);
                self.apply_hits(&mut g, &hits, sheet, ctx, None);
                if let Some(content) = g.content.clone()
                    && g.layout.display != taffy::style::Display::None
                {
                    pgen[slot] = Some(Rc::new(GenStyle {
                        content,
                        style: Rc::new(g),
                    }));
                }
            }
        }
        if cx.budget.exceeded() {
            return Err(Failure::Overrun);
        }
        Ok(GroupData {
            style: Rc::new(c),
            pgen,
        })
    }

    /// Style the nodes `from..doc.len()` (all real elements); returns the generated leaves to append, in order.
    fn pass(
        &mut self,
        cx: &mut Cx,
        doc: &Doc,
        from: Id,
        flags: &[u8],
    ) -> Result<Vec<(Id, Gen, Rc<GenStyle>)>, Failure> {
        let n = doc.len();
        let placeholder = self
            .styles
            .first()
            .cloned()
            .unwrap_or_else(|| Rc::new(Computed::root()));
        self.styles.resize(n, placeholder);
        self.groups.resize(n, u32::MAX);
        self.sib.resize(n, 0);
        let media = cx.sheet.media_active(cx.vp);
        let mut mcx = MatchCx::new(doc, flags);
        let mut generated = vec![];
        for i in from..n {
            if cx.budget.exceeded() {
                return Err(Failure::Overrun);
            }
            let (g, new) = self.group_of(cx, &mut mcx, doc, i, flags, &media)?;
            let _ = new;
            self.groups[i] = g;
            self.styles[i] = self.data[g as usize].style.clone();
            self.sib[i] = self.sib_hash(doc, i);
            for (slot, which) in [(0, Gen::Before), (1, Gen::After)] {
                if let Some(gs) = &self.data[g as usize].pgen[slot] {
                    generated.push((i, which, gs.clone()));
                }
            }
        }
        Ok(generated)
    }

    fn sib_hash(&self, doc: &Doc, i: Id) -> u64 {
        match doc.prev_sibling(i) {
            Some(p) => {
                let mut h = DefaultHasher::new();
                (self.groups[p], self.sib[p]).hash(&mut h);
                h.finish()
            }
            None => 0,
        }
    }

    fn group_of(
        &mut self,
        cx: &mut Cx,
        mcx: &mut MatchCx,
        doc: &Doc,
        i: Id,
        flags: &[u8],
        media: &[bool],
    ) -> Result<(u32, bool), Failure> {
        // `sib[i]` must be known before the key is built.
        self.sib[i] = self.sib_hash(doc, i);
        let key = self.share_key(doc, cx.sheet, i, flags);
        if let Some(&g) = self.cache.get(&key) {
            return Ok((g, false));
        }
        let data = self.compute(cx, mcx, doc, i, media)?;
        self.data.push(data);
        let g = (self.data.len() - 1) as u32;
        self.cache.insert(key, g);
        Ok((g, true))
    }

    fn append_generated(&mut self, doc: &mut Doc, generated: Vec<(Id, Gen, Rc<GenStyle>)>) {
        let mut have = doc.nodes.iter().filter(|n| n.generated.is_some()).count();
        for (origin, which, gs) in generated {
            if have >= MAX_GENERATED {
                self.report.add(
                    Kind::Limit,
                    format!("more than {MAX_GENERATED} generated elements: the rest was left out"),
                );
                break;
            }
            have += 1;
            let id = doc.nodes.len();
            let depth = doc.nodes[origin].depth + 1;
            doc.nodes.push(crate::doc::Node {
                tag: match which {
                    Gen::Before => "::before".into(),
                    Gen::After => "::after".into(),
                },
                text: Some(gs.content.to_string()),
                parent: Some(origin),
                depth,
                generated: Some(which),
                ..crate::doc::Node::default()
            });
            match which {
                Gen::Before => doc.nodes[origin].before = Some(id),
                Gen::After => doc.nodes[origin].after = Some(id),
            }
            self.styles.push(gs.style.clone());
            self.groups.push(u32::MAX);
            self.sib.push(0);
            self.prot.push(0);
        }
    }

    /// Style a whole tree (or the part from `from` on), appending generated leaves to `doc`.
    pub fn style_range(
        &mut self,
        doc: &mut Doc,
        cx: &mut Cx,
        flags: &[u8],
        from: Id,
    ) -> Result<(), Failure> {
        let vp = (cx.vp.0.to_bits(), cx.vp.1.to_bits());
        if cx.sheet.uses_viewport && vp != self.cache_vp {
            self.cache.clear();
            self.data.clear();
            self.dyn_memo.clear();
            // Groups pointed into `data`; every node is restyled below, so none of them is read first.
            self.groups.iter_mut().for_each(|g| *g = u32::MAX);
        }
        self.cache_vp = vp;
        if from == 0 {
            self.styles.clear();
            self.groups.clear();
            self.sib.clear();
        }
        self.prepare(doc, cx.sheet, flags);
        self.flags = flags.to_vec();
        let generated = self.pass(cx, doc, from, flags)?;
        self.append_generated(doc, generated);
        // Arrays cover generated leaves too.
        self.prot.resize(doc.len(), 0);
        Ok(())
    }

    /// New interaction state (or a window size, or edited classes): restyle what can have changed.
    pub fn restyle(
        &mut self,
        doc: &Doc,
        cx: &mut Cx,
        new_flags: &[u8],
        touched: &[Id],
    ) -> Result<Restyle, Failure> {
        let n = doc.len();
        let mut out = Restyle::default();
        let vp = (cx.vp.0.to_bits(), cx.vp.1.to_bits());
        let all = cx.sheet.deps.state_in_has || (cx.sheet.uses_viewport && vp != self.cache_vp);
        if cx.sheet.uses_viewport && vp != self.cache_vp {
            self.cache.clear();
            self.data.clear();
            self.dyn_memo.clear();
        }
        self.cache_vp = vp;
        let mut dirty = vec![all; n];
        let old = std::mem::take(&mut self.flags);
        for &t in touched {
            if t < n {
                dirty[t] = true;
            }
        }
        if !all {
            for i in 0..n {
                if old.get(i) != new_flags.get(i) {
                    dirty[i] = true;
                }
            }
            let d = cx.sheet.deps;
            for i in 0..n {
                let node = &doc.nodes[i];
                if let Some(p) = node.parent
                    && dirty[p]
                {
                    dirty[i] = true;
                }
                if d.sibling
                    && let Some(s) = doc.prev_sibling(i)
                    && dirty[s]
                {
                    dirty[i] = true;
                }
            }
        }
        self.flags = new_flags.to_vec();
        self.prepare(doc, cx.sheet, new_flags);
        let media = cx.sheet.media_active(cx.vp);
        let mut mcx = MatchCx::new(doc, new_flags);
        for i in 0..n {
            let node = &doc.nodes[i];
            if node.generated.is_some() {
                continue;
            }
            if !dirty[i] {
                self.sib[i] = self.sib_hash(doc, i);
                continue;
            }
            if cx.budget.exceeded() {
                return Err(Failure::Overrun);
            }
            let (g, _) = self.group_of(cx, &mut mcx, doc, i, new_flags, &media)?;
            self.groups[i] = g;
            let style = self.data[g as usize].style.clone();
            for (slot, own) in [(0, node.before), (1, node.after)] {
                let want = self.data[g as usize].pgen[slot].clone();
                match (&want, own) {
                    (Some(gs), Some(id)) => {
                        if self.styles[id] != gs.style {
                            let old = std::mem::replace(&mut self.styles[id], gs.style.clone());
                            out.changed.push((id, old));
                        }
                    }
                    (None, None) => {}
                    _ => out.structural = true,
                }
            }
            if self.styles[i] != style {
                let old = std::mem::replace(&mut self.styles[i], style);
                out.changed.push((i, old));
            }
        }
        Ok(out)
    }

    /// Forget generated and virtual nodes past `keep` (the caller truncated the doc).
    pub fn truncate(&mut self, keep: usize) {
        self.styles.truncate(keep);
        self.groups.truncate(keep);
        self.sib.truncate(keep);
        self.prot.truncate(keep);
    }
}
