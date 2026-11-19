use super::*;

fn p(css: &str) -> Parsed {
    parse(css).unwrap()
}

fn clean(css: &str) -> Parsed {
    let r = p(css);
    assert!(r.warnings.is_empty(), "{css}: {:?}", r.warnings);
    r
}

fn kinds(r: &Parsed) -> Vec<WarningKind> {
    r.warnings.iter().map(|w| w.kind).collect()
}

#[test]
fn colour_only_themes_have_an_empty_layout() {
    let r = clean(
        ":root{--nocturne-accent:#f06}.card{border-radius:6px;background:#111}.header{background:#000}button{border-radius:3px}",
    );
    assert_eq!(r.layout, Layout::default());
    assert_eq!(r.layout.count(), 0);
    assert_eq!(r.theme.radius_card, Some(6.0));
}

#[test]
fn card_properties() {
    let r = clean(
        ".card{aspect-ratio:16/9;width:280px;--title-position:overlay;box-shadow:0 4px 12px rgba(0,0,0,.5);transition-duration:.2s}",
    );
    let c = &r.layout.base.card;
    assert_eq!(c.aspect, Some(Aspect::Thumb));
    assert_eq!(c.width, Some(280.0));
    assert_eq!(c.title_position, Some(TitlePosition::Overlay));
    assert_eq!(c.transition_ms, Some(200));
    assert_eq!(
        r.layout.base.card_image.shadow,
        Some(Shadow {
            x: 0.0,
            y: 4.0,
            blur: 12.0,
            color: Color::rgba(0, 0, 0, 128)
        })
    );
}

#[test]
fn aspect_ratios_map_to_image_kinds() {
    for (v, want) in [
        ("2/3", Aspect::Poster),
        ("2 / 3", Aspect::Poster),
        ("16/9", Aspect::Thumb),
        ("1/1", Aspect::Square),
        ("1", Aspect::Square),
    ] {
        let r = clean(&format!(".card{{aspect-ratio:{v}}}"));
        assert_eq!(r.layout.base.card.aspect, Some(want), "{v}");
    }
    // Odd ratios snap to the nearest kind and say so.
    let r = p(".card{aspect-ratio:5/4}");
    assert_eq!(r.layout.base.card.aspect, Some(Aspect::Square));
    assert_eq!(kinds(&r), [WarningKind::Approximated]);
    assert_eq!(
        kinds(&p(".card{aspect-ratio:0}")),
        [WarningKind::UnsupportedValue]
    );
}

#[test]
fn card_size_alias_and_title_positions() {
    assert_eq!(
        clean(".card{--card-size:200px}").layout.base.card.width,
        Some(200.0)
    );
    for (v, want) in [
        ("below", TitlePosition::Below),
        ("overlay", TitlePosition::Overlay),
        ("none", TitlePosition::None),
    ] {
        let r = clean(&format!(".card{{--title-position:{v}}}"));
        assert_eq!(r.layout.base.card.title_position, Some(want));
    }
    assert_eq!(
        kinds(&p(".card{--title-position:above}")),
        [WarningKind::UnsupportedValue]
    );
}

#[test]
fn hover_scale_on_card_and_button_only() {
    let r = clean(".card:hover{transform:scale(1.08)} .button:hover{transform:scale(1.05)}");
    assert_eq!(r.layout.base.card.hover_scale, Some(1.08));
    assert_eq!(r.layout.base.button.hover_scale, Some(1.05));
    // The tag selector counts as the button part too.
    assert_eq!(
        clean("button:hover{transform:scale(1.1)}")
            .layout
            .base
            .button
            .hover_scale,
        Some(1.1)
    );
    // Anything else under :hover, and other pseudo-classes, still warn.
    for css in [
        ".row:hover{gap:4px}",
        ".header:hover{gap:4px}",
        ".card:focus{gap:4px}",
    ] {
        let r = p(css);
        assert_eq!(kinds(&r), [WarningKind::UnsupportedSelector], "{css}");
        assert_eq!(r.layout, Layout::default());
    }
    let r = p(".card:hover{opacity:.5;transform:scale(1.1)}");
    assert_eq!(kinds(&r), [WarningKind::UnsupportedProperty]);
    assert_eq!(r.layout.base.card.hover_scale, Some(1.1));
    // Not a hover rule: transform is refused with a reason, other forms too.
    assert_eq!(
        kinds(&p(".card{transform:scale(1.1)}")),
        [WarningKind::UnsupportedValue]
    );
    assert_eq!(
        kinds(&p(".card:hover{transform:rotate(3deg)}")),
        [WarningKind::UnsupportedValue]
    );
}

#[test]
fn clamping() {
    let r = p(".card:hover{transform:scale(2)} .card{transition-duration:5s;width:5000px}");
    assert_eq!(r.layout.base.card.hover_scale, Some(1.3));
    assert_eq!(r.layout.base.card.transition_ms, Some(1000));
    assert_eq!(r.layout.base.card.width, Some(640.0));
    assert_eq!(kinds(&r), [WarningKind::Clamped; 3]);
    let r = p(".card:hover{transform:scale(.5)} .card{width:10px;transition:opacity 0ms}");
    assert_eq!(r.layout.base.card.hover_scale, Some(1.0));
    assert_eq!(r.layout.base.card.width, Some(80.0));
    assert_eq!(r.layout.base.card.transition_ms, Some(0));
    let r = p(
        ".row{gap:500px;padding:500px;--row-columns:40} .hero{height:20px} .header{height:500px}",
    );
    let l = &r.layout.base;
    assert_eq!(
        (l.row.gap, l.row.padding, l.row.columns),
        (Some(64.0), Some(96.0), Some(12))
    );
    assert_eq!((l.hero.height, l.header.height), (Some(120.0), Some(120.0)));
    let r = p(".card-title{font-size:2px;font-weight:2000;letter-spacing:99px;line-height:9}");
    let t = &r.layout.base.card_title;
    assert_eq!(
        (t.font_size, t.font_weight, t.letter_spacing, t.line_height),
        (Some(8.0), Some(900), Some(10.0), Some(2.5))
    );
    assert_eq!(kinds(&r), [WarningKind::Clamped; 4]);
}

#[test]
fn transition_shorthand_reads_the_duration() {
    let r = p(".card{transition: transform 250ms ease-out}");
    assert_eq!(r.layout.base.card.transition_ms, Some(250));
    assert_eq!(kinds(&r), [WarningKind::Approximated]);
    assert_eq!(
        kinds(&p(".card{transition:all}")),
        [WarningKind::UnsupportedValue]
    );
}

#[test]
fn shadow_forms() {
    let sh = |css: &str| clean(css).layout.base.card_image.shadow.unwrap();
    assert_eq!(sh(".card-image{box-shadow:none}").color.a, 0);
    let s = sh(".card-image{box-shadow:-2px 3px 8px #ff0000}");
    assert_eq!(
        (s.x, s.y, s.blur, s.color),
        (-2.0, 3.0, 8.0, Color::rgba(255, 0, 0, 255))
    );
    assert_eq!(sh(".card-image{box-shadow:#000 0 2px}").blur, 0.0);
    assert_eq!(
        kinds(&p(".card-image{box-shadow:inset 0 0 4px red}")),
        [WarningKind::UnsupportedValue]
    );
    assert_eq!(
        kinds(&p(".card-image{box-shadow:0 0 4px 2px red}")),
        [WarningKind::Approximated]
    );
}

#[test]
fn card_radius_is_global_at_base_and_per_scope_otherwise() {
    let r = clean(
        ".card{border-radius:9px} .page-home .card{border-radius:14px} .card-image{border-radius:2px}",
    );
    assert_eq!(r.theme.radius_card, Some(9.0));
    assert_eq!(r.layout.pages[&Page::Home].card_image.radius, Some(14.0));
    assert_eq!(r.layout.base.card_image.radius, Some(2.0));
}

#[test]
fn row_properties() {
    let r = clean(".row{display:grid;gap:12px;--row-columns:5;padding:32px}");
    let w = &r.layout.base.row;
    assert_eq!(w.layout, Some(RowLayout::Grid));
    assert_eq!(
        (w.gap, w.columns, w.padding, w.hidden),
        (Some(12.0), Some(5), Some(32.0), Some(false))
    );
    assert_eq!(
        clean(".row{display:carousel}").layout.base.row.layout,
        Some(RowLayout::Carousel)
    );
    assert_eq!(
        clean(".row{display:flex}").layout.base.row.layout,
        Some(RowLayout::Carousel)
    );
    assert_eq!(
        clean(".row{--row-layout:grid}").layout.base.row.layout,
        Some(RowLayout::Grid)
    );
    assert_eq!(
        kinds(&p(".row{--row-layout:masonry}")),
        [WarningKind::UnsupportedValue]
    );
}

#[test]
fn hero_properties() {
    let r = clean(
        ".hero{--hero-style:banner;height:320px;background:linear-gradient(to top,#000,transparent)}",
    );
    let h = &r.layout.base.hero;
    assert_eq!(
        (h.style, h.height, h.hidden),
        (Some(HeroStyle::Banner), Some(320.0), Some(false))
    );
    assert!(matches!(h.overlay, Some(Fill::Linear { .. })));
    assert_eq!(
        clean(".hero{--hero-style:none}").layout.base.hero.hidden,
        Some(true)
    );
    assert_eq!(
        clean(".hero{display:none}").layout.base.hero.hidden,
        Some(true)
    );
}

#[test]
fn header_properties_and_items() {
    let r = clean(".header{background:transparent;position:fixed;height:64px}");
    assert_eq!(r.theme.header, Some(Color::rgba(0, 0, 0, 0)));
    assert_eq!(r.layout.base.header.position, Some(HeaderPosition::Fixed));
    assert_eq!(r.layout.base.header.height, Some(64.0));
    assert_eq!(
        clean(".header{position:static}")
            .layout
            .base
            .header
            .position,
        Some(HeaderPosition::Static)
    );
    let b = clean(".header{--header-items: nav, search}").layout.base;
    assert_eq!(
        [
            b.header_nav.hidden,
            b.header_search.hidden,
            b.header_history.hidden,
            b.header_account.hidden
        ],
        [Some(false), Some(false), Some(true), Some(true)]
    );
    let r = p(".header{--header-items:nav tabs}");
    assert_eq!(kinds(&r), [WarningKind::UnsupportedValue]);
    let b = clean(".header-search{display:none} .header-nav{display:flex}")
        .layout
        .base;
    assert_eq!(
        (b.header_search.hidden, b.header_nav.hidden),
        (Some(true), Some(false))
    );
}

#[test]
fn typography_per_part() {
    let r = clean(
        ".row-title{font-size:20px;font-weight:bold;text-transform:uppercase;letter-spacing:-.5px;line-height:1.2;color:#eee}",
    );
    let t = &r.layout.base.row_title;
    assert_eq!(t.font_size, Some(20.0));
    assert_eq!(t.font_weight, Some(700));
    assert_eq!(t.text_transform, Some(TextTransform::Uppercase));
    assert_eq!(t.letter_spacing, Some(-0.5));
    assert_eq!(t.line_height, Some(1.2));
    assert_eq!(t.color, Some(Color::rgba(0xee, 0xee, 0xee, 255)));
    assert_eq!(
        clean(".card-meta{display:none}")
            .layout
            .base
            .card_meta
            .hidden,
        Some(true)
    );
    for sel in [
        "card-title",
        "hero-title",
        "hero-meta",
        "header-link",
        "detail-title",
        "detail-text",
    ] {
        assert_eq!(
            clean(&format!(".{sel}{{font-weight:600}}")).layout.count(),
            1,
            "{sel}"
        );
    }
    assert_eq!(
        kinds(&p(".card-title{text-transform:wavy}")),
        [WarningKind::UnsupportedValue]
    );
}

#[test]
fn display_none_hides_and_other_display_shows() {
    let r = clean(".card{display:none} .row-title{display:none}");
    assert_eq!(r.layout.base.card.hidden, Some(true));
    assert_eq!(r.layout.base.row_title.hidden, Some(true));
    let r = clean(".card{display:none} .card{display:block}");
    assert_eq!(r.layout.base.card.hidden, Some(false));
    // The player controls cannot be hidden, but can be tinted.
    let r = p(".player-controls{display:none;background:rgba(0,0,0,.6)}");
    assert_eq!(kinds(&r), [WarningKind::UnsupportedValue]);
    assert_eq!(
        r.layout.base.player_controls.background,
        Some(Fill::Solid(Color::rgba(0, 0, 0, 153)))
    );
}

#[test]
fn sections_order_and_hide() {
    let r =
        clean(".section-latest{order:-1} .section-resume{display:none} .section-next-up{order:5}");
    assert_eq!(
        r.layout.section_order(),
        [Section::Latest, Section::Resume, Section::NextUp]
    );
    assert_eq!(
        r.layout.parts(None, Some(Section::Resume)).row.hidden,
        Some(true)
    );
    assert_eq!(r.layout.parts(None, Some(Section::Latest)).row.hidden, None);
    // Without `order` the built-in sequence stays.
    assert_eq!(Layout::default().section_order(), Section::ALL);
    // `order` means nothing outside a section.
    assert_eq!(kinds(&p(".row{order:1}")), [WarningKind::UnsupportedValue]);
    assert_eq!(
        kinds(&p(".section-latest{order:500}")),
        [WarningKind::Clamped]
    );
}

#[test]
fn page_and_section_scopes_layer_over_the_base() {
    let r = clean(
        ".card{width:200px;aspect-ratio:2/3} .page-library .card{aspect-ratio:1/1} .section-resume .card{aspect-ratio:16/9;width:300px}",
    );
    let home = r.layout.parts(Some(Page::Home), Some(Section::Latest));
    assert_eq!(
        (home.card.aspect, home.card.width),
        (Some(Aspect::Poster), Some(200.0))
    );
    let lib = r.layout.parts(Some(Page::Library), None);
    assert_eq!(
        (lib.card.aspect, lib.card.width),
        (Some(Aspect::Square), Some(200.0))
    );
    let resume = r.layout.parts(Some(Page::Home), Some(Section::Resume));
    assert_eq!(
        (resume.card.aspect, resume.card.width),
        (Some(Aspect::Thumb), Some(300.0))
    );
}

#[test]
fn scope_errors_warn() {
    for (css, why) in [
        (".page-home{gap:4px}", "scope"),
        (".page-nowhere .card{gap:4px}", "unknown page"),
        (".section-x .card{gap:4px}", "unknown section"),
        (".page-home .nothing{gap:4px}", "unknown part"),
        (".page-home .card .row{gap:4px}", "descendant"),
    ] {
        let r = p(css);
        assert_eq!(kinds(&r), [WarningKind::UnsupportedSelector], "{css}");
        assert!(
            r.warnings[0].message.contains(why),
            "{css}: {}",
            r.warnings[0].message
        );
        assert_eq!(r.layout, Layout::default(), "{css}");
    }
}

#[test]
fn properties_outside_the_vocabulary_warn() {
    let r = p(
        ".card{margin:4px;position:absolute;display:inline-grid} .hero{--hero-style:wide;top:4px}",
    );
    assert_eq!(
        kinds(&r),
        [
            WarningKind::UnsupportedProperty,
            WarningKind::UnsupportedProperty,
            WarningKind::UnsupportedValue,
            WarningKind::UnsupportedProperty,
        ]
    );
    // A custom property on a part that does not read it.
    assert_eq!(
        kinds(&p(".card{--row-columns:3}")),
        [WarningKind::UnsupportedProperty]
    );
    // Rejected values leave no empty scope behind.
    assert_eq!(p(".page-home .card{margin:1px}").layout, Layout::default());
}

#[test]
fn var_substitution_reaches_layout_values() {
    let r = clean(":root{--w:260px;--k:grid} .card{width:var(--w)} .row{display:var(--k)}");
    assert_eq!(r.layout.base.card.width, Some(260.0));
    assert_eq!(r.layout.base.row.layout, Some(RowLayout::Grid));
    assert_eq!(
        kinds(&p(".card{width:var(--nope)}")),
        [WarningKind::UnresolvedVar]
    );
    assert_eq!(
        kinds(&p(".card{width:url(x)}")),
        [WarningKind::RemoteResource]
    );
}

#[test]
fn detail_backdrop_is_a_backdrop_alias() {
    let r = clean(".detail-backdrop{opacity:.5;filter:blur(8px)}");
    assert_eq!(
        (r.theme.backdrop_opacity, r.theme.backdrop_blur),
        (Some(0.5), Some(8.0))
    );
}

#[test]
fn overlay_merges_layers() {
    let mut a = clean(".card{width:200px;--title-position:below} .page-home .row{gap:8px}").layout;
    let b =
        clean(".card{width:300px} .page-home .row{display:grid} .section-latest{order:2}").layout;
    a.overlay(&b);
    assert_eq!(a.base.card.width, Some(300.0));
    assert_eq!(a.base.card.title_position, Some(TitlePosition::Below));
    let home = &a.pages[&Page::Home].row;
    assert_eq!((home.gap, home.layout), (Some(8.0), Some(RowLayout::Grid)));
    assert_eq!(a.sections[&Section::Latest].row.order, Some(2));
}

#[test]
fn presets_parse_without_warnings() {
    assert_eq!(PRESETS.len(), 2);
    for (name, css) in PRESETS {
        let r = clean(css);
        assert!(r.theme.count() >= 10, "{name}");
        assert!(r.layout.count() >= 10, "{name}");
        assert!(!css.to_lowercase().contains("netflix") && !css.to_lowercase().contains("youtube"));
    }
}

#[test]
fn marquee_is_billboard_carousels_with_hover_zoom() {
    let r = clean(PRESETS[0].1);
    let l = &r.layout.base;
    assert_eq!(l.hero.style, Some(HeroStyle::Billboard));
    assert_eq!(l.hero.height, Some(720.0));
    assert!(matches!(l.hero.overlay, Some(Fill::Linear { .. })));
    assert_eq!(l.header.position, Some(HeaderPosition::Fixed));
    assert_eq!(r.theme.header, Some(Color::rgba(0, 0, 0, 0)));
    assert_eq!(l.row.layout, Some(RowLayout::Carousel));
    assert_eq!(l.card.aspect, Some(Aspect::Thumb));
    assert_eq!(l.card.title_position, Some(TitlePosition::None));
    assert_eq!(l.card.hover_scale, Some(1.1));
    assert_eq!(l.card.transition_ms, Some(200));
    assert_eq!(l.button.hover_scale, Some(1.05));
    assert_eq!(r.theme.accent, Some(Color::rgba(0xd6, 0x28, 0x39, 255)));
}

#[test]
fn feed_is_a_titled_grid_without_hero() {
    let r = clean(PRESETS[1].1);
    let l = &r.layout.base;
    assert_eq!(l.hero.hidden, Some(true));
    assert_eq!(l.row.layout, Some(RowLayout::Grid));
    assert_eq!(l.card.aspect, Some(Aspect::Thumb));
    assert_eq!(l.card.title_position, Some(TitlePosition::Below));
    assert_eq!(l.card_image.radius, Some(12.0));
    assert_eq!(l.header.height, Some(48.0));
    assert_eq!(l.header.position, Some(HeaderPosition::Static));
    assert_eq!(l.card_title.font_size, Some(15.0));
    assert_eq!(
        r.layout.pages[&Page::Detail].detail_title.font_weight,
        Some(700)
    );
}

#[test]
fn hostile_layout_input_never_panics() {
    for css in [
        ".card{width:}",
        ".card{aspect-ratio:/}",
        ".card{aspect-ratio:1/0}",
        ".card{transition:,,}",
        ".card:hover{transform:scale()}",
        ".card:hover{transform:scale(,)}",
        ".card-image{box-shadow:1px}",
        ".row{--row-columns:nan}",
        ".card-title{letter-spacing:-}",
        ".page-home .page-home{gap:1px}",
        ".section-latest:hover{order:1}",
    ] {
        let _ = p(css);
    }
}
