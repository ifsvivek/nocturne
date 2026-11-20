use super::*;

fn r(s: &Scene, n: Id) -> [f32; 4] {
    s.rects[n]
}

/// page with n boxes of the given class, each `w`x`h` unless the css says otherwise.
fn boxes(n: usize) -> (Doc, Vec<Id>) {
    let mut d = Doc::new("page");
    let ids = (0..n).map(|_| d.add(0, "div.b")).collect();
    (d, ids)
}

/// page > div.c > n boxes: the container is the one the css styles as flex or grid (the page itself is the document).
fn boxes_in_c(n: usize) -> (Doc, Vec<Id>) {
    let mut d = Doc::new("page");
    let c = d.add(0, "div.c");
    let ids = (0..n).map(|_| d.add(c, "div.b")).collect();
    (d, ids)
}

#[test]
fn flex_row_gap_grow_and_alignment() {
    let (d, b) = boxes_in_c(3);
    let s = scene(
        &d,
        ".c { display: flex; gap: 10px; align-items: center; height: 100px } .b { width: 50px; height: 20px } .b:nth-child(2) { flex: 1 }",
        (400.0, 300.0),
    );
    assert_eq!(r(&s, b[0]), [0.0, 40.0, 50.0, 20.0]);
    assert_eq!(r(&s, b[1]), [60.0, 40.0, 280.0, 20.0]);
    assert_eq!(r(&s, b[2]), [350.0, 40.0, 50.0, 20.0]);
}

#[test]
fn flex_column_wrap_and_justify() {
    let (d, b) = boxes_in_c(3);
    let s = scene(
        &d,
        ".c { display: flex; flex-direction: column; justify-content: space-between; height: 200px } .b { height: 20px }",
        (400.0, 300.0),
    );
    assert_eq!(
        (r(&s, b[0])[1], r(&s, b[1])[1], r(&s, b[2])[1]),
        (0.0, 90.0, 180.0)
    );
    let s = scene(
        &d,
        ".c { display: flex; flex-wrap: wrap; width: 100px } .b { width: 60px; height: 10px }",
        (400.0, 300.0),
    );
    assert_eq!((r(&s, b[1])[0], r(&s, b[1])[1]), (0.0, 10.0));
}

#[test]
fn grid_columns_gap_and_spans() {
    let (d, b) = boxes_in_c(5);
    let s = scene(
        &d,
        ".c { display: grid; grid-template-columns: 100px 1fr 100px; gap: 8px; width: 400px } .b { height: 10px } .b:nth-child(1) { grid-column: 1 / -1 }",
        (400.0, 300.0),
    );
    assert_eq!(r(&s, b[0]), [0.0, 0.0, 400.0, 10.0]);
    assert_eq!(r(&s, b[1]), [0.0, 18.0, 100.0, 10.0]);
    assert_eq!(r(&s, b[2]), [108.0, 18.0, 184.0, 10.0]);
    assert_eq!(r(&s, b[3]), [300.0, 18.0, 100.0, 10.0]);
}

#[test]
fn grid_auto_fill_minmax() {
    let (d, b) = boxes_in_c(5);
    let s = scene(
        &d,
        ".c { display: grid; grid-template-columns: repeat(auto-fill, minmax(100px, 1fr)); width: 350px } .b { height: 10px }",
        (400.0, 300.0),
    );
    // 3 columns of 116.67px, the 4th box wraps to the second row.
    assert!((r(&s, b[1])[0] - 350.0 / 3.0).abs() < 0.5);
    assert_eq!(r(&s, b[3])[1], 10.0);
}

#[test]
fn grid_template_areas_place_named_areas() {
    let mut d = Doc::new("page");
    let [h, sb, m, f] = ["div.h", "div.sb", "div.m", "div.f"].map(|c| d.add(0, c));
    let css = "page { display: grid; width: 400px; height: 300px; grid-template-columns: 100px 1fr; grid-template-rows: 40px 1fr 30px; grid-template-areas: \"head head\" \"side main\" \"foot foot\" } .h { grid-area: head } .sb { grid-area: side } .m { grid-area: main } .f { grid-area: foot }";
    let s = scene(&d, css, (400.0, 300.0));
    assert_eq!(r(&s, h), [0.0, 0.0, 400.0, 40.0]);
    assert_eq!(r(&s, sb), [0.0, 40.0, 100.0, 230.0]);
    assert_eq!(r(&s, m), [100.0, 40.0, 300.0, 230.0]);
    assert_eq!(r(&s, f), [0.0, 270.0, 400.0, 30.0]);
    // A non-rectangular area and ragged rows are rejected and reported.
    let s = scene(
        &d,
        "page { display: grid; grid-template-areas: \"a b\" \"b a\" }",
        (400.0, 300.0),
    );
    assert!(s.warnings().count(Kind::BadValue) >= 1);
    let s = scene(
        &d,
        "page { display: grid; grid-template-areas: \"a b\" \"c\" }",
        (400.0, 300.0),
    );
    assert!(s.warnings().count(Kind::BadValue) >= 1);
}

#[test]
fn absolute_positioning_relative_to_the_parent_box() {
    let mut d = Doc::new("page");
    let host = d.add(0, "div.host");
    let pos = d.add(host, "div.pos");
    let s = scene(
        &d,
        ".host { position: relative; margin: 20px; width: 200px; height: 100px } .pos { position: absolute; right: 10px; bottom: 5px; width: 30px; height: 15px }",
        (400.0, 300.0),
    );
    assert_eq!(r(&s, pos), [20.0 + 160.0, 20.0 + 80.0, 30.0, 15.0]);
}

#[test]
fn fixed_is_relative_to_the_window_and_ignores_page_scroll() {
    let mut d = Doc::new("page");
    let tall = d.add(0, "div.tall");
    let wrap = d.add(0, "div.wrap");
    let fx = d.add(wrap, "div.fx");
    let inner = d.add(fx, "div.inner");
    let mut s = scene(
        &d,
        ".tall { height: 2000px } .wrap { margin: 30px; padding: 30px } .fx { position: fixed; left: 5px; bottom: 7px; width: 100px; height: 40px } .inner { height: 10px }",
        (400.0, 300.0),
    );
    assert_eq!(r(&s, fx), [5.0, 300.0 - 7.0 - 40.0, 100.0, 40.0]);
    assert_eq!(r(&s, inner)[1], 300.0 - 7.0 - 40.0);
    let _ = tall;
    // The hit test finds it at the same window position whatever the page scroll.
    s.wheel(Some(0), 0.0, -500.0, &mut approx);
    assert!(s.scroll.page_y > 0.0);
    assert!(matches!(s.hit_test(10.0, 273.0), Some(n) if n == fx || n == inner));
    assert_eq!(s.nav_rect(fx)[1], 253.0 + s.scroll.page_y);
}

#[test]
fn order_reorders_layout_paint_and_focus_but_not_the_tree() {
    let mut d = Doc::new("page");
    let [a, b, c] = ["a.x", "a.y", "a.z"].map(|k| {
        let n = d.add(0, k);
        d.focusable(n);
        n
    });
    let s = scene(
        &d,
        "page { display: flex } a { width: 10px; height: 10px } .x { order: 3 } .y { order: 1 } .z { order: 2 }",
        (400.0, 300.0),
    );
    assert_eq!((r(&s, b)[0], r(&s, c)[0], r(&s, a)[0]), (0.0, 10.0, 20.0));
    assert_eq!(s.kids[0], vec![b, c, a]);
    assert_eq!(s.focus_order(), vec![b, c, a]);
}

#[test]
fn z_index_sets_paint_order_not_layout() {
    let (d, b) = boxes(3);
    let s = scene(
        &d,
        ".b { height: 10px } .b:nth-child(1) { z-index: 5 } .b:nth-child(3) { z-index: -1 }",
        (400.0, 300.0),
    );
    assert_eq!(s.paint[0], vec![b[2], b[1], b[0]]);
    assert_eq!(s.kids[0], b);
    assert_eq!(r(&s, b[1])[1], 10.0);
}

#[test]
fn overflow_scroll_reports_content_size_and_scrolls() {
    let mut d = Doc::new("page");
    let sc = d.add(0, "div.sc");
    for _ in 0..10 {
        d.add(sc, "div.item");
    }
    let mut s = scene(
        &d,
        ".sc { height: 100px; overflow-y: auto } .item { height: 30px }",
        (400.0, 300.0),
    );
    assert_eq!(s.content[sc][1], 300.0);
    assert_eq!(r(&s, sc)[3], 100.0);
    s.wheel(Some(sc + 1), 0.0, -80.0, &mut approx);
    assert_eq!(s.scroll.offsets[&sc][1], 80.0);
    s.wheel(Some(sc + 1), 0.0, -5000.0, &mut approx);
    assert_eq!(s.scroll.offsets[&sc][1], 200.0);
    // The page itself does not move while the container can.
    assert_eq!(s.scroll.page_y, 0.0);
}

#[test]
fn horizontal_rows_scroll_sideways() {
    let mut d = Doc::new("page");
    let row = d.add(0, "div.row");
    for _ in 0..8 {
        d.add(row, "a.card");
    }
    let mut s = scene(
        &d,
        ".row { display: flex; overflow-x: scroll; width: 300px } .card { flex: none; width: 100px; height: 10px }",
        (400.0, 300.0),
    );
    assert_eq!(s.content[row][0], 800.0);
    assert!(s.wheel(Some(row + 1), -120.0, 0.0, &mut approx));
    assert_eq!(s.scroll.offsets[&row][0], 120.0);
}

#[test]
fn units_calc_min_max_clamp_and_env() {
    let (d, b) = boxes(1);
    let b = b[0];
    let vp = (1000.0, 500.0);
    let wd = |css: &str| scene(&d, &format!(".b {{ height: 5px; width: {css} }}"), vp).rects[b][2];
    assert_eq!(wd("calc(100vw - 2 * 80px - 16px)"), 824.0);
    assert_eq!(wd("min(340px, 70vw)"), 340.0);
    assert_eq!(wd("min(340px, 20vw)"), 200.0);
    assert_eq!(wd("max(10vh, 30px)"), 50.0);
    assert_eq!(wd("clamp(100px, 50vw, 400px)"), 400.0);
    assert_eq!(wd("calc(50% + 25%)"), 750.0);
    assert_eq!(wd("calc(10px + env(safe-area-inset-bottom, 4px))"), 14.0);
    assert_eq!(wd("calc(10px + env(safe-area-inset-bottom))"), 10.0);
    assert_eq!(wd("calc(2rem + 1em)"), 48.0);
    assert_eq!(wd("calc(-1 * 10px + 30px)"), 20.0);
    assert_eq!(wd("calc((100vw - 40px) / 4)"), 240.0);
    assert_eq!(wd("20vmin"), 100.0);
    assert_eq!(wd("10vmax"), 100.0);
    // % mixed with px cannot be resolved without the containing block: reported, the declaration is dropped.
    let s = scene(&d, ".b { width: calc(100% - 10px) }", vp);
    assert!(s.warnings().count(Kind::BadValue) >= 1);
}

#[test]
fn percent_aspect_ratio_and_min_max() {
    let (d, b) = boxes(1);
    let b = b[0];
    let s = scene(
        &d,
        ".b { width: 50%; aspect-ratio: 16 / 9 }",
        (800.0, 600.0),
    );
    assert_eq!((r(&s, b)[2], r(&s, b)[3]), (400.0, 225.0));
    let s = scene(
        &d,
        ".b { width: 80%; max-width: 300px; min-height: 40px; height: 10px }",
        (800.0, 600.0),
    );
    assert_eq!((r(&s, b)[2], r(&s, b)[3]), (300.0, 40.0));
}

#[test]
fn display_none_removes_the_box_and_its_subtree() {
    let (d, b) = boxes(2);
    let s = scene(
        &d,
        ".b { height: 10px } .b:nth-child(1) { display: none }",
        (400.0, 300.0),
    );
    assert_eq!((r(&s, b[0])[3], r(&s, b[1])[1]), (0.0, 0.0));
    assert!(s.is_hidden(b[0]));
}

#[test]
fn text_wraps_to_the_available_width() {
    let mut d = Doc::new("page");
    let p = d.add(0, "p.p");
    d.text(p, "one two three four five six seven eight");
    let s = scene(
        &d,
        ".p { width: 100px; font-size: 10px; line-height: 1 }",
        (400.0, 300.0),
    );
    // approx: 5px per character; 100px fits 20 characters: two lines of 10px.
    assert_eq!(r(&s, p)[3], 20.0);
    let s = scene(
        &d,
        ".p { width: 100px; font-size: 10px; line-height: 1; white-space: nowrap }",
        (400.0, 300.0),
    );
    assert_eq!(r(&s, p)[3], 10.0);
}

#[test]
fn images_are_replaced_elements_measured_from_their_intrinsic_size() {
    let mut d = Doc::new("page");
    let img = d.add(0, "img.poster");
    d.image(img, 7, 200, 100);
    let s = scene(&d, ".poster { }", (800.0, 600.0));
    assert_eq!((r(&s, img)[2], r(&s, img)[3]), (200.0, 100.0));
    let s = scene(&d, ".poster { width: 100px }", (800.0, 600.0));
    assert_eq!((r(&s, img)[2], r(&s, img)[3]), (100.0, 50.0));
    let s = scene(
        &d,
        ".poster { height: 20px; width: auto; display: flex }",
        (800.0, 600.0),
    );
    assert_eq!(r(&s, img)[3], 20.0);
}

#[test]
fn page_is_as_tall_as_its_content_and_at_least_the_window() {
    let (d, _) = boxes(3);
    let s = scene(&d, ".b { height: 500px }", (800.0, 600.0));
    assert_eq!(s.page_height(), 1500.0);
    let s = scene(&d, ".b { height: 10px }", (800.0, 600.0));
    assert_eq!(s.page_height(), 600.0);
}

#[test]
fn resize_relays_out_without_restyling_when_the_sheet_ignores_the_window() {
    let (d, b) = boxes(1);
    let mut s = scene(&d, ".b { width: 50%; height: 10px }", (800.0, 600.0));
    assert_eq!(r(&s, b[0])[2], 400.0);
    s.resize((600.0, 600.0), &mut approx);
    assert_eq!(r(&s, b[0])[2], 300.0);
    assert!(!s.sheet.uses_viewport);
}

#[test]
fn viewport_units_restyle_on_resize() {
    let (d, b) = boxes(1);
    let mut s = scene(&d, ".b { width: 50vw; height: 10px }", (800.0, 600.0));
    assert!(s.sheet.uses_viewport);
    s.resize((600.0, 600.0), &mut approx);
    assert_eq!(r(&s, b[0])[2], 300.0);
    assert_eq!(r(&s, 0)[2], 600.0, "the page follows the window");
}

#[test]
fn borders_padding_and_box_sizing() {
    let (d, b) = boxes(1);
    let s = scene(
        &d,
        ".b { width: 100px; height: 40px; padding: 4px 6px; border: 2px solid #fff }",
        (800.0, 600.0),
    );
    assert_eq!(r(&s, b[0])[2..], [100.0, 40.0]);
    assert_eq!(s.insets[b[0]], [8.0, 6.0, 8.0, 6.0]);
    let s = scene(
        &d,
        ".b { width: 100px; height: 40px; padding: 4px 6px; border: 2px solid #fff; box-sizing: content-box }",
        (800.0, 600.0),
    );
    assert_eq!(r(&s, b[0])[2..], [116.0, 52.0]);
    let s = scene(
        &d,
        ".b { border-left: 3px solid #f00; border-right: 0; height: 5px }",
        (800.0, 600.0),
    );
    assert_eq!(s.borders[b[0]], [3.0, 0.0, 0.0, 0.0]);
    assert_eq!(s.style(b[0]).border_color[3].r, 255);
}

#[test]
fn custom_font_family_resolves_against_installed_and_registered_fonts() {
    let (mut d, a, _, c) = small();
    d.inline(a, "font-family: 'Inter', 'Noto Sans', sans-serif");
    let mut fonts = Fonts::new();
    fonts.set_installed(["Noto Sans".to_string(), "DejaVu Sans".to_string()]);
    let build = |f: &Fonts| {
        Scene::build(
            d.clone(),
            Rc::new(Sheet::parse("")),
            Rc::new(f.clone()),
            (800.0, 600.0),
            Interaction::default(),
            None,
            &Builders::new(),
            &mut approx,
        )
        .unwrap()
    };
    assert_eq!(
        build(&fonts).text_style(c).font,
        FontChoice::Named("Noto Sans".into())
    );
    fonts.register("inter");
    assert_eq!(
        build(&fonts).text_style(c).font,
        FontChoice::Named("inter".into())
    );
    fonts.unregister("inter");
    assert_eq!(
        fonts.resolve(&["serif".into(), "Noto Sans".into()]),
        FontChoice::Serif
    );
    assert_eq!(fonts.resolve(&["monospace".into()]), FontChoice::Monospace);
    assert_eq!(fonts.resolve(&["Nope".into()]), FontChoice::Default);
}

#[test]
fn font_weight_keywords_numbers_and_relative() {
    let (d, a, _, c) = small();
    let wt = |css: &str, n: Id| scene(&d, css, (800.0, 600.0)).style(n).weight;
    assert_eq!(wt(".a { font-weight: bold }", a), 700);
    assert_eq!(wt(".a { font-weight: 450 }", c), 450);
    assert_eq!(
        wt(".a { font-weight: 300 } .c { font-weight: bolder }", c),
        400
    );
    assert_eq!(
        wt(".a { font-weight: 700 } .c { font-weight: lighter }", c),
        400
    );
    assert_eq!(wt(".a { font-weight: normal }", a), 400);
    assert!(
        scene(&d, ".a { font-weight: heavy }", (800.0, 600.0))
            .warnings()
            .count(Kind::BadValue)
            == 1
    );
}

#[test]
fn the_measure_cache_shapes_each_text_once() {
    let mut d = Doc::new("page");
    for i in 0..50 {
        let t = d.add(0, "p.p");
        d.text(
            t,
            if i % 2 == 0 {
                "same words here"
            } else {
                "other words there"
            },
        );
    }
    let mut cache = MeasureCache::new();
    let mut shaped = 0u32;
    let mut m = |t: &str, ts: &TextStyle, max: Option<f32>| {
        cache.measure(t, ts, max, &mut |t, ts, max| {
            shaped += 1;
            approx(t, ts, max)
        })
    };
    let s = Scene::build(
        d.clone(),
        Rc::new(Sheet::parse(".p { width: 90px }")),
        Rc::new(Fonts::new()),
        (800.0, 600.0),
        Interaction::default(),
        None,
        &Builders::new(),
        &mut m,
    )
    .unwrap();
    assert!(s.rects[1][3] > 0.0);
    // 50 paragraphs, two distinct strings: a handful of shaper calls, not one per paragraph and layout pass.
    assert!(shaped <= 8, "{shaped} shaper calls");
    assert!(cache.hits > 0 && cache.len() <= 8);
}

#[test]
fn font_shorthand() {
    let (d, a, ..) = small();
    let s = scene(
        &d,
        ".a { font: italic 600 13px/1.5 Inter, 'Noto Sans', sans-serif }",
        (800.0, 600.0),
    );
    let c = s.style(a);
    assert_eq!((c.font_size, c.weight, c.italic), (13.0, 600, true));
    assert_eq!(c.line_height, props::LineH::Mult(1.5));
    assert_eq!(
        &*c.family,
        &["Inter".to_string(), "Noto Sans".into(), "sans-serif".into()]
    );
    let s = scene(&d, ".a { font: 500 16px Inter }", (800.0, 600.0));
    assert_eq!((s.style(a).font_size, s.style(a).weight), (16.0, 500));
}

#[test]
fn html_and_body_at_a_hundred_percent_fill_the_window() {
    // jellyfin-web: `html, body { height: 100%; margin: 0 }`, a page that is `position: absolute` inside, `rem` from the root's `font-size: 93%`.
    let mut d = Doc::new("html");
    let body = d.add(0, "body");
    let main = d.add(body, "div.m");
    let s = scene(
        &d,
        "html, body { height: 100%; margin: 0; padding: 0 } html { font-size: 93% } .m { height: 10rem }",
        (800.0, 600.0),
    );
    assert_eq!(r(&s, body)[3], 600.0);
    assert_eq!(r(&s, main)[3], 160.0 * 0.93);
}

#[test]
fn an_absolute_box_is_placed_against_its_nearest_positioned_ancestor() {
    // jellyfin-web: `.mainAnimatedPage { position: absolute; top/right/bottom/left: 0 }` under static wrappers fills the window.
    let mut d = Doc::new("html");
    let wrap = d.add(0, "div.w");
    let page = d.add(wrap, "div.p");
    let inner = d.add(page, "div.i");
    let s = scene(
        &d,
        ".w { height: 0; padding-top: 50px } .p { position: absolute; top: 0; right: 0; bottom: 0; left: 0 } .i { position: absolute; top: 10px; left: 10px; width: 5px; height: 5px }",
        (800.0, 600.0),
    );
    assert_eq!(r(&s, page), [0.0, 0.0, 800.0, 600.0]);
    assert_eq!(
        r(&s, inner),
        [10.0, 10.0, 5.0, 5.0],
        "the page is positioned: its child is placed against it"
    );
}

#[test]
fn jellyfin_web_page_padding_rules_apply() {
    // `!important` padding in a media block, a `:not()` on the same element, `em` of the page's own font size, an absolute page with `contain` and vendor-prefixed flex.
    let mut d = Doc::new("html");
    let page = d.add(0, "div.page.libraryPage");
    let inner = d.add(page, "div.in");
    let s = scene(
        &d,
        ".page { bottom: 0; contain: layout style size; left: 0; position: absolute; right: 0; top: 0 } .libraryPage { padding-top: 7em !important } @media (min-width:100em){.libraryPage:not(.noSecondaryNavPage){padding-top:4.6em!important}} .in { height: 10px } html { font-size: 93% } .page { display: -webkit-flex; display: flex; -webkit-flex-direction: column; flex-direction: column }",
        (2000.0, 600.0),
    );
    assert_eq!(r(&s, inner)[1], 4.6 * 14.88);
}

/// Needs a jellyfin-web install: `JFWEB=/usr/share/jellyfin/web cargo test -p nocturne-style jf_real -- --ignored --nocapture`.
#[test]
#[ignore]
fn jf_real_css_home_page() {
    let dir = std::env::var("JFWEB").expect("JFWEB");
    let mut css = String::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let n = p.file_name().unwrap().to_string_lossy().to_string();
        if n.starts_with("main.jellyfin.") && n.ends_with(".css") {
            css += &std::fs::read_to_string(&p).unwrap();
        }
    }
    css += &std::fs::read_to_string(format!("{dir}/themes/dark/theme.css")).unwrap();
    let mut d = Doc::new("html");
    d.attr(0, "dir", "ltr");
    d.nodes[0].classes.push("layout-desktop".into());
    let body = d.add(0, "body");
    let main = d.add(body, "main");
    let skin = d.add(main, "div.skinBody");
    let page = d.add(skin, "div.page.mainAnimatedPage.homePage.libraryPage.allLibraryPage.pageWithAbsoluteTabs.withTabs");
    let tab = d.add(page, "div.tabContent.pageTabContent.is-active");
    let h = d.add(tab, "h2.sectionTitle.sectionTitle-cards.padded-left");
    d.text(h, "x");
    let s = scene(&d, &css, (1920.0, 1080.0));
    for n in [body, main, skin, page, tab, h] {
        eprintln!(
            "{} {:?} pad {:?}",
            s.doc.describe(n),
            s.rects[n],
            s.style(n).layout.padding
        );
    }
    for l in s.warnings().lines() {
        eprintln!("{l}");
    }
}

#[test]
fn a_var_with_a_fallback_after_a_plain_declaration_paints() {
    let mut d = Doc::new("html");
    let b = d.add(0, "body");
    let s = scene(
        &d,
        "html { background-color: #101010; background-color: var(--jf-palette-background-default, #101010); background-image: none; background-image: var(--jf-palette-background-defaultImage, none) } body { background-color: transparent !important }",
        (200.0, 100.0),
    );
    assert_eq!(s.style(0).bg.len(), 1, "the root's colour is painted");
    let _ = b;
}

#[test]
fn own_text_gets_its_own_box_beside_generated_bullets() {
    let mut d = Doc::new("page");
    let t = d.add(0, "h2.t");
    d.text(t, "Next Up");
    let s = scene(
        &d,
        ".t { font-size: 20px } .t::before { content: \"\"; display: inline-block; width: 6px; height: 6px; margin-right: 8px } .t::after { content: \"\"; display: inline-block; width: 6px; height: 6px }",
        (400.0, 300.0),
    );
    assert!(r(&s, t)[2] > 14.0 + 20.0, "width {}", r(&s, t)[2]);
    assert!(
        (s.insets[t][0] - 14.0).abs() < 0.5,
        "text starts after the bullet: {:?}",
        s.insets[t]
    );
}

#[test]
fn class_attribute_selectors_read_the_class_list() {
    let mut d = Doc::new("page");
    let b = d.add(0, "div.defaultCardBackground3.x");
    let s = scene(
        &d,
        "[class*=\"defaultCardBackground\"] { width: 33px; height: 5px }",
        (400.0, 300.0),
    );
    assert_eq!(r(&s, b)[2], 33.0);
}

/// jellyfin-web's `.indicator` (2em box, set before `.countIndicator { font-size: 88% }`).
#[test]
fn round_badge_em_size_follows_its_own_font_size() {
    let mut d = Doc::new("page");
    let b = d.add(0, "div.countIndicator.indicator");
    d.text(b, "12");
    let s = scene(
        &d,
        ".indicator{display:flex;align-items:center;justify-content:center;border-radius:100em;height:2em;width:2em} .countIndicator{font-size:88%}",
        (400.0, 300.0),
    );
    let fs = s.style(b).font_size;
    assert!((fs - 14.08).abs() < 0.01);
    // `em` is the element's own final font size, not the one in force when the declaration was read.
    assert_eq!((r(&s, b)[2], r(&s, b)[3]), (2.0 * fs, 2.0 * fs));
    assert_eq!(s.style(b).radius, [100.0 * fs; 4]);
}

/// An icon plus a label (`gap`/`margin` in em, set before the `font-size` that decides them) keeps its space.
#[test]
fn icon_and_label_keep_their_em_gap() {
    let mut d = Doc::new("page");
    let b = d.add(0, "div.dc");
    let i = d.add(b, "span.i");
    d.text(i, "\u{2661}");
    let l = d.add(b, "span.l");
    d.text(l, "Favourite");
    let s = scene(
        &d,
        ".dc { display: flex; align-items: center; gap: .5em } .l { margin-left: .25em } .dc, .l { font-size: 20px }",
        (400.0, 300.0),
    );
    // gap .5em + margin .25em of the label, both at 20 px
    let (ri, rl) = (r(&s, i), r(&s, l));
    assert!(
        (rl[0] - (ri[0] + ri[2]) - 15.0).abs() < 0.1,
        "{ri:?} {rl:?}"
    );
}

/// jellyfin-web's `.detailButton-icon { font-size: 1.6em !important; height: 1em; width: 1em }`: the icon box is as big as its enlarged glyph, so the label is not under it.
#[test]
fn important_font_size_sizes_the_icon_box_in_em() {
    let mut d = Doc::new("page");
    let b = d.add(0, "div.row");
    let i = d.add(b, "span.detailButton-icon");
    d.text(i, "\u{2661}");
    let l = d.add(b, "span.l");
    d.text(l, "Favourite");
    let s = scene(
        &d,
        ".row { display: flex } .detailButton-icon { font-size: 1.6em !important; height: 1em; width: 1em; margin-right: .25em } .detailButton-icon { font-size: 30px }",
        (400.0, 300.0),
    );
    // 1.6em of the parent's 16 px, not the later 30 px: 25.6 px square plus a .25em margin of 25.6 px.
    let (ri, rl) = (r(&s, i), r(&s, l));
    assert_eq!((ri[2], ri[3]), (25.6, 25.6));
    assert!((rl[0] - ri[2] - 6.4).abs() < 0.1, "{ri:?} {rl:?}");
}

/// The number in a round badge: the text of a flex container is an item of its own, placed by `justify-content` and `align-items`.
#[test]
fn flex_container_text_is_centred() {
    let mut d = Doc::new("page");
    let b = d.add(0, "div.indicator");
    d.text(b, "12");
    let s = scene(
        &d,
        ".indicator{display:flex;align-items:center;justify-content:center;height:40px;width:40px}",
        (400.0, 300.0),
    );
    let i = s.insets[b];
    assert!(i[0] > 1.0 && (i[0] - i[2]).abs() < 0.5, "{i:?}");
    assert!(i[1] > 1.0 && (i[1] - i[3]).abs() < 0.5, "{i:?}");
}
