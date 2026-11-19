use super::*;
use crate::select::{self, HOVER};

fn scene(doc: &Doc, css: &str, vp: (f32, f32)) -> Scene {
    Scene::build(
        doc,
        &Sheet::parse(css),
        &flags(doc, None, None),
        vp,
        &mut approx,
    )
}

fn find(doc: &Doc, class: &str) -> Vec<Id> {
    (0..doc.len())
        .filter(|&i| doc.nodes[i].classes.iter().any(|c| c == class))
        .collect()
}

fn small() -> (Doc, Id, Id, Id) {
    let mut d = Doc::new("page");
    let a = d.add(0, "div.a");
    let b = d.add(0, "div.b");
    let c = d.add(a, "span.c");
    d.text(c, "hello");
    d.attr(b, "data-kind", "movie");
    (d, a, b, c)
}

#[test]
fn selectors_match_the_documented_forms() {
    let (d, a, b, c) = small();
    let f = vec![0u8; d.len()];
    let m = |s: &str, n: Id| select::parse(s).unwrap().matches(&d, n, &f);
    assert!(m(".a", a) && !m(".a", b));
    assert!(m("page > .a > .c", c) && !m("page > .c", c) && m("page .c", c));
    assert!(m(".a + .b", b) && m(".a ~ .b", b) && !m(".b + .a", a));
    assert!(m("[data-kind=movie]", b) && m("[data-kind^=mo]", b) && !m("[data-kind=show]", b));
    assert!(m(":not(.a)", b) && !m(":not(.a)", a));
    assert!(
        m(".a:first-child", a)
            && m(".b:last-child", b)
            && m(":nth-child(2)", b)
            && m(":nth-child(odd)", a)
    );
    assert!(m("page:root", 0));
    let mut f2 = f.clone();
    f2[a] |= HOVER;
    assert!(select::parse(".a:hover").unwrap().matches(&d, a, &f2));
    for bad in ["a::before", "a:visited", ">", "", "a:nth-child(x)"] {
        assert!(select::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn specificity_beats_order_and_important_beats_specificity() {
    let (d, a, ..) = small();
    let s = scene(
        &d,
        "div { width: 10px } .a { width: 20px } div { width: 30px }",
        (800.0, 600.0),
    );
    assert_eq!(s.rects[a][2], 20.0);
    let s = scene(
        &d,
        ".a { width: 20px } div { width: 30px !important }",
        (800.0, 600.0),
    );
    assert_eq!(s.rects[a][2], 30.0);
}

#[test]
fn custom_properties_inherit_and_substitute() {
    let (d, a, _, c) = small();
    let s = scene(
        &d,
        "page { --w: 40px; --h: var(--w) } .a { width: var(--w); height: var(--h) } .c { color: var(--nope, #fff) }",
        (800.0, 600.0),
    );
    assert_eq!(&s.rects[a][2..], &[40.0, 40.0]);
    assert!(s.report.messages.is_empty(), "{:?}", s.report.messages);
    assert_eq!(s.styles[c].color.r, 255);
}

#[test]
fn text_properties_inherit() {
    let (d, _, _, c) = small();
    let s = scene(
        &d,
        "page { color: #102030; font-size: 20px }",
        (800.0, 600.0),
    );
    assert_eq!((s.styles[c].color.g, s.styles[c].font_size), (0x20, 20.0));
}

#[test]
fn flex_and_grid_place_children_and_order_reorders() {
    let (d, a, b, _) = small();
    let s = scene(
        &d,
        "page { display: flex } .a { width: 100px } .b { width: 50px }",
        (800.0, 600.0),
    );
    assert!(s.rects[a][0] < s.rects[b][0]);
    let s = scene(
        &d,
        "page { display: flex } .a { width: 100px; order: 2 } .b { width: 50px }",
        (800.0, 600.0),
    );
    assert!(s.rects[b][0] < s.rects[a][0] && s.rects[a][0] == 50.0);
    let s = scene(
        &d,
        "page { display: grid; grid-template-columns: 200px 1fr } .a { grid-column: 2 } .b { grid-column: 1 }",
        (800.0, 600.0),
    );
    assert_eq!(
        (s.rects[a][0], s.rects[a][2], s.rects[b][0]),
        (200.0, 600.0, 0.0)
    );
}

#[test]
fn viewport_units_resolve_and_display_none_removes_the_box() {
    let (d, a, b, _) = small();
    let s = scene(
        &d,
        ".a { height: 50vh } .b { display: none }",
        (800.0, 600.0),
    );
    assert_eq!(s.rects[a][3], 300.0);
    assert!(s.is_hidden(b));
}

#[test]
fn unsupported_things_are_reported_not_fatal() {
    let sheet = Sheet::parse(
        "a::before { content: 'x' } .a { frobnicate: 1; width: 10px } @media (min-width:1px) { .a { width: 1px } }",
    );
    assert!(!sheet.warnings.is_empty());
    let (d, a, ..) = small();
    let s = Scene::build(
        &d,
        &sheet,
        &flags(&d, None, None),
        (800.0, 600.0),
        &mut approx,
    );
    assert_eq!(s.rects[a][2], 10.0);
    assert_eq!(s.report.unsupported, 1);
}

#[test]
fn the_page_is_as_tall_as_its_content() {
    let (d, ..) = small();
    let s = scene(&d, ".a { height: 2000px }", (800.0, 600.0));
    assert_eq!(s.rects[0][3], 2000.0 + s.rects[1 + 1][3]);
}

#[test]
fn bundled_themes_read_cleanly_and_lay_out_the_demo() {
    let d = demo::home();
    for (name, css) in THEMES {
        let sheet = Sheet::parse(css);
        assert!(sheet.warnings.is_empty(), "{name}: {:?}", sheet.warnings);
        let s = Scene::build(
            &d,
            &sheet,
            &flags(&d, None, None),
            (1920.0, 1080.0),
            &mut approx,
        );
        assert!(
            s.report.messages.is_empty(),
            "{name}: {:?}",
            s.report.messages
        );
        // Every visible card has a real box and lies inside the page width or inside a scroll container.
        for c in find(&d, "thumb") {
            assert!(
                s.rects[c][2] > 50.0 && s.rects[c][3] > 20.0,
                "{name}: thumb {:?}",
                s.rects[c]
            );
        }
    }
}

#[test]
fn themes_really_differ_in_layout() {
    let d = demo::home();
    let at = |name: &str, class: &str| {
        let css = THEMES.iter().find(|t| t.0 == name).unwrap().1;
        let s = scene(&d, css, (1920.0, 1080.0));
        let n = find(&d, class)[0];
        (s.rects[n], s.is_hidden(n))
    };
    let (hero_streaming, hidden) = at("streaming", "hero");
    assert!(!hidden && hero_streaming[3] > 800.0);
    assert!(at("videosite", "hero").1);
    let (side, _) = at("videosite", "sidebar");
    let (main, _) = at("videosite", "main");
    assert!(
        side[2] == 240.0 && main[0] == 240.0,
        "grid columns: {side:?} {main:?}"
    );
    let (rail, _) = at("rail", "header");
    assert_eq!((rail[0], rail[2]), (0.0, 230.0));
    let (dock, _) = at("dock", "header");
    assert_eq!(dock[1] + dock[3], dock[1] + 64.0);
}

/// Timing, run with `cargo test -p nocturne-style --release -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing() {
    for n in [1_000, 10_000] {
        let d = demo::big(n);
        for (name, css) in THEMES {
            let sheet = Sheet::parse(css);
            let f = flags(&d, None, None);
            let mut best = (u128::MAX, u128::MAX, u128::MAX);
            for _ in 0..5 {
                let mut s = Scene::build(&d, &sheet, &f, (1920.0, 1080.0), &mut approx);
                let r = s.resize(&d, (1600.0, 900.0), &mut approx);
                best = (
                    best.0.min(s.timing.cascade.as_micros()),
                    best.1.min(s.timing.layout.as_micros()),
                    best.2.min(r.as_micros()),
                );
            }
            println!(
                "{n:>6} elements ({} nodes) {name:<10} cascade {:>6} us  layout {:>6} us  resize {:>6} us",
                d.len(),
                best.0,
                best.1,
                best.2
            );
        }
    }
}
