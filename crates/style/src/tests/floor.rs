use super::*;
use crate::sheet::{DEFAULT, USER};

/// The app's own look for the controls: a sane size, so the floor only ever meets what a theme did.
const BASE: &str = ".back, .other { display: block; width: 60px; height: 30px }";

fn themed(d: &Doc, css: &str) -> Scene {
    scene_with(d, Sheet::layered(&[(BASE, DEFAULT), (css, USER)]), (800.0, 600.0), Interaction::default())
}

/// page > header > [a.back (essential), a.other] ; page > main
fn app() -> (Doc, Id, Id, Id) {
    let mut d = Doc::new("page");
    let header = d.add(0, "header.top");
    let back = d.add(header, "a.back");
    d.text(back, "Back");
    d.essential(back);
    d.focusable(back);
    let other = d.add(header, "a.other");
    d.text(other, "Other");
    d.add(0, "main.m");
    (d, header, back, other)
}

#[test]
fn an_essential_control_cannot_be_hidden_or_made_transparent() {
    let (d, _, back, other) = app();
    let s = themed(&d, ".back { display: none } .other { display: none }");
    assert!(!s.is_hidden(back));
    assert!(s.is_hidden(other), "other elements may still be hidden");
    let r = s.warnings();
    assert_eq!(r.count(Kind::Floor), 1);
    assert!(r.groups[&Kind::Floor].examples[0].contains(".back"), "{:?}", r.lines());
    for css in [
        ".back { opacity: 0 }",
        ".back { opacity: 0.39 }",
        ".back { visibility: hidden }",
        ".back { display: none !important }",
    ] {
        let s = themed(&d, css);
        let c = s.style(back);
        assert!(!s.is_hidden(back) && c.opacity >= 0.4 && c.visible, "{css}");
        assert_eq!(s.warnings().count(Kind::Floor), 1, "{css}");
    }
    // A mild fade is the theme's business.
    let s = themed(&d, ".back { opacity: 0.6 }");
    assert_eq!(s.style(back).opacity, 0.6);
    assert_eq!(s.warnings().count(Kind::Floor), 0);
}

#[test]
fn ancestors_of_an_essential_control_are_protected_too() {
    let (d, header, back, _) = app();
    let s = themed(&d, ".top { display: none; opacity: 0 }");
    assert!(!s.is_hidden(header) && !s.is_hidden(back));
    assert_eq!(s.style(header).opacity, 1.0);
    // The page's other children are free to go.
    let s = themed(&d, ".m { display: none }");
    assert!(s.is_hidden(d.nodes[0].children[1]));
}

#[test]
fn the_apps_own_default_sheet_may_do_what_a_theme_may_not() {
    let (d, _, back, _) = app();
    let sheet = Sheet::layered(&[(".back { display: none }", DEFAULT), ("", USER)]);
    let s = scene_with(&d, sheet, (800.0, 600.0), Interaction::default());
    assert!(s.is_hidden(back));
    let sheet = Sheet::layered(&[(".back { display: none }", DEFAULT), (".back { display: none }", USER)]);
    let s = scene_with(&d, sheet, (800.0, 600.0), Interaction::default());
    // The default sheet's rule hid it; the user's repeat is blocked from changing anything it did not already decide.
    assert_eq!(s.warnings().count(Kind::Floor), 1);
}

#[test]
fn too_small_gets_a_minimum_size_and_is_reported() {
    let (d, _, back, _) = app();
    let s = themed(&d, ".back { display: block; width: 4px; height: 3px; padding: 0 }");
    let [_, _, w, h] = s.rects[back];
    assert!(w >= 24.0 && h >= 24.0, "{w}x{h}");
    assert_eq!(s.warnings().count(Kind::Floor), 1);
    // Not shrunk: a control that is already big enough is left alone.
    let s = themed(&d, ".back { display: block; width: 60px; height: 30px }");
    assert_eq!(s.rects[back][2..], [60.0, 30.0]);
    assert_eq!(s.warnings().count(Kind::Floor), 0);
}

#[test]
fn off_screen_is_moved_back_into_view() {
    let (d, _, back, other) = app();
    for css in [
        ".back { position: absolute; left: -9999px; top: 10px; width: 40px; height: 30px }",
        ".back { position: absolute; right: -5000px; top: 10px; width: 40px; height: 30px }",
        ".back { position: fixed; left: 10px; top: -4000px; width: 40px; height: 30px }",
        ".back { position: fixed; left: 10px; bottom: -4000px; width: 40px; height: 30px }",
        ".back { position: absolute; top: 100000px; left: 3px; width: 40px; height: 30px }",
    ] {
        let s = themed(&d, css);
        let [x, y, w, h] = s.rects[back];
        assert!(x >= 0.0 && x + w <= 800.0 + 0.01, "{css}: {x}");
        assert!(y >= 0.0 && y + h <= 600.0f32.max(s.page_height()) + 0.01, "{css}: {y}");
        assert!(s.warnings().count(Kind::Floor) >= 1, "{css}");
    }
    // Other elements may be placed anywhere.
    let s = themed(&d, ".other { position: absolute; left: -9999px }");
    assert!(s.rects[other][0] < -100.0);
    // A fixed control, moved, still takes its children along.
    let mut d2 = d.clone();
    let child = d2.add(back, "i.k");
    let s = themed(&d2, ".back { position: fixed; left: -3000px; top: 5px; width: 50px; height: 30px } .k { width: 5px; height: 5px }");
    assert!(s.rects[child][0] >= 0.0 && s.rects[child][0] < 60.0);
}

#[test]
fn essential_marking_is_visible_to_selectors() {
    let (d, _, back, other) = app();
    assert!(matches(&d, "[data-essential]", back) && !matches(&d, "[data-essential]", other));
}
