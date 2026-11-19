//! The element inspector behind the Theme Studio: what an element is (tag, classes, id, data attributes, path), which rules matched it
//! and which declarations won, its computed style as editable values, and selector helpers ("this element only" and "all like
//! this"). Pure reads of a built [`Scene`]; nothing here changes a tree or a sheet.
use crate::doc::{Doc, Gen, Id};
use crate::layout::Scene;
use crate::select::{self, MatchCx};
use crate::sheet::{DEFAULT, INLINE, Sheet, USER};
use taffy::style::{CompactLength, Display};

/// What an element is.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub id: Id,
    pub tag: String,
    pub dom_id: Option<String>,
    pub classes: Vec<String>,
    /// `data-*` attributes (and any other attribute) as written by the builder.
    pub attrs: Vec<(String, String)>,
    /// `tag.class` of each ancestor from the root to this element.
    pub path: Vec<String>,
    /// The app protects this element (or something inside it) from being hidden or moved off screen.
    pub essential: bool,
    pub text: Option<String>,
    /// Window coordinates before clipping: x, y, width, height.
    pub rect: [f32; 4],
}

/// One declaration of a matched rule.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclInfo {
    pub name: String,
    pub important: bool,
    /// It decides the value of its property; `false` means a stronger declaration overrides it.
    pub won: bool,
}

/// A rule that matches the element.
#[derive(Clone, Debug, PartialEq)]
pub struct Matched {
    pub selector: String,
    /// `default` (NOCTURNE's own sheet), `theme` or `app` (style set from data).
    pub source: &'static str,
    pub spec: (u32, u32, u32),
    pub decls: Vec<DeclInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inspection {
    pub info: Info,
    /// In cascade order, strongest last.
    pub rules: Vec<Matched>,
    pub computed: Computed,
}

/// The style of an element as values the Studio controls edit. `None` means "not a plain value" (a gradient, `auto`, a keyword).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Computed {
    pub color: Option<[u8; 4]>,
    pub background: Option<[u8; 4]>,
    pub border_color: Option<[u8; 4]>,
    pub border_width: f32,
    pub radius: f32,
    pub width: f32,
    pub height: f32,
    /// Vertical and horizontal.
    pub padding: (f32, f32),
    pub margin: (f32, f32),
    pub gap: f32,
    /// First outer shadow: x, y, blur, spread, colour.
    pub shadow: Option<(f32, f32, f32, f32, [u8; 4])>,
    pub opacity: f32,
    pub family: Option<String>,
    pub font_size: f32,
    pub weight: u16,
    pub letter_spacing: f32,
    pub line_height: f32,
    pub transform: &'static str,
    pub align: &'static str,
    pub display: String,
    pub direction: String,
    pub wrap: String,
    pub align_items: String,
    pub justify: String,
    pub order: i32,
    pub position: String,
    pub visible: bool,
    pub hidden: bool,
}

fn kebab(debug: &str) -> String {
    let mut out = String::new();
    for (i, ch) in debug.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn px(l: CompactLength) -> f32 {
    if l.tag() == CompactLength::LENGTH_TAG {
        l.value()
    } else {
        0.0
    }
}

fn rgba(c: nocturne_core::customize::value::Color) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}

fn solid(c: &crate::paint::Fill) -> Option<[u8; 4]> {
    match c {
        crate::paint::Fill::Solid(c) => Some(rgba(*c)),
        _ => None,
    }
}

fn node_label(doc: &Doc, n: Id) -> String {
    let node = &doc.nodes[n];
    let mut s = node.tag.clone();
    for c in &node.classes {
        s.push('.');
        s.push_str(c);
    }
    s
}

/// `tag.class` label of an element, for the pick-mode outline.
pub fn label(doc: &Doc, n: Id) -> String {
    node_label(doc, n)
}

fn holds_essential(doc: &Doc, n: Id) -> bool {
    doc.nodes[n].essential
        || doc.nodes[n]
            .children
            .iter()
            .any(|&c| holds_essential(doc, c))
}

pub fn info(scene: &Scene, id: Id) -> Info {
    let doc = &scene.doc;
    let node = &doc.nodes[id];
    let mut path = vec![];
    let mut up = Some(id);
    while let Some(n) = up {
        path.push(node_label(doc, n));
        up = doc.nodes[n].parent;
    }
    path.reverse();
    Info {
        id,
        tag: node.tag.clone(),
        dom_id: node.id.clone(),
        classes: node.classes.clone(),
        attrs: node.attrs.clone(),
        path,
        essential: holds_essential(doc, id),
        text: node.text.clone(),
        rect: scene.screen_rect(id),
    }
}

fn computed(scene: &Scene, id: Id) -> Computed {
    let c = scene.style(id);
    let l = &c.layout;
    let b = scene.borders.get(id).copied().unwrap_or_default();
    let r = scene.rects[id];
    let shadow = c
        .shadows
        .iter()
        .find(|s| !s.inset)
        .map(|s| (s.dx, s.dy, s.blur, s.spread, rgba(s.color)));
    Computed {
        color: Some(rgba(c.color)),
        background: c.bg.first().and_then(solid),
        border_color: Some(rgba(c.border_color[0])),
        border_width: b[0],
        radius: c.radius[0],
        width: r[2],
        height: r[3],
        padding: (px(l.padding.top.into_raw()), px(l.padding.left.into_raw())),
        margin: (px(l.margin.top.into_raw()), px(l.margin.left.into_raw())),
        gap: px(l.gap.width.into_raw()),
        shadow,
        opacity: c.opacity,
        family: c.family.first().cloned(),
        font_size: c.font_size,
        weight: c.weight,
        letter_spacing: c.letter_spacing,
        line_height: c.line_height.ratio(c.font_size),
        transform: match c.text_transform {
            crate::props::Transform::None => "none",
            crate::props::Transform::Upper => "uppercase",
            crate::props::Transform::Lower => "lowercase",
        },
        align: match c.align {
            crate::props::Align::Left => "left",
            crate::props::Align::Center => "center",
            crate::props::Align::Right => "right",
        },
        display: kebab(&format!("{:?}", l.display)),
        direction: kebab(&format!("{:?}", l.flex_direction)),
        wrap: kebab(&format!("{:?}", l.flex_wrap)),
        align_items: kebab(&format!("{:?}", l.align_items.keyword)),
        justify: kebab(&format!("{:?}", l.justify_content.keyword)),
        order: c.order,
        position: if c.fixed {
            "fixed".into()
        } else {
            kebab(&format!("{:?}", l.position))
        },
        visible: c.visible,
        hidden: l.display == Display::None,
    }
}

fn source(origin: u8) -> &'static str {
    match origin {
        DEFAULT => "default",
        USER => "theme",
        INLINE => "app",
        _ => "other",
    }
}

fn unpack(s: u32) -> (u32, u32, u32) {
    (s >> 16, (s >> 8) & 255, s & 255)
}

/// The rules of `sheet` that match element `id`, with the winner of every declared property marked.
pub fn matched(scene: &Scene, sheet: &Sheet, id: Id) -> Vec<Matched> {
    let doc = &scene.doc;
    let flags = scene.st.flags();
    let mut cx = MatchCx::new(doc, flags);
    let media = sheet.media_active(scene.viewport);
    let mut hits: Vec<(&crate::sheet::Rule, Vec<DeclInfo>)> = vec![];
    for r in &sheet.rules {
        if r.sel.pseudo.is_some() || !media.get(r.media as usize).copied().unwrap_or(true) {
            continue;
        }
        cx.begin_element();
        if r.sel.matches(&mut cx, id) {
            let decls = r
                .decls
                .iter()
                .map(|d| DeclInfo {
                    name: d.name.clone(),
                    important: d.important,
                    won: false,
                })
                .collect();
            hits.push((r, decls));
        }
    }
    hits.sort_by_key(|(r, _)| (r.origin, r.spec, r.order));
    // Winner per property: highest (important, origin, specificity, order).
    let mut best: std::collections::HashMap<&str, (usize, usize, (bool, u8, u32, u32))> =
        Default::default();
    for (ri, (r, ds)) in hits.iter().enumerate() {
        for (di, d) in ds.iter().enumerate() {
            let key = (d.important, r.origin, r.spec, r.order);
            let e = best.entry(d.name.as_str()).or_insert((ri, di, key));
            if key >= e.2 {
                *e = (ri, di, key);
            }
        }
    }
    let wins: Vec<(usize, usize)> = best.values().map(|&(a, b, _)| (a, b)).collect();
    hits.into_iter()
        .enumerate()
        .map(|(ri, (r, mut ds))| {
            for (di, d) in ds.iter_mut().enumerate() {
                d.won = wins.contains(&(ri, di));
            }
            Matched {
                selector: r.text.to_string(),
                source: source(r.origin),
                spec: unpack(r.spec),
                decls: ds,
            }
        })
        .collect()
}

/// Everything about one element. The generated `::before`/`::after` leaves are inspected as their owner.
pub fn inspect(scene: &Scene, id: Id) -> Inspection {
    let id = owner(&scene.doc, id);
    Inspection {
        info: info(scene, id),
        rules: matched(scene, &scene.sheet, id),
        computed: computed(scene, id),
    }
}

/// The element that owns a generated leaf (or the element itself).
pub fn owner(doc: &Doc, id: Id) -> Id {
    let n = &doc.nodes[id];
    match (n.generated, n.parent) {
        (Some(Gen::Before | Gen::After), Some(p)) => p,
        _ => id,
    }
}

/// Which way the pick mode's arrow keys move the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Parent,
    Child,
    Prev,
    Next,
}

/// The neighbour of `id` in the tree (real elements only).
pub fn step(doc: &Doc, id: Id, to: Step) -> Option<Id> {
    match to {
        Step::Parent => doc.nodes[id].parent,
        Step::Child => doc.nodes[id].children.first().copied(),
        Step::Prev => doc.prev_sibling(id),
        Step::Next => doc.next_sibling(id),
    }
}

// ---- selectors ----

fn compound(doc: &Doc, n: Id, nth: bool) -> String {
    let mut s = node_label(doc, n);
    if nth {
        s.push_str(&format!(":nth-child({})", doc.nodes[n].index + 1));
    }
    s
}

/// The real elements that `sel` matches (no state pseudo-classes in play).
pub fn matches_of(doc: &Doc, sel: &str) -> Vec<Id> {
    let Ok(s) = select::parse(sel) else {
        return vec![];
    };
    let flags = vec![0u8; doc.len()];
    let mut cx = MatchCx::new(doc, &flags);
    (0..doc.len())
        .filter(|&i| doc.nodes[i].generated.is_none() && !doc.nodes[i].virtual_proto)
        .filter(|&i| {
            cx.begin_element();
            s.matches(&mut cx, i)
        })
        .collect()
}

/// A selector that matches this element and nothing else: its `#id` when it has one, else the shortest chain of `>` steps from an
/// ancestor that is unique, else the full path with positions.
pub fn this_selector(doc: &Doc, id: Id) -> String {
    let id = owner(doc, id);
    if let Some(i) = &doc.nodes[id].id {
        let s = format!("#{i}");
        if matches_of(doc, &s) == [id] {
            return s;
        }
    }
    let mut chain = vec![];
    let mut up = Some(id);
    while let Some(n) = up {
        if doc.nodes[n].parent.is_some() {
            chain.push(n);
        }
        up = doc.nodes[n].parent;
    }
    chain.reverse();
    for nth in [false, true] {
        for len in 1..=chain.len() {
            let tail = &chain[chain.len() - len..];
            let s = tail
                .iter()
                .map(|&n| compound(doc, n, nth))
                .collect::<Vec<_>>()
                .join(" > ");
            if matches_of(doc, &s) == [id] {
                return s;
            }
        }
    }
    // The root itself.
    doc.nodes[id].tag.clone()
}

/// A way to say "elements like this one".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Like {
    pub selector: String,
    pub count: usize,
}

/// Class-based selectors for the elements of the same kind as `id`, shortest first. NOCTURNE's own component classes (the ones the
/// default sheet styles) come before modifiers and state classes. Every selector matches only elements with `id`'s tag.
pub fn like_selectors(doc: &Doc, sheet: &Sheet, id: Id) -> Vec<Like> {
    let id = owner(doc, id);
    let node = &doc.nodes[id];
    let known = |c: &str| !sheet.class_rules(c).is_empty();
    let mut cands: Vec<String> = vec![];
    let mut classes: Vec<&String> = node.classes.iter().collect();
    classes.sort_by_key(|c| !known(c));
    for c in &classes {
        cands.push(format!(".{c}"));
    }
    if node.classes.is_empty() {
        cands.push(node.tag.clone());
    } else {
        for c in classes.iter().take(2) {
            cands.push(format!("{}.{c}", node.tag));
        }
        for a in 0..classes.len() {
            for b in a + 1..classes.len() {
                cands.push(format!(".{}.{}", classes[a], classes[b]));
            }
        }
        // Qualified by the nearest ancestor that has a class: "the buttons inside the hero".
        let mut up = node.parent;
        while let Some(p) = up {
            if let Some(pc) = doc.nodes[p].classes.first() {
                cands.push(format!(".{pc} .{}", classes[0]));
                break;
            }
            up = doc.nodes[p].parent;
        }
    }
    let mut out: Vec<Like> = vec![];
    for s in cands {
        if out.iter().any(|l| l.selector == s) {
            continue;
        }
        let m = matches_of(doc, &s);
        if m.contains(&id) && m.iter().all(|&i| doc.nodes[i].tag == node.tag) {
            out.push(Like {
                selector: s,
                count: m.len(),
            });
        }
    }
    // Stable: shorter text first, but the order of the candidates (known classes first) breaks ties.
    let mut ranked: Vec<(usize, Like)> = out.into_iter().enumerate().collect();
    ranked.sort_by_key(|(i, l)| (l.selector.matches(['.', ' ']).count(), *i));
    ranked.into_iter().map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scene_with;
    use crate::{Interaction, Sheet};

    fn doc() -> Doc {
        let mut d = Doc::new("page");
        let hero = d.add(0, "section.hero");
        let acts = d.add(hero, "div.hero-actions");
        let a = d.add(acts, "a.button.primary");
        d.text(a, "Play");
        let b = d.add(acts, "a.button");
        d.text(b, "More");
        let c = d.add(acts, "a.button");
        d.text(c, "Share");
        d.essential(c);
        let row = d.add(0, "section.row");
        let t = d.add(row, "div.row-title");
        d.text(t, "Latest");
        d
    }

    fn scene(css: &str) -> Scene {
        let mut s = Sheet::parse_default("a { color: #fff } .button { padding: 4px 8px }");
        s.add(css, USER);
        scene_with(&doc(), s, (800.0, 600.0), Interaction::default())
    }

    #[test]
    fn describes_an_element() {
        let sc = scene("");
        let i = info(&sc, 3);
        assert_eq!(i.tag, "a");
        assert_eq!(i.classes, ["button", "primary"]);
        assert_eq!(
            i.path,
            [
                "page",
                "section.hero",
                "div.hero-actions",
                "a.button.primary"
            ]
        );
        assert!(!i.essential);
        assert!(
            info(&sc, 2).essential,
            "a parent of an essential element is protected too"
        );
    }

    #[test]
    fn rules_are_listed_with_winners() {
        let sc = scene(".button { color: red } .primary { padding: 1px }");
        let ins = inspect(&sc, 3);
        let by = |sel: &str| ins.rules.iter().find(|r| r.selector == sel).unwrap();
        assert_eq!(by("a").source, "default");
        assert!(
            !by("a").decls[0].won,
            "the theme's colour beats the tag rule"
        );
        assert_eq!(
            ins.rules.iter().filter(|r| r.selector == ".button").count(),
            2,
            "the default and the theme rule both match"
        );
        // `.primary` and the default `.button` both set padding; the user rule wins by origin.
        let pad_default = ins
            .rules
            .iter()
            .find(|r| r.source == "default" && r.selector == ".button")
            .unwrap();
        assert!(!pad_default.decls[0].won);
        let pad_user = ins.rules.iter().find(|r| r.selector == ".primary").unwrap();
        assert!(pad_user.decls[0].won);
        assert_eq!(ins.computed.padding, (1.0, 1.0));
    }

    #[test]
    fn important_beats_specificity() {
        let sc = scene(".button { color: red !important } a.button.primary { color: blue }");
        let ins = inspect(&sc, 3);
        let imp = ins
            .rules
            .iter()
            .find(|r| r.selector == ".button" && r.source == "theme")
            .unwrap();
        assert!(imp.decls[0].won);
        let strong = ins
            .rules
            .iter()
            .find(|r| r.selector == "a.button.primary")
            .unwrap();
        assert!(!strong.decls[0].won);
    }

    #[test]
    fn this_selector_is_unique() {
        let d = doc();
        for i in 1..d.len() {
            let s = this_selector(&d, i);
            assert_eq!(matches_of(&d, &s), [i], "{s}");
        }
        // Two identical buttons need a position.
        assert!(this_selector(&d, 4).contains("nth-child") || this_selector(&d, 4).contains(">"));
        let mut d = doc();
        d.set_id(3, "play");
        assert_eq!(this_selector(&d, 3), "#play");
    }

    #[test]
    fn like_selectors_prefer_component_classes() {
        let d = doc();
        let sheet = Sheet::parse_default(".button { color: red }");
        let l = like_selectors(&d, &sheet, 3);
        assert_eq!(l[0].selector, ".button");
        assert_eq!(l[0].count, 3);
        assert!(
            l.iter()
                .any(|x| x.selector == ".button.primary" && x.count == 1)
        );
        assert!(
            l.iter()
                .any(|x| x.selector == ".hero-actions .button" || x.selector == ".hero .button")
        );
        let t = like_selectors(&d, &sheet, 7);
        assert_eq!(t[0].selector, ".row-title");
    }

    #[test]
    fn like_selectors_never_mix_tags() {
        let mut d = Doc::new("page");
        d.add(0, "a.item");
        d.add(0, "div.item");
        let l = like_selectors(&d, &Sheet::default(), 1);
        assert!(l.iter().all(|x| x.selector != ".item"), "{l:?}");
        assert_eq!(l[0].selector, "a.item");
    }

    #[test]
    fn steps_walk_the_tree() {
        let d = doc();
        assert_eq!(step(&d, 3, Step::Parent), Some(2));
        assert_eq!(step(&d, 2, Step::Child), Some(3));
        assert_eq!(step(&d, 3, Step::Next), Some(4));
        assert_eq!(step(&d, 4, Step::Prev), Some(3));
        assert_eq!(step(&d, 3, Step::Prev), None);
    }
}
