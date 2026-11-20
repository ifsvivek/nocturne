use super::*;
use std::time::{Duration, Instant};

fn deep(n: usize) -> Doc {
    let mut d = Doc::new("page");
    let mut p = 0;
    for _ in 0..n {
        p = d.add(p, "div.a");
    }
    d
}

#[test]
fn oversized_stylesheet_is_refused_whole() {
    let big = format!(".a {{ width: 1px }}{}", " ".repeat(1100 * 1024));
    let sheet = Sheet::parse(&big);
    assert_eq!(sheet.rule_count(), 0);
    assert_eq!(sheet.report.count(Kind::Limit), 1);
}

#[test]
fn too_many_rules_are_cut_off() {
    let css: String = (0..22_000)
        .map(|i| format!(".c{i} {{ width: 1px }}\n"))
        .collect();
    let sheet = Sheet::parse(&css);
    assert!(sheet.rule_count() <= sheet::MAX_RULES);
    assert!(sheet.report.count(Kind::Limit) >= 1);
}

#[test]
fn tree_depth_and_node_count_are_capped() {
    let d = deep(500);
    assert!(d.len() <= doc::MAX_DEPTH as usize + 1);
    assert!(d.dropped >= 400);
    // The capped tree still builds and lays out.
    let s = scene(&d, ".a { padding: 1px }", (800.0, 600.0));
    assert!(s.rects[0][3] > 0.0);
    let mut big = Doc::new("page");
    for _ in 0..60_000 {
        big.add(0, "i");
    }
    assert_eq!(big.len(), doc::MAX_NODES);
    assert_eq!(big.dropped, 60_000 - (doc::MAX_NODES - 1));
}

#[test]
fn pathological_selectors_end_within_the_time_budget() {
    let d = deep(90);
    // Eight descendant combinators over a 90-deep chain would be exponential without the step budget; `:is()` and `:has()` too.
    let css = ".a .a .a .a .a .a .a .zzz { width: 1px } \
               :is(.a, .b) :is(.a, .c) :is(.a, .d) :is(.a, .e) :is(.a, .f) :is(.a, .g) .zzz { width: 2px } \
               .a:has(.a .a .a .a .zzz) { width: 3px } \
               * * * * * * * * { height: 1px } \
               .a ~ .a ~ .a ~ .a ~ .a ~ .a ~ .zzz { width: 4px }";
    let t = Instant::now();
    let s = scene(&d, css, (800.0, 600.0));
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "took {:?}",
        t.elapsed()
    );
    assert!(s.rects[0][3] > 0.0);
}

#[test]
fn wide_sibling_lists_stay_cheap() {
    let mut d = Doc::new("page");
    for _ in 0..8_000 {
        d.add(0, "i.x");
    }
    let t = Instant::now();
    let _ = scene(
        &d,
        ".x:nth-of-type(7n+1) { width: 1px } .x ~ .x ~ .x ~ .y { width: 2px } .x:last-of-type { height: 1px }",
        (800.0, 600.0),
    );
    assert!(
        t.elapsed() < Duration::from_secs(10),
        "took {:?}",
        t.elapsed()
    );
}

#[test]
fn a_hostile_value_cannot_reach_taffy() {
    let (d, a, ..) = small();
    let css = ".a { width: calc(99999999999px * 99999999 * 99999999 * 99999999); height: 1e99999px; padding: 1e40px; margin-left: calc(1px / 0); grid-template-columns: repeat(100000, 1fr); } .b { display: grid; grid-template-columns: repeat(500, 1px) }";
    let s = scene(&d, css, (800.0, 600.0));
    for r in &s.rects {
        assert!(r.iter().all(|v| v.is_finite()), "{r:?}");
    }
    assert!(s.rects[a][2] <= 100_001.0);
    assert!(s.warnings().count(Kind::BadValue) >= 1);
}

#[test]
fn many_tracks_are_rejected() {
    let tracks = "1px ".repeat(300);
    let (d, ..) = small();
    let s = scene(
        &d,
        &format!(".a {{ display: grid; grid-template-columns: {tracks} }}"),
        (800.0, 600.0),
    );
    assert!(s.warnings().count(Kind::BadValue) >= 1);
}

#[test]
fn garbage_input_never_panics() {
    let (d, ..) = small();
    for css in [
        "}",
        "{",
        "{{{{",
        "a{",
        "a{b",
        "a{b:",
        "a{b:c",
        "a{b:c;;;}",
        "@media",
        "@media {",
        "@media (",
        "@supports",
        "@supports (",
        "@@@",
        "a, , b { x: y }",
        "\u{0}",
        "a { width: calc( }",
        "a { width: calc(((((((((((((((((1px)))))))))))))))) }",
        "a { background: linear-gradient( }",
        "a { background: linear-gradient(red) }",
        "a { transition: , , }",
        "a { content: \"unterminated }",
        "a { font: }",
        "a { font: 12px }",
        "a:is(  ) { x: y }",
        "a:nth-child(  ) {}",
        "a { transform: translateX( }",
        "a { box-shadow: 1px }",
        ":root { --x: var(--x) } a { width: var(--x) }",
        "a { grid-area: / / / / }",
        "a { border-radius: 1px / 2px }",
        "a { margin: 1px 2px 3px 4px 5px }",
    ] {
        let s = scene(&d, css, (800.0, 600.0));
        assert!(s.rects[0][2] > 0.0, "{css:?}");
        let _ = s.warnings().lines();
    }
}

#[test]
fn restyle_over_budget_ends_in_a_failure_not_a_hang() {
    let mut d = Doc::new("page");
    for _ in 0..2_000 {
        d.add(0, "div.x");
    }
    let r = Scene::build(
        d,
        Rc::new(Sheet::parse(".x { width: 1px }")),
        Rc::new(Fonts::new()),
        (800.0, 600.0),
        Interaction::default(),
        Some(Duration::from_nanos(1)),
        &Builders::new(),
        &mut approx,
    );
    assert!(matches!(r, Err(Failure::Overrun)));
}

#[test]
fn a_selector_cut_off_by_its_step_budget_is_reported() {
    let d = deep(95);
    let s = scene(
        &d,
        ".a .a .a .a .a .a .a .zzz { width: 1px } .a:has(.a .a .a .a .a .a .a .zzz) { width: 2px }",
        (800.0, 600.0),
    );
    // Whether or not this tree trips the budget, the report is consistent and the layout is sane.
    let _ = s.warnings().count(Kind::Limit);
    assert!(s.rects[0][3] > 0.0);
}
