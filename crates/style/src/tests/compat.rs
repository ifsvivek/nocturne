//! The jellyfin-web compatibility layer: mapping families, structure collapsing, variables, the coverage of a hand-written Jellyfin-style sheet, the engine flag, and (ignored, needs themes fetched into a directory) the measurement over real themes.
use crate::compat::{Compat, translate};
use crate::sheet::Sheet;
use crate::*;

/// The selector a single-rule sheet comes out with, or `None` when it was lost.
fn tr(sel: &str) -> Option<String> {
    let (out, c) = translate(&format!("{sel}{{color:red}}"));
    if c.applied && c.translated == 0 {
        return None;
    }
    Some(out.split_once('{').unwrap().0.trim().to_owned())
}

fn lost(sel: &str) -> bool {
    tr(sel).is_none()
}

/// The translation without the specificity padding (`specificity_survives_dropped_wrappers` covers that).
fn sel(sel: &str) -> String {
    tr(sel)
        .unwrap_or_else(|| panic!("`{sel}` was lost"))
        .replace(":not(.jf-pad)", "")
}

#[test]
fn header_family() {
    assert_eq!(sel(".skinHeader"), ".header");
    assert_eq!(
        sel(".skinHeader .headerTabs .emby-tab-button"),
        ".header .header-nav .header-link"
    );
    assert_eq!(sel(".emby-tab-button-active"), ".header-link.active");
    assert_eq!(
        sel(".headerRight .headerSearchButton"),
        ".header-icons .header-search"
    );
    assert_eq!(
        sel(".headerBackButton"),
        ".header-brand > .slot:first-child"
    );
    assert_eq!(sel(".pageTitle"), ".header-name");
    // `.skinHeader.semiTransparent` is one element: the modifier collapses into the same class.
    assert_eq!(sel(".skinHeader.semiTransparent"), ".header");
    assert!(lost(".headerCastButton"));
    assert!(lost(".mainDrawer .navMenuOption"));
}

#[test]
fn home_sections_and_rows() {
    assert_eq!(
        sel(".sectionTitle-cards"),
        ":is(.row-title, .section-title)"
    );
    assert_eq!(
        sel(".homeSectionsContainer .verticalSection .sectionTitleContainer .sectionTitle"),
        ".page-home :is(.row, .section) :is(.row-head, .section-head) :is(.row-title, .section-title)"
    );
    assert_eq!(
        sel(".itemsContainer.vertical-wrap"),
        ":is(.row-cards, .section-strip).row-grid"
    );
    // Wrappers with no element drop out of a chain and the descendant relation stays.
    assert_eq!(
        sel(".verticalSection .padded-left .sectionTitle"),
        ":is(.row, .section) :is(.row-title, .section-title)"
    );
    assert_eq!(
        sel(".verticalSection > .focuscontainer-x > .card"),
        ":is(.row, .section) :is(.card, .poster)"
    );
}

#[test]
fn card_family_collapses() {
    // cardBox, cardScalable and card are one element in the trees.
    assert_eq!(sel(".card .cardBox"), ":is(.card, .poster)");
    assert_eq!(sel(".cardBox > .cardScalable"), ":is(.card, .poster)");
    assert_eq!(
        sel(".cardScalable > .cardImageContainer"),
        ":is(.card, .poster) :is(.card-image, .poster-art)"
    );
    // The padder is a spacer with no element: `.cardPadder + .cardImageContainer` is the image alone.
    assert_eq!(
        sel(".cardBox > .cardScalable > .cardPadder + .cardImageContainer"),
        ":is(.card, .poster) :is(.card-image, .poster-art)"
    );
    assert_eq!(
        sel(".cardBox .cardFooter .cardText-first"),
        ":is(.card, .poster) :is(.card-caption, .poster-caption) :is(.card-title, .poster-title)"
    );
    assert_eq!(sel(".cardText-secondary"), ":is(.card-meta, .poster-meta)");
    assert_eq!(sel(".cardContent"), ":is(.card-image, .poster-art)");
    assert_eq!(
        sel(".itemProgressBar .itemProgressBarForeground"),
        ":is(.card-progress, .card-bar) .card-progress-fill"
    );
    assert_eq!(sel(".countIndicator"), ".card-badge");
    assert_eq!(
        sel(".cardOverlayContainer"),
        ":is(.card, .poster):hover .card-ring"
    );
    // Shapes are data on the card.
    assert_eq!(
        sel(".overflowBackdropCard"),
        ":is(.card, .poster)[data-shape=backdrop]"
    );
    assert_eq!(
        sel(".card.squareCard .cardText"),
        ":is(.card, .poster)[data-shape=square] :is(.card-title, .card-meta, .poster-title, .poster-meta)"
    );
    assert_eq!(sel(".cardOverlayButton"), ".card-play");
    assert_eq!(
        sel(".card .cardOverlayButton-hover"),
        ":is(.card, .poster) .card-play"
    );
    assert_eq!(
        sel(".playedIndicator"),
        ".card-chip[data-action=watched].on"
    );
}

#[test]
fn data_attributes() {
    assert_eq!(
        sel(".card[data-type=Movie]"),
        ":is(.card, .poster)[data-kind=movie]"
    );
    assert_eq!(
        sel(".card[data-type=\"Series\"]"),
        ":is(.card, .poster)[data-kind=series]"
    );
    assert_eq!(sel("[data-type=BoxSet]"), "[data-kind=folder]");
    assert_eq!(sel("[data-mediatype=Photo]"), "[data-kind=photo]");
    assert_eq!(
        sel(".card[data-isfolder=true]"),
        ":is(.card, .poster):is([data-kind=folder], [data-kind=series], [data-kind=season])"
    );
    // NOCTURNE's own attributes pass; Jellyfin's data-action values (resume, link) do not.
    assert_eq!(
        sel(".cardBox[data-watched=true]"),
        ":is(.card, .poster)[data-watched=true]"
    );
    assert!(lost(".cardBox[data-action=resume]"));
    assert!(lost(".cardBox[data-id]"));
    assert!(lost(".cardBox[class*=Mui]"));
}

#[test]
fn detail_family() {
    assert_eq!(
        sel(".itemDetailPage .itemName"),
        ".page-detail .detail-title"
    );
    assert_eq!(
        sel(".detailPagePrimaryContainer .detailImageContainer"),
        ".detail-hero .detail-poster"
    );
    assert_eq!(
        sel(".mainDetailButtons .btnPlay"),
        ".detail-actions .button[data-action=play]"
    );
    assert_eq!(sel(".overview-text"), ".detail-text");
    assert_eq!(
        sel(".detailSection .detailSectionHeader"),
        ".section .section-head"
    );
    assert_eq!(
        sel(".listItem:hover .listItemBodyText"),
        ":is(.episode, .entry):hover :is(.episode-title, .episode-meta, .entry-title, .entry-meta)"
    );
    assert_eq!(sel(".backdropImage"), ":is(.detail-backdrop, .hero-img)");
    assert_eq!(sel(".trackSelections"), ".detail-tracks");
    assert_eq!(sel(".parentName"), ".detail-parent");
}

#[test]
fn player_family() {
    assert_eq!(sel(".osdTextContainer"), ".player-info");
    assert_eq!(sel(".btnPause"), ".player-btn[data-action=pause]");
    assert_eq!(sel(".osdPositionSliderContainer"), ".player-seek");
    assert_eq!(sel(".osdControls"), ".player-bar");
    assert!(lost(".videoOsdBottom .btnCast"));
}

#[test]
fn root_and_layout_classes() {
    assert_eq!(sel("html .cardBox"), ":root :is(.card, .poster)");
    assert_eq!(sel("body.layout-desktop"), ":root");
    assert_eq!(sel(".layout-desktop .card"), ":root :is(.card, .poster)");
    assert_eq!(
        sel(".layout-tv .card"),
        ":root[data-layout=tv] :is(.card, .poster)"
    );
    assert!(lost(".layout-mobile .card"));
    assert_eq!(sel(".backgroundContainer"), ":root");
}

#[test]
fn pseudo_lists_and_functions() {
    assert_eq!(sel(".emby-button:hover"), ".button:hover");
    assert_eq!(sel(".card:not(.hide):hover"), ":is(.card, .poster):hover");
    assert_eq!(sel(".emby-button:not(:disabled):focus"), ".button:focus");
    assert_eq!(
        sel(".emby-button:not(.button-flat):not(:disabled)"),
        ".button:not(.button)"
    );
    assert_eq!(
        sel(":is(.cardText, .cardFooter) > a"),
        ":is(:is(.card-title, .card-meta, .poster-title, .poster-meta), :is(.card-caption, .poster-caption)) a"
    );
    assert_eq!(
        sel(".skinHeader:has(.headerTabs)"),
        ".header:has(.header-nav)"
    );
    assert_eq!(
        sel(".cardImageContainer::before"),
        ":is(.card-image, .poster-art)::before"
    );
    assert!(lost(".emby-input::placeholder"));
    assert!(lost(".emby-checkbox:checked"));
    // One member of a list that cannot match is dropped from `:is`, the rest stays.
    assert_eq!(
        sel(":is(.card, .emby-checkbox)"),
        ":is(:is(.card, .poster))"
    );
}

#[test]
fn native_classes_pass_through() {
    assert_eq!(sel(".card-title.accent"), ".card-title.accent");
    assert_eq!(sel(".hero .hero-title"), ".hero .hero-title");
    assert!(lost(".skinHeader .some-plugin-class"));
}

#[test]
fn specificity_survives_dropped_wrappers() {
    // `.verticalSection .padded-left .sectionTitle` loses a class; padding keeps its weight against a plain `.sectionTitle` rule.
    let (out, _) = translate(
        ".sectionTitle{color:red} .verticalSection .padded-left .sectionTitle{color:blue} .skinHeader{x:y}",
    );
    let rules: Vec<&str> = out.lines().collect();
    let long = rules.iter().find(|l| l.contains("color:blue")).unwrap();
    let a = select_spec(":is(.row, .section) :is(.row-title, .section-title)");
    let b = select_spec(long.split_once('{').unwrap().0.trim());
    assert!(b >= a);
    assert!(
        b >= select_spec(".verticalSection .padded-left .sectionTitle"),
        "{long}"
    );
}

fn select_spec(s: &str) -> u32 {
    crate::select::parse(s).unwrap().specificity()
}

#[test]
fn child_degrades_to_descendant_and_is_counted() {
    let (out, c) = translate(".skinHeader > .headerTabs{color:red}");
    assert!(out.starts_with(".header .header-nav"), "{out}");
    assert_eq!(c.child_to_descendant, 1);
}

#[test]
fn variables_are_mirrored_and_defaulted() {
    let (out, c) = translate(
        ":root{--jf-palette-primary-main:#e5a00d;--accentColor:#e5a00d} .emby-button{color:var(--jf-palette-text-primary);border-color:var(--jf-palette-primary-main)}",
    );
    assert!(out.contains("--nocturne-accent:#e5a00d"), "{out}");
    assert_eq!(c.vars_mirrored, 2);
    // Read but never set: defaults to the NOCTURNE token, defined first so a later definition wins.
    assert!(
        out.starts_with(":root{--jf-palette-text-primary: var(--nocturne-text);}"),
        "{out}"
    );
    assert_eq!(c.vars_defaulted, 1);
}

#[test]
fn a_nocturne_sheet_is_left_alone() {
    for (name, css) in STARTERS.iter().chain(THEMES) {
        let (out, c) = translate(css);
        assert!(!c.applied, "{name} was rewritten");
        assert_eq!(out, *css);
    }
}

const SAMPLE: &str = "
:root { --jf-palette-primary-main: #e5a00d; --jf-palette-background-default: #101010; --jf-card-borderRadius: 14px; }
html, body { background: #101010; color: #eee; }
.skinHeader { background: rgba(0,0,0,.6); backdrop-filter: blur(10px); }
.skinHeader .headerTabs .emby-tab-button { color: #aaa; }
.emby-tab-button-active { color: #fff; }
.headerRight .headerButton:hover { color: red; }
.sectionTitle-cards { font-size: 1.4em; }
.homeSectionsContainer .verticalSection .sectionTitleContainer-cards .sectionTitle { margin: 0; }
.itemsContainer > .card { margin: 4px; }
.card .cardBox { border-radius: 8px; }
.cardBox > .cardScalable > .cardPadder + .cardImageContainer { border-radius: 8px; }
.cardScalable:hover .cardImageContainer { transform: scale(1.04); }
.cardFooter .cardText-first { font-weight: 600; }
.cardText-secondary { opacity: .7; }
.itemProgressBar { height: 4px; }
.itemProgressBarForeground { background: #e5a00d; }
.countIndicator { background: red; }
.card[data-type=Movie] .cardText { color: gold; }
.overflowBackdropCard { width: 20em; }
.layout-desktop .detailImageContainer { width: 300px; }
.itemName { font-size: 3em; }
.detailPagePrimaryContainer .mainDetailButtons .btnPlay { background: #e5a00d; }
.listItem:hover .listItemBodyText { color: #fff; }
.card:not(.cardOverlayButton-hover):hover { transform: scale(1.05); }
.osdControls { background: black; }
.cardOverlayButton { opacity: 0; }
.emby-checkbox:checked + span { color: red; }
.somePluginThing { color: red; }
input[type=text] { color: red; }
";

#[test]
fn a_jellyfin_style_sheet_mostly_translates() {
    let (out, c) = translate(SAMPLE);
    assert!(c.applied);
    assert_eq!(c.selectors, 30, "{c:?}");
    // The rules about things NOCTURNE does not have, and nothing else, are lost.
    assert_eq!(c.selectors - c.translated, 3, "{:?}", c.groups());
    assert!(c.percent_selectors() > 80.0);
    let reasons: Vec<&str> = c.groups().iter().map(|g| g.0).collect();
    assert!(
        reasons.contains(&"class with no entry in the map"),
        "{reasons:?}"
    );
    // The translated sheet is readable by the engine: nothing is reported as an unsupported selector.
    let sheet = Sheet::parse(&out);
    assert_eq!(
        sheet.report.count(Kind::UnsupportedSelector),
        0,
        "{:?}\n{out}",
        sheet.report.lines()
    );
    assert!(sheet.rule_count() >= 25, "{}", sheet.rule_count());
}

#[test]
fn the_engine_flag_switches_the_translation() {
    let css = ".skinHeader{background:red} .itemName{color:blue}";
    let mut e = Engine::new(include_str!("../../themes/default.css"));
    e.set_user_css(Some(css));
    assert!(e.compat_report().is_some_and(|c| c.translated == 2));
    assert!(e.active_sheet().rules.iter().any(|r| &*r.text == ".header"));
    assert_eq!(e.active_sheet().report.count(Kind::Compat), 0);
    e.set_compat(false);
    e.set_user_css(Some(css));
    assert!(e.compat_report().is_none());
    assert!(
        e.active_sheet()
            .rules
            .iter()
            .any(|r| &*r.text == ".skinHeader")
    );
    // Lost selectors show up in the sheet's own report.
    e.set_compat(true);
    e.set_user_css(Some(
        ".skinHeader{background:red} .headerCastButton{color:red}",
    ));
    assert_eq!(e.active_sheet().report.count(Kind::Compat), 1);
}

#[test]
fn hostile_input_does_not_panic() {
    for css in [
        "{",
        "}",
        ".a{",
        "@media (",
        ".a,,,{x:y}",
        ".card:not(",
        ".a>{x:y}",
        "[data-type=Movie",
        ".card:is(",
        "\u{0}.x{y:z}",
        "@media screen{.skinHeader{",
        ":not(:not(:not(.a)))",
    ] {
        let _ = translate(css);
        let _ = translate(&css.repeat(50));
    }
}

/// `NOCTURNE_COMPAT_THEMES=/tmp/compat-themes cargo test -p nocturne-style compat_measure -- --ignored --nocapture`: the numbers of `docs/dev/THEME_COMPAT.md`. The directory holds one fetched `.css` per theme; nothing from them is stored.
#[test]
#[ignore]
fn compat_measure() {
    let Some(dir) = std::env::var_os("NOCTURNE_COMPAT_THEMES") else {
        return;
    };
    // Declarations the engine compiled, once per rule; `plain` keeps only rules whose selector names no class, id or attribute (what could match without any translation).
    let applied = |s: &Sheet, plain: bool| {
        let mut seen = std::collections::HashSet::new();
        s.rules
            .iter()
            .filter(|r| !plain || !r.text.contains(['.', '#', '[']))
            .filter(|r| seen.insert(std::rc::Rc::as_ptr(&r.decls) as *const u8))
            .map(|r| {
                r.decls
                    .iter()
                    .filter(|d| !d.name.starts_with("--nocturne-"))
                    .count()
            })
            .sum::<usize>()
    };
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "css"))
        .collect();
    names.sort();
    let (mut tot, mut all_unknown) = (
        Compat::default(),
        std::collections::BTreeMap::<String, usize>::new(),
    );
    let (mut reasons, mut tot_un, mut tot_before, mut tot_after) =
        (std::collections::BTreeMap::<String, usize>::new(), 0, 0, 0);
    println!(
        "| Theme | Selectors | Translated | No counterpart by design | Name not in the map | Declarations | In translated rules | Compiled by the engine | Without translation |"
    );
    for p in names {
        let css = std::fs::read_to_string(&p).unwrap();
        let before = Sheet::parse(&css);
        let (out, c) = translate(&css);
        let after = Sheet::parse(&out);
        let (b, a) = (applied(&before, true), applied(&after, false));
        let by_design = c.selectors - c.translated - c.unmapped();
        println!(
            "| {} | {} | {:.0}% | {:.0}% | {:.0}% | {} | {:.0}% | {:.0}% | {:.0}% |",
            p.file_stem().unwrap().to_string_lossy(),
            c.selectors,
            c.percent_selectors(),
            pct(by_design, c.selectors),
            pct(c.unmapped(), c.selectors),
            c.decls,
            c.percent_decls(),
            pct(a, c.decls),
            pct(b, c.decls),
        );
        tot_un += c.unmapped();
        tot_before += b;
        tot_after += a;
        for (why, n, d) in c.groups() {
            *reasons.entry(why.to_owned()).or_default() += n;
            for (k, n) in d {
                *all_unknown.entry(format!("{why} | {k}")).or_default() += n;
            }
        }
        tot.selectors += c.selectors;
        tot.translated += c.translated;
        tot.decls += c.decls;
        tot.decls_reachable += c.decls_reachable;
    }
    println!(
        "TOTAL unmapped {:.1}% compiled {:.1}% without translation {:.1}%",
        pct(tot_un, tot.selectors),
        pct(tot_after, tot.decls),
        pct(tot_before, tot.decls)
    );
    println!(
        "TOTAL selectors {} translated {} ({:.1}%) decls {} reachable {} ({:.1}%)",
        tot.selectors,
        tot.translated,
        tot.percent_selectors(),
        tot.decls,
        tot.decls_reachable,
        tot.percent_decls()
    );
    let mut r: Vec<_> = reasons.into_iter().collect();
    r.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in r.iter().take(25) {
        println!("REASON {n:5} {k}");
    }
    let mut u: Vec<_> = all_unknown.into_iter().collect();
    u.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in u.iter().take(260) {
        println!("DETAIL {n:5} {k}");
    }
}

fn pct(a: usize, b: usize) -> f32 {
    if b == 0 {
        100.0
    } else {
        100.0 * a as f32 / b as f32
    }
}

/// A Jellyfin theme restyles the real elements of the trees: header, section title, a card's caption and image.
#[test]
fn a_jellyfin_theme_restyles_the_trees_through_the_engine() {
    use super::default_sheet::default_css;
    use nocturne_core::customize::value::Color;
    let mut d = Doc::new("page");
    d.nodes[0].classes.push("page-home".into());
    let header = d.add(0, "header.header");
    let bar = d.add(header, "div.header-bar");
    let left = d.add(bar, "div.header-left");
    let nav = d.add(left, "nav.header-nav");
    let tab = d.add(nav, "a.header-link.active");
    let row = d.add(0, "section.row.section-latest");
    let head = d.add(row, "div.row-head");
    let title = d.add(head, "div.row-title");
    let cards = d.add(row, "div.row-cards");
    let card = d.add(cards, "a.card");
    d.attr(card, "data-kind", "movie");
    let image = d.add(card, "div.card-image");
    let cap = d.add(card, "div.card-caption");
    let first = d.add(cap, "div.card-title");
    let second = d.add(cap, "div.card-meta");
    let css = "
        :root { --jf-palette-primary-main: rgb(5, 6, 7); }
        .skinHeader { background: rgb(10, 20, 30); }
        .headerTabs .emby-tab-button-active { color: rgb(9, 9, 9); }
        .homeSectionsContainer .sectionTitle-cards { color: rgb(1, 2, 3); }
        .card[data-type=Movie] .cardFooter .cardText-first { color: rgb(200, 100, 50); }
        .cardText-secondary { color: rgb(40, 50, 60); }
        .cardBox > .cardScalable > .cardPadder + .cardImageContainer { border-radius: 9px; }
        .emby-checkbox { color: red; }
    ";
    let mut e = Engine::new(&default_css());
    e.set_user_css(Some(css));
    let s = e.build(
        &d,
        (1600.0, 900.0),
        Interaction::default(),
        &Builders::new(),
        &mut approx,
    );
    let c = |i: Id| s.style(i).color;
    assert_eq!(c(tab), Color::rgba(9, 9, 9, 255));
    assert_eq!(c(title), Color::rgba(1, 2, 3, 255));
    assert_eq!(c(first), Color::rgba(200, 100, 50, 255));
    assert_eq!(c(second), Color::rgba(40, 50, 60, 255));
    assert_eq!(s.style(image).radius, [9.0; 4]);
    assert_eq!(s.style(header).bg.len(), 1);
    let rep = e.compat_report().expect("translated");
    assert_eq!(rep.selectors - rep.translated, 1, "{:?}", rep.groups());
}
