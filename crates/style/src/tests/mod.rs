//! Engine tests: selectors and the cascade, layout goldens, hostile input, the essential floor, safe mode, the virtual
//! container, navigation, transitions and the expressibility of the built-in "Glass" theme's building blocks.
use crate::select::{self, HOVER, MatchCx};
use crate::*;
use std::rc::Rc;

mod cascade;
mod compat;
mod default_sheet;
mod engine;
mod floor;
mod hostile;
mod layout;
mod nav;
mod perf;
mod selectors;
mod spec;
mod virt;

#[test]
fn only_engine_sheets_are_layered_over_the_default_sheet() {
    for (name, css) in nocturne_core::customize::PRESETS {
        assert!(!is_engine_sheet(css), "{name} is a stage-1 theme");
    }
    for (name, css) in STARTERS {
        assert!(is_engine_sheet(css), "{name} uses the engine vocabulary");
    }
}

pub(crate) fn scene_with(doc: &Doc, sheet: Sheet, vp: (f32, f32), it: Interaction) -> Scene {
    Scene::build(
        doc.clone(),
        Rc::new(sheet),
        Rc::new(Fonts::new()),
        vp,
        it,
        None,
        &Builders::new(),
        &mut approx,
    )
    .expect("builds")
}

pub(crate) fn scene(doc: &Doc, css: &str, vp: (f32, f32)) -> Scene {
    scene_with(doc, Sheet::parse(css), vp, Interaction::default())
}

pub(crate) fn find(doc: &Doc, class: &str) -> Vec<Id> {
    (0..doc.len())
        .filter(|&i| doc.nodes[i].classes.iter().any(|c| c == class))
        .collect()
}

/// page > .a > span.c("hello"), page > .b[data-kind=movie]
pub(crate) fn small() -> (Doc, Id, Id, Id) {
    let mut d = Doc::new("page");
    let a = d.add(0, "div.a");
    let b = d.add(0, "div.b");
    let c = d.add(a, "span.c");
    d.text(c, "hello");
    d.attr(b, "data-kind", "movie");
    (d, a, b, c)
}

pub(crate) fn matches(d: &Doc, sel: &str, n: Id) -> bool {
    matches_with(d, sel, n, &vec![0u8; d.len()])
}

pub(crate) fn matches_with(d: &Doc, sel: &str, n: Id, flags: &[u8]) -> bool {
    let s = select::parse(sel).unwrap_or_else(|e| panic!("{sel}: {e}"));
    let mut cx = MatchCx::new(d, flags);
    s.matches(&mut cx, n)
}

pub(crate) fn hover_flags(d: &Doc, n: Id) -> Vec<u8> {
    let mut f = vec![0u8; d.len()];
    let mut up = Some(n);
    while let Some(x) = up {
        f[x] |= HOVER;
        up = d.nodes[x].parent;
    }
    f
}
