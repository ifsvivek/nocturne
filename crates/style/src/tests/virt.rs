use super::*;
use std::time::Instant;

fn item(d: &mut Doc, parent: Id, idx: usize) -> Id {
    let c = d.add(parent, "a.card");
    d.attr(c, "data-i", &idx.to_string());
    d.focusable(c);
    d.add(c, "div.thumb");
    let t = d.add(c, "div.title");
    d.text(t, &format!("Item {idx}"));
    c
}

/// page > div.grid (virtual, `count` items, prototype first).
fn list(count: usize) -> (Doc, Id, Builders) {
    let mut d = Doc::new("page");
    let grid = d.add(0, "div.grid");
    d.virtual_list(grid, count);
    let proto = item(&mut d, grid, 0);
    d.virtual_proto(proto);
    let mut b = Builders::new();
    b.insert(
        grid,
        Rc::new(|d: &mut Doc, p: Id, i: usize| {
            item(d, p, i);
        }) as VirtBuilder,
    );
    (d, grid, b)
}

const CSS: &str = ".grid { padding: 0 20px } .card { display: block; width: 100px; margin-right: 10px; margin-bottom: 20px } .thumb { aspect-ratio: 2 / 3 } .title { height: 20px }";

fn build(d: &Doc, css: &str, b: &Builders, vp: (f32, f32)) -> Scene {
    Scene::build(
        d.clone(),
        Rc::new(Sheet::parse(css)),
        Rc::new(Fonts::new()),
        vp,
        Interaction::default(),
        None,
        b,
        &mut approx,
    )
    .expect("builds")
}

fn indexes(s: &Scene, grid: Id) -> Vec<usize> {
    let mut v: Vec<usize> = s.doc.nodes[grid]
        .children
        .iter()
        .filter(|&&c| s.doc.nodes[c].virtual_item)
        .map(|c| s.virt.index[c])
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn metrics_come_from_the_prototype_and_the_theme() {
    let (d, grid, b) = list(10_000);
    let s = build(&d, CSS, &b, (1000.0, 700.0));
    let g = s.virt.grid(grid).unwrap();
    // 960px of content, cells 100 + 10 margin: 8 columns; a card is 100 x (150 + 20).
    assert_eq!(
        (g.cols, g.item_w, g.item_h, g.pitch_x, g.pitch_y),
        (8, 100.0, 170.0, 110.0, 190.0)
    );
    assert_eq!(g.rows, 1250);
    assert_eq!(g.total, 1250.0 * 190.0);
    // The container (and the page) are as tall as the whole list.
    assert_eq!(s.rects[grid][3], g.total);
    assert_eq!(s.page_height(), g.total);
}

#[test]
fn only_the_visible_rows_exist() {
    let (d, grid, b) = list(10_000);
    let t = Instant::now();
    let s = build(&d, CSS, &b, (1000.0, 700.0));
    let ix = indexes(&s, grid);
    // 700px of window: 4 rows, plus 2 rows of overscan below.
    assert_eq!(ix.first(), Some(&0));
    assert_eq!(ix.len(), 6 * 8);
    assert!(s.doc.len() < 400, "{} nodes", s.doc.len());
    assert!(t.elapsed().as_secs() < 5);
    // Items sit at their cell: index 9 is column 1 of row 1.
    let n = *s.doc.nodes[grid]
        .children
        .iter()
        .find(|&&c| s.virt.index.get(&c) == Some(&9))
        .unwrap();
    assert_eq!(s.rects[n][..2], [20.0 + 110.0, 190.0]);
    assert_eq!(s.rects[n][2..], [100.0, 170.0]);
    // The prototype is never shown and never focusable in the tree.
    let proto = s.doc.nodes[grid].children[0];
    assert!(s.doc.nodes[proto].virtual_proto && s.is_hidden(proto));
    assert!(!s.kids[grid].contains(&proto));
    assert!(!s.focus_order().contains(&proto));
}

#[test]
fn scrolling_moves_the_window_without_growing_the_tree() {
    let (d, grid, b) = list(10_000);
    let mut s = build(&d, CSS, &b, (1000.0, 700.0));
    let base = s.doc.len();
    s.wheel(Some(0), 0.0, -3000.0, &mut approx);
    assert_eq!(s.scroll.page_y, 3000.0);
    let ix = indexes(&s, grid);
    // Row 15 starts at 2850: rows 13..=21 are in (2 overscan each side), none of the first rows remain.
    assert!(
        ix.first().unwrap() / 8 >= 12 && *ix.last().unwrap() / 8 <= 22,
        "{ix:?}"
    );
    assert!(s.doc.len() <= base + 8 * 8 * 4);
    let n = *s.doc.nodes[grid]
        .children
        .iter()
        .find(|&&c| s.virt.index.get(&c) == Some(&(16 * 8)))
        .unwrap();
    assert_eq!(s.rects[n][1], 16.0 * 190.0);
    // Far scroll, then back: still bounded and consistent.
    s.wheel(Some(0), 0.0, -100_000_000.0, &mut approx);
    assert_eq!(s.scroll.page_y, s.page_height() - 700.0);
    assert_eq!(indexes(&s, grid).last(), Some(&9_999));
    s.wheel(Some(0), 0.0, 100_000_000.0, &mut approx);
    assert_eq!(indexes(&s, grid).first(), Some(&0));
    assert!(s.doc.len() < 400);
}

#[test]
fn scroll_to_item_brings_it_into_view() {
    let (d, grid, b) = list(10_000);
    let mut s = build(&d, CSS, &b, (1000.0, 700.0));
    s.scroll_to_item(grid, 5_000, &mut approx);
    let n = *s.doc.nodes[grid]
        .children
        .iter()
        .find(|&&c| s.virt.index.get(&c) == Some(&5_000))
        .expect("instantiated");
    let [_, y, _, h] = s.rects[n];
    assert!(
        y >= s.scroll.page_y && y + h <= s.scroll.page_y + 700.0,
        "{y} in {}",
        s.scroll.page_y
    );
    // Focus moves with the keyboard inside the window: Down from an item lands on the item one row below.
    let below = s.spatial(n, Dir::Down).unwrap();
    assert_eq!(s.virt.index[&below], 5_000 + 8);
}

#[test]
fn resize_changes_the_column_count() {
    let (d, grid, b) = list(1_000);
    let mut s = build(&d, CSS, &b, (1000.0, 700.0));
    assert_eq!(s.virt.grid(grid).unwrap().cols, 8);
    s.resize((500.0, 700.0), &mut approx);
    let g = s.virt.grid(grid).unwrap();
    assert_eq!(g.cols, 4);
    assert_eq!(g.rows, 250);
    let ix = indexes(&s, grid);
    assert_eq!(ix.last(), Some(&((6 * 4) - 1)));
}

#[test]
fn the_theme_controls_the_cell_through_css() {
    let (d, grid, b) = list(100);
    let css = ".grid { padding: 0; gap: 8px 12px } .card { display: block; width: 200px } .thumb { height: 100px } .title { height: 20px }";
    let s = build(&d, css, &b, (1000.0, 700.0));
    let g = s.virt.grid(grid).unwrap();
    assert_eq!(
        (g.item_w, g.item_h, g.pitch_x, g.pitch_y),
        (200.0, 120.0, 212.0, 128.0)
    );
    // (1000 + 12) / 212 = 4 columns.
    assert_eq!(g.cols, 4);
    let n = *s.doc.nodes[grid]
        .children
        .iter()
        .find(|&&c| s.virt.index.get(&c) == Some(&5))
        .unwrap();
    assert_eq!(s.rects[n][..2], [212.0, 128.0]);
    // Percent widths follow the container.
    let s = build(
        &d,
        ".grid { padding: 0 } .card { display: block; width: 25%; height: 50px }",
        &b,
        (1000.0, 700.0),
    );
    assert_eq!(s.virt.grid(grid).unwrap().cols, 4);
}

#[test]
fn item_subtrees_are_styled_and_share_styles() {
    let (d, grid, b) = list(1_000);
    let s = build(
        &d,
        &format!("{CSS} .card:hover {{ opacity: 0.5 }}"),
        &b,
        (1000.0, 700.0),
    );
    let cards: Vec<Id> = s.doc.nodes[grid]
        .children
        .iter()
        .copied()
        .filter(|&c| s.doc.nodes[c].virtual_item)
        .collect();
    assert!(cards.len() > 10);
    assert!(
        cards
            .windows(2)
            .all(|p| Rc::ptr_eq(s.style(p[0]), s.style(p[1])))
    );
    // Item text is laid out inside the cell.
    let title = s.doc.nodes[cards[0]].children[1];
    assert_eq!(s.rects[title][3], 20.0);
}

#[test]
fn hover_works_on_virtual_items() {
    let (d, grid, b) = list(1_000);
    let mut s = build(
        &d,
        &format!("{CSS} .card:hover {{ opacity: 0.5 }}"),
        &b,
        (1000.0, 700.0),
    );
    let n = *s.doc.nodes[grid]
        .children
        .iter()
        .find(|&&c| s.virt.index.get(&c) == Some(&3))
        .unwrap();
    let [x, y, ..] = s.rects[n];
    let hit = s.hit_test(x + 5.0, y + 5.0).expect("hit");
    let card = if s.doc.nodes[hit].virtual_item {
        hit
    } else {
        s.doc.nodes[hit].parent.unwrap()
    };
    assert_eq!(card, n);
    s.update(
        Interaction {
            hover: Some(hit),
            ..Default::default()
        },
        Instant::now(),
        &mut approx,
    )
    .unwrap();
    assert_eq!(s.style(n).opacity, 0.5);
}

#[test]
fn ten_thousand_items_lay_out_quickly() {
    let (d, _, b) = list(10_000);
    let t = Instant::now();
    let mut s = build(&d, CSS, &b, (1920.0, 1080.0));
    let first = t.elapsed();
    let t = Instant::now();
    for k in 0..50 {
        s.wheel(Some(0), 0.0, -(190.0 * (1 + k % 3) as f32), &mut approx);
    }
    let scrolls = t.elapsed();
    eprintln!("virtual 10k: build {first:?}, 50 scrolls {scrolls:?}");
    assert!(s.doc.len() < 800);
    // Debug build, shared machine: only a gross regression fails this.
    assert!(first.as_millis() < 2000 && scrolls.as_millis() < 5000);
}

#[test]
fn focus_and_hover_follow_their_item_when_rows_are_recycled() {
    let (d, grid, b) = list(1_000);
    let mut s = build(&d, CSS, &b, (1000.0, 700.0));
    let focus = s.item_node(10).unwrap();
    let hover = s.doc.nodes[focus].children[1];
    s.update(
        Interaction {
            hover: Some(hover),
            focus: Some(focus),
            ..Default::default()
        },
        Instant::now(),
        &mut approx,
    )
    .unwrap();
    // Rows scroll by: the nodes are rebuilt, the state stays with item 10.
    let old = s.item_node(10).unwrap();
    s.wheel(Some(grid), 0.0, -600.0, &mut approx);
    let now = s.item_node(10);
    assert!(
        now.is_some() && now != Some(old),
        "item 10 survived, as a new node"
    );
    assert_eq!(s.interaction.focus, now);
    assert_eq!(s.item_index(s.interaction.hover.unwrap()), Some(10));
    assert_eq!(s.interaction.hover, now.map(|n| s.doc.nodes[n].children[1]));
    // Far away, the item is gone and the state with it.
    s.scroll_to_item(grid, 900, &mut approx);
    assert!(s.item_node(10).is_none());
    assert_eq!(s.interaction.focus, None);
}
