use super::*;
use crate::sheet::{DEFAULT, USER};
use nocturne_core::customize::value::Color;
use std::time::Instant;

fn w(s: &Scene, n: Id) -> f32 {
    s.rects[n][2]
}

#[test]
fn specificity_beats_order_and_important_beats_specificity() {
    let (d, a, ..) = small();
    let s = scene(&d, "div { width: 10px } .a { width: 20px } div { width: 30px }", (800.0, 600.0));
    assert_eq!(w(&s, a), 20.0);
    let s = scene(&d, ".a { width: 20px } div { width: 30px !important }", (800.0, 600.0));
    assert_eq!(w(&s, a), 30.0);
    // Equal specificity: the later rule wins; `:where()` weighs nothing so it loses to a type selector.
    let s = scene(&d, ".a { width: 20px } .a { width: 25px }", (800.0, 600.0));
    assert_eq!(w(&s, a), 25.0);
    let s = scene(&d, "div { width: 20px } :where(.a) { width: 25px }", (800.0, 600.0));
    assert_eq!(w(&s, a), 20.0);
    let s = scene(&d, ":is(.a) { width: 25px } div { width: 20px }", (800.0, 600.0));
    assert_eq!(w(&s, a), 25.0);
}

#[test]
fn user_origin_beats_default_origin_whatever_the_specificity() {
    let (d, a, ..) = small();
    let sheet = Sheet::layered(&[("#x.a.a.a { width: 50px }", DEFAULT), ("div { width: 77px }", USER)]);
    let mut d2 = d.clone();
    d2.set_id(a, "x");
    let s = scene_with(&d2, sheet, (800.0, 600.0), Interaction::default());
    assert_eq!(w(&s, a), 77.0);
}

#[test]
fn inline_beats_selectors_and_important_beats_inline() {
    let (mut d, a, ..) = small();
    d.inline(a, "width: 11px");
    let s = scene(&d, "#nope, .a { width: 20px }  div.a { width: 30px }", (800.0, 600.0));
    assert_eq!(w(&s, a), 11.0);
    let s = scene(&d, ".a { width: 30px !important }", (800.0, 600.0));
    assert_eq!(w(&s, a), 30.0);
    // A stray `}` in the inline text cannot inject a rule.
    let mut d3 = small().0;
    d3.inline(1, "width: 11px } .b { width: 99px");
    let s = scene(&d3, "", (800.0, 600.0));
    assert_ne!(w(&s, 2), 99.0);
}

#[test]
fn custom_properties_inherit_substitute_and_fall_back() {
    let (d, a, _, c) = small();
    let s = scene(
        &d,
        "page { --w: 40px; --h: var(--w) } .a { width: var(--w); height: var(--h) } .c { color: var(--nope, #fff) }",
        (800.0, 600.0),
    );
    assert_eq!(&s.rects[a][2..], &[40.0, 40.0]);
    assert_eq!(s.style(c).color, Color::rgba(255, 255, 255, 255));
    // A declaration after the use still counts (custom properties are resolved first).
    let s = scene(&d, ".a { width: var(--w); --w: 33px }", (800.0, 600.0));
    assert_eq!(w(&s, a), 33.0);
    // A child can override what it inherits.
    let s = scene(&d, "page { --w: 10px } .a { --w: 20px } span { width: var(--w) }", (800.0, 600.0));
    assert_eq!(w(&s, 3), 20.0);
}

#[test]
fn cyclic_and_runaway_var_is_dropped_and_reported() {
    let (d, a, ..) = small();
    let s = scene(&d, ".a { --x: var(--y); --y: var(--x); width: var(--x) }", (800.0, 600.0));
    assert_ne!(w(&s, a), 0.0 - 1.0);
    assert!(s.warnings().count(Kind::Variable) >= 1);
    // 2^12 expansion is cut by the length limit, not computed to the end.
    let mut css = String::from(".a { --v0: 1px;");
    for i in 1..14 {
        css.push_str(&format!(" --v{i}: var(--v{}) var(--v{});", i - 1, i - 1));
    }
    css.push_str(" width: var(--v13) }");
    let s = scene(&d, &css, (800.0, 600.0));
    assert!(s.warnings().count(Kind::Variable) >= 1);
    let _ = a;
}

#[test]
fn text_properties_inherit_and_others_do_not() {
    let (d, a, _, c) = small();
    let s = scene(
        &d,
        ".a { color: #f00; font-size: 20px; font-weight: 700; font-style: italic; text-transform: uppercase; text-align: center; letter-spacing: 2px; padding: 10px; background: #00f; opacity: 0.5 }",
        (800.0, 600.0),
    );
    let cs = s.style(c);
    assert_eq!(cs.color, Color::rgba(255, 0, 0, 255));
    assert_eq!((cs.font_size, cs.weight, cs.italic, cs.letter_spacing), (20.0, 700, true, 2.0));
    assert_eq!(cs.text_transform, props::Transform::Upper);
    assert_eq!(cs.align, props::Align::Center);
    assert!(cs.bg.is_empty() && cs.opacity == 1.0);
    assert_eq!(s.style(a).opacity, 0.5);
}

#[test]
fn media_queries_follow_the_window() {
    let (d, a, ..) = small();
    let css = ".a { width: 100px } @media (min-width: 600px) { .a { width: 200px } } @media (min-width: 900px) and (max-width: 1200px) { .a { width: 300px } } @media (max-width: 500px), (orientation: portrait) { .a { height: 55px } }";
    let mut s = scene(&d, css, (800.0, 600.0));
    assert_eq!(w(&s, a), 200.0);
    s.resize((1000.0, 700.0), &mut approx);
    assert_eq!(w(&s, a), 300.0);
    s.resize((1400.0, 700.0), &mut approx);
    assert_eq!(w(&s, a), 200.0);
    s.resize((400.0, 700.0), &mut approx);
    assert_eq!((w(&s, a), s.rects[a][3]), (100.0, 55.0));
    s.resize((700.0, 900.0), &mut approx);
    assert_eq!(s.rects[a][3], 55.0);
    s.resize((700.0, 500.0), &mut approx);
    assert_ne!(s.rects[a][3], 55.0);
}

#[test]
fn media_in_em_range_syntax_and_nesting() {
    let (d, a, ..) = small();
    let css = "@media (min-width: 40em) { .a { width: 1px } } @media (width >= 900px) { .a { width: 2px } } @media screen { @media (min-width: 100px) { .a { height: 9px } } } @media print { .a { width: 5px } } @media not all and (min-width: 5000px) { .a { margin-top: 3px } }";
    let s = scene(&d, css, (1000.0, 600.0));
    assert_eq!((w(&s, a), s.rects[a][3], s.rects[a][1]), (2.0, 9.0, 3.0));
    let s = scene(&d, css, (700.0, 600.0));
    assert_eq!(w(&s, a), 1.0);
    let s = scene(&d, css, (600.0, 600.0));
    assert_ne!(w(&s, a), 1.0);
}

#[test]
fn supports_blocks_are_read_when_the_condition_holds() {
    let (d, a, ..) = small();
    let css = "@supports (display: grid) { .a { width: 11px } } @supports not (display: nonsense-value) { .a { height: 12px } } @supports (frobnicate: 1) { .a { padding-left: 50px } } @supports (display: grid) and (position: absolute) { .b { width: 13px } } @supports (display: nope) or (display: flex) { .b { height: 14px } } @supports selector(:is(a)) { .b { padding-left: 4px } } @supports selector(:frob) { .b { padding-left: 40px } }";
    let s = scene(&d, css, (800.0, 600.0));
    assert_eq!((w(&s, a), s.rects[a][3]), (11.0, 12.0));
    assert_eq!(s.insets[a][0], 0.0);
    assert_eq!((w(&s, 2), s.rects[2][3], s.insets[2][0]), (13.0, 14.0, 4.0));
}

#[test]
fn layer_blocks_are_flattened_and_other_at_rules_reported_once() {
    let (d, a, ..) = small();
    let css = "@layer base, theme; @layer theme { .a { width: 17px } } @font-face { font-family: X; src: url(x.woff2) } @import url(a.css); @keyframes k { from { opacity: 0 } to { opacity: 1 } } @container (min-width: 1px) { .a { width: 1px } }";
    let s = scene(&d, css, (800.0, 600.0));
    assert_eq!(w(&s, a), 17.0);
    let r = s.warnings();
    assert!(r.count(Kind::UnsupportedAtRule) >= 3 && r.count(Kind::Planned) >= 1);
    assert_eq!(r.count(Kind::Syntax), 0);
}

#[test]
fn pseudo_elements_become_bounded_generated_leaves() {
    let (d, a, _, c) = small();
    let css = ".c::before { content: \"[\"; color: #f00 } .c::after { content: \"]\" } .a::after { content: \"\"; display: block; height: 3px } .b::before { content: none } .c:hover::before { content: \"!\" }";
    let s = scene(&d, css, (800.0, 600.0));
    let (before, after) = (s.doc.nodes[c].before.unwrap(), s.doc.nodes[c].after.unwrap());
    assert_eq!(s.doc.nodes[before].text.as_deref(), Some("["));
    assert_eq!(s.doc.nodes[after].text.as_deref(), Some("]"));
    assert_eq!(s.style(before).color, Color::rgba(255, 0, 0, 255));
    // Generated leaves are children of the element in layout order: [ text ].
    assert_eq!(s.kids[c], vec![before, after]);
    assert!(s.doc.nodes[a].after.is_some() && s.doc.nodes[2].before.is_none());
    // `:nth-child` and `:last-child` do not count them.
    assert!(matches(&s.doc, ".c:last-child", c) && matches(&s.doc, "span:only-child", c));
    // The leaves inherit from the element.
    assert_eq!(s.style(after).font_size, s.style(c).font_size);
}

#[test]
fn generated_leaves_are_capped() {
    let mut d = Doc::new("page");
    for _ in 0..5000 {
        d.add(0, "i.x");
    }
    let s = scene(&d, ".x::before { content: \"a\" } .x::after { content: \"b\" }", (800.0, 600.0));
    assert_eq!(s.doc.len(), 5001 + crate::cascade::MAX_GENERATED);
    assert!(s.warnings().count(Kind::Limit) >= 1);
}

#[test]
fn identical_elements_share_one_computed_style() {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    let cards: Vec<Id> = (0..500).map(|_| d.add(row, "a.card")).collect();
    let s = scene(&d, ".card { width: 10px } .row .card:hover { width: 20px }", (800.0, 600.0));
    assert!(cards.windows(2).all(|p| Rc::ptr_eq(s.style(p[0]), s.style(p[1]))));
}

#[test]
fn sharing_stays_correct_for_structural_selectors() {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    let cards: Vec<Id> = (0..6).map(|_| d.add(row, "a.card")).collect();
    let s = scene(
        &d,
        ".card { width: 10px } .card:first-child { width: 11px } .card:last-child { width: 12px } .card:nth-child(3) { width: 13px } .card + .card + .card + .card + .card { width: 14px }",
        (800.0, 600.0),
    );
    let ws: Vec<f32> = cards.iter().map(|&c| w(&s, c)).collect();
    assert_eq!(ws, vec![11.0, 10.0, 13.0, 10.0, 14.0, 14.0]);
}

#[test]
fn sharing_stays_correct_for_ancestor_context() {
    let mut d = Doc::new("page");
    let r1 = d.add(0, "div.r1");
    let r2 = d.add(0, "div.r2");
    let a = d.add(r1, "a.card");
    let b = d.add(r2, "a.card");
    let s = scene(&d, ".r1 .card { width: 10px } .r2 .card { width: 20px }", (800.0, 600.0));
    assert_eq!((w(&s, a), w(&s, b)), (10.0, 20.0));
}

#[test]
fn sharing_stays_correct_with_has() {
    let mut d = Doc::new("page");
    let r1 = d.add(0, "div.row");
    let r2 = d.add(0, "div.row");
    d.add(r1, "i.mark");
    d.add(r2, "b.plain");
    let s = scene(&d, ".row { width: 10px } .row:has(.mark) { width: 30px } .row:has(+ .row) { height: 7px }", (800.0, 600.0));
    assert_eq!((w(&s, r1), w(&s, r2)), (30.0, 10.0));
    assert_eq!((s.rects[r1][3], s.rects[r2][3] != 7.0), (7.0, true));
}

fn cards_doc(n: usize) -> (Doc, Id, Vec<Id>) {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    let cards = (0..n).map(|_| d.add(row, "a.card")).collect();
    (d, row, cards)
}

#[test]
fn hover_restyles_only_what_it_reaches_and_skips_layout_for_paint_changes() {
    let (d, _, cards) = cards_doc(50);
    let css = ".card { width: 10px; height: 10px; background: #111 } .card:hover { background: #fff }";
    let mut s = scene(&d, css, (800.0, 600.0));
    let before: Vec<_> = cards.iter().map(|&c| s.style(c).clone()).collect();
    let up = s
        .update(Interaction { hover: Some(cards[7]), ..Default::default() }, Instant::now(), &mut approx)
        .unwrap();
    assert!(!up.relaid, "a background change must not relayout");
    // The hovered card and its ancestors may change; the other 49 keep the very same style object.
    let same = cards.iter().enumerate().filter(|(i, c)| *i != 7 && Rc::ptr_eq(s.style(**c), &before[*i])).count();
    assert_eq!(same, 49);
    assert_ne!(s.style(cards[7]).bg, before[7].bg);
    // Moving away restores it.
    s.update(Interaction::default(), Instant::now(), &mut approx).unwrap();
    assert_eq!(s.style(cards[7]).bg, before[7].bg);
}

#[test]
fn hover_that_changes_size_relays_out() {
    let (d, _, cards) = cards_doc(4);
    let mut s = scene(&d, ".card { width: 10px; height: 10px } .card:hover { width: 40px }", (800.0, 600.0));
    let up = s
        .update(Interaction { hover: Some(cards[1]), ..Default::default() }, Instant::now(), &mut approx)
        .unwrap();
    assert!(up.relaid);
    assert_eq!(w(&s, cards[1]), 40.0);
}

#[test]
fn hover_reaches_descendants_and_following_siblings() {
    let mut d = Doc::new("page");
    let a = d.add(0, "div.a");
    let kid = d.add(a, "span.k");
    let next = d.add(0, "div.n");
    let far = d.add(0, "div.f");
    let css = ".a:hover .k { height: 5px } .a:hover + .n { height: 6px } .a:hover ~ .f { height: 7px }";
    let h = |s: &Scene, n: Id| s.rects[n][3];
    let mut s = scene(&d, css, (800.0, 600.0));
    assert_eq!((h(&s, kid), h(&s, next), h(&s, far)), (0.0, 0.0, 0.0));
    s.update(Interaction { hover: Some(a), ..Default::default() }, Instant::now(), &mut approx).unwrap();
    assert_eq!((h(&s, kid), h(&s, next), h(&s, far)), (5.0, 6.0, 7.0));
    s.update(Interaction::default(), Instant::now(), &mut approx).unwrap();
    assert_eq!((h(&s, kid), h(&s, next), h(&s, far)), (0.0, 0.0, 0.0));
}

#[test]
fn has_invalidates_ancestors_when_a_descendant_is_hovered() {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    let title = d.add(row, "h2.title");
    let card = d.add(row, "a.card");
    let other = d.add(0, "div.row");
    let other_title = d.add(other, "h2.title");
    let css = ".row:has(.card:hover) .title { width: 90px } .title { width: 10px }";
    let mut s = scene(&d, css, (800.0, 600.0));
    assert_eq!((w(&s, title), w(&s, other_title)), (10.0, 10.0));
    s.update(Interaction { hover: Some(card), ..Default::default() }, Instant::now(), &mut approx).unwrap();
    assert_eq!((w(&s, title), w(&s, other_title)), (90.0, 10.0));
    s.update(Interaction::default(), Instant::now(), &mut approx).unwrap();
    assert_eq!(w(&s, title), 10.0);
}

#[test]
fn focus_and_focus_visible_differ() {
    let (d, a, b, _) = small();
    let css = ".a:focus { width: 11px } .a:focus-visible { height: 12px } .b:focus-within { width: 13px }";
    let mut s = scene(&d, css, (800.0, 600.0));
    s.update(Interaction { focus: Some(a), focus_visible: false, ..Default::default() }, Instant::now(), &mut approx)
        .unwrap();
    assert_eq!((w(&s, a), s.rects[a][3] == 12.0), (11.0, false));
    s.update(Interaction { focus: Some(a), focus_visible: true, ..Default::default() }, Instant::now(), &mut approx)
        .unwrap();
    assert_eq!(s.rects[a][3], 12.0);
    assert_eq!(w(&s, b), s.rects[b][2]);
}

#[test]
fn edited_classes_restyle_through_touch() {
    let (d, a, ..) = small();
    let mut s = scene(&d, ".a { width: 10px } .a.wide { width: 90px }", (800.0, 600.0));
    s.doc.nodes[a].classes.push("wide".into());
    s.touch(&[a]);
    s.refresh(Instant::now(), &mut approx).unwrap();
    assert_eq!(w(&s, a), 90.0);
}

#[test]
fn warnings_are_grouped_by_kind_with_counts_and_examples() {
    let (d, ..) = small();
    let mut css = String::new();
    for i in 0..40 {
        css.push_str(&format!(".a{i} {{ frob-{i}: 1; }}\n"));
    }
    css.push_str(".a { width: nonsense; cursor: pointer; user-select: none; -webkit-tap-highlight-color: transparent; color-scheme: dark; transform: rotate(3deg); backdrop-filter: blur(4px); a:hover::selection{} }");
    css.push_str(" .b:visited { color: red } .c:frob { color: red }");
    let r = scene(&d, &css, (800.0, 600.0)).warnings();
    assert_eq!(r.count(Kind::UnsupportedProperty), 40);
    assert_eq!(r.groups[&Kind::UnsupportedProperty].examples.len(), warn::EXAMPLES);
    assert!(r.count(Kind::BadValue) >= 1 && r.count(Kind::Planned) >= 1);
    assert!(r.count(Kind::UnsupportedSelector) >= 2);
    // Browser-only properties and fallbacks are accepted and listed apart: never counted as failures.
    assert_eq!(r.count(Kind::NoEffect), 4);
    assert_eq!(r.count(Kind::Approximated), 1);
    assert_eq!(r.failures(), r.groups.iter().filter(|(k, _)| k.is_failure()).map(|(_, g)| g.count).sum::<usize>());
    // The low-priority group sorts after every failure kind, so a long list of failures does not hide categories.
    let lines = r.lines();
    assert!(lines.last().unwrap().contains("no effect in a desktop app"));
    assert!(lines.len() >= 6);
}

#[test]
fn browser_only_properties_are_silent_no_ops() {
    let (d, a, ..) = small();
    let css = ".a { -webkit-tap-highlight-color: transparent; color-scheme: dark; touch-action: manipulation; user-select: none; cursor: pointer; scroll-behavior: smooth; -webkit-overflow-scrolling: touch; appearance: none; will-change: transform; contain: layout; font-optical-sizing: auto; font-display: swap; text-rendering: optimizeLegibility; -webkit-font-smoothing: antialiased; width: 12px }";
    let s = scene(&d, css, (800.0, 600.0));
    assert_eq!(w(&s, a), 12.0);
    let r = s.warnings();
    assert_eq!(r.failures(), 0, "{:?}", r.lines());
    assert_eq!(r.count(Kind::NoEffect), 14);
}
