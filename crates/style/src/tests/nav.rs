use super::*;

/// A `cols` x `rows` grid of focusable 50x30 cells under `div.g`, ids row by row.
fn grid(cols: usize, rows: usize) -> (Doc, Vec<Id>) {
    let mut d = Doc::new("page");
    let g = d.add(0, "div.g");
    let cells = (0..cols * rows)
        .map(|_| {
            let c = d.add(g, "a.cell");
            d.focusable(c);
            c
        })
        .collect();
    (d, cells)
}

const GRID_CSS: &str = ".g { display: grid; grid-template-columns: repeat(3, 50px); gap: 10px } .cell { height: 30px }";

#[test]
fn arrow_keys_walk_a_grid() {
    let (d, c) = grid(3, 3);
    let s = scene(&d, GRID_CSS, (800.0, 600.0));
    assert_eq!(s.spatial(c[4], Dir::Right), Some(c[5]));
    assert_eq!(s.spatial(c[4], Dir::Left), Some(c[3]));
    assert_eq!(s.spatial(c[4], Dir::Down), Some(c[7]));
    assert_eq!(s.spatial(c[4], Dir::Up), Some(c[1]));
    // Edges have no neighbour.
    assert_eq!(s.spatial(c[0], Dir::Left), None);
    assert_eq!(s.spatial(c[0], Dir::Up), None);
    assert_eq!(s.spatial(c[8], Dir::Right), None);
    assert_eq!(s.spatial(c[8], Dir::Down), None);
}

#[test]
fn spatial_follows_css_order_not_source_order() {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    let c: Vec<Id> = (0..3)
        .map(|_| {
            let n = d.add(row, "a.cell");
            d.focusable(n);
            n
        })
        .collect();
    // The first cell of the source is the last on screen.
    let s = scene(
        &d,
        ".row { display: flex } .cell { width: 50px; height: 30px } .cell:nth-child(1) { order: 3 } .cell:nth-child(3) { order: -1 }",
        (800.0, 600.0),
    );
    // Visual: c2 c1 c0.
    assert_eq!(s.spatial(c[2], Dir::Right), Some(c[1]));
    assert_eq!(s.spatial(c[1], Dir::Right), Some(c[0]));
    assert_eq!(s.spatial(c[0], Dir::Right), None);
    assert_eq!(s.spatial(c[0], Dir::Left), Some(c[1]));
}

#[test]
fn spatial_uses_the_cone_when_nothing_overlaps() {
    let mut d = Doc::new("page");
    let a = d.add(0, "a.a");
    let b = d.add(0, "a.b");
    let far = d.add(0, "a.far");
    for n in [a, b, far] {
        d.focusable(n);
    }
    let css = "a { position: absolute; width: 20px; height: 20px } .a { left: 0; top: 0 } .b { left: 30px; top: 70px } .far { left: 400px; top: 5px }";
    let s = scene(&d, css, (800.0, 600.0));
    // Right of `a`: `far` overlaps its row; `b` is lower and only in the cone: the same-row candidate wins.
    assert_eq!(s.spatial(a, Dir::Right), Some(far));
    assert_eq!(s.spatial(a, Dir::Down), Some(b));
    // Straight down from `far` has no one below; `b` is outside a narrow cone to the left, so Left finds it.
    assert_eq!(s.spatial(far, Dir::Down), None);
    assert_eq!(
        s.spatial(far, Dir::Left),
        Some(a),
        "the same row wins over the lower, nearer one"
    );
    assert_eq!(
        s.spatial(b, Dir::Right),
        Some(far),
        "no one overlaps b's row: the cone finds far"
    );
}

#[test]
fn hidden_elements_are_skipped() {
    let (d, c) = grid(3, 1);
    let s = scene(
        &d,
        &format!("{GRID_CSS} .cell:nth-child(2) {{ display: none }}"),
        (800.0, 600.0),
    );
    assert_eq!(s.spatial(c[0], Dir::Right), Some(c[2]));
    assert_eq!(s.focus_order(), vec![c[0], c[2]]);
    let s = scene(
        &d,
        &format!("{GRID_CSS} .cell:nth-child(2) {{ visibility: hidden }}"),
        (800.0, 600.0),
    );
    assert_eq!(s.focus_order(), vec![c[0], c[2]]);
}

#[test]
fn focus_order_is_visual_and_tab_wraps() {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    let cs: Vec<Id> = (0..3)
        .map(|_| {
            let c = d.add(row, "a.c");
            d.focusable(c);
            c
        })
        .collect();
    let s = scene(
        &d,
        ".row { display: flex; flex-direction: row-reverse }",
        (800.0, 600.0),
    );
    assert_eq!(s.focus_order(), vec![cs[2], cs[1], cs[0]]);
    assert_eq!(s.focus_next(None, true), Some(cs[2]));
    assert_eq!(s.focus_next(Some(cs[0]), true), Some(cs[2]), "wraps");
    assert_eq!(
        s.focus_next(Some(cs[2]), false),
        Some(cs[0]),
        "wraps backwards"
    );
    assert_eq!(s.focus_next(None, false), Some(cs[0]));
    // Only what the app marked is focusable, whatever the CSS says.
    let mut d2 = d.clone();
    d2.nodes[cs[1]].focusable = false;
    let s = scene(&d2, "", (800.0, 600.0));
    assert_eq!(s.focus_order(), vec![cs[0], cs[2]]);
}

#[test]
fn nav_rects_follow_container_and_page_scroll() {
    let mut d = Doc::new("page");
    let sc = d.add(0, "div.sc");
    let items: Vec<Id> = (0..10)
        .map(|_| {
            let i = d.add(sc, "a.i");
            d.focusable(i);
            i
        })
        .collect();
    let mut s = scene(
        &d,
        ".sc { height: 100px; overflow-y: auto } .i { display: block; height: 30px }",
        (800.0, 600.0),
    );
    let before = s.nav_rect(items[5])[1];
    s.wheel(Some(items[0]), 0.0, -90.0, &mut approx);
    assert_eq!(s.nav_rect(items[5])[1], before - 90.0);
    // Revealing a far item scrolls the container so it is inside it.
    s.reveal(items[9], &mut approx);
    let off = s.scroll.offsets[&sc][1];
    assert_eq!(off, 300.0 - 100.0);
}

#[test]
fn hit_testing_honours_clipping_visibility_and_pointer_events() {
    let mut d = Doc::new("page");
    let clip = d.add(0, "div.clip");
    let inner = d.add(clip, "div.inner");
    let over = d.add(0, "div.over");
    let css = ".clip { width: 100px; height: 100px; overflow: hidden } .inner { width: 300px; height: 300px } .over { position: absolute; left: 0; top: 150px; width: 50px; height: 50px }";
    let s = scene(&d, css, (800.0, 600.0));
    assert_eq!(s.hit_test(10.0, 10.0), Some(inner));
    assert_eq!(
        s.hit_test(150.0, 50.0),
        Some(0),
        "outside the clip falls through to the page"
    );
    assert_eq!(s.hit_test(10.0, 160.0), Some(over));
    let s = scene(
        &d,
        &format!("{css} .over {{ pointer-events: none }}"),
        (800.0, 600.0),
    );
    assert_eq!(s.hit_test(10.0, 160.0), Some(0));
    let s = scene(
        &d,
        &format!("{css} .over {{ visibility: hidden }}"),
        (800.0, 600.0),
    );
    assert_eq!(s.hit_test(10.0, 160.0), Some(0));
    // z-index decides which of two overlapping boxes is on top.
    let s = scene(
        &d,
        &format!("{css} .over {{ top: 10px; z-index: -1 }}"),
        (800.0, 600.0),
    );
    assert_eq!(s.hit_test(10.0, 20.0), Some(inner));
}

#[test]
fn scroll_state_clamps_and_survives_resize() {
    let (d, _) = grid(1, 1);
    let mut d = d;
    let tall = d.add(0, "div.tall");
    let _ = tall;
    let mut s = scene(&d, ".tall { height: 2000px }", (800.0, 600.0));
    s.wheel(Some(0), 0.0, -100000.0, &mut approx);
    assert_eq!(s.scroll.page_y, s.page_height() - 600.0);
    s.resize((800.0, 1200.0), &mut approx);
    assert!(s.scroll.page_y <= s.page_height() - 1200.0 + 0.01);
    s.wheel(Some(0), 0.0, 100000.0, &mut approx);
    assert_eq!(s.scroll.page_y, 0.0);
}
