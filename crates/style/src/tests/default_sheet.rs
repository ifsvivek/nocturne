//! The app's default sheet (`themes/default.css`) parses without a report and styles the classes the tree builders emit.
use super::*;

const ROOT: &str = ":root { --nocturne-text: rgba(236, 238, 245, 1.000); --nocturne-accent: rgba(76, 141, 255, 1.000); --nocturne-accent-text: rgba(156, 192, 255, 1.000); --nocturne-accent-tint: rgba(76, 141, 255, 0.122); --nocturne-on-accent: rgba(5, 8, 22, 1.000); --nocturne-edge: rgba(116, 122, 145, 1.000); --nocturne-hairline: rgba(255, 255, 255, 0.078); --nocturne-hairline-strong: rgba(255, 255, 255, 0.161); --nocturne-header-bg: rgba(7, 8, 12, 0.902); --nocturne-surface: rgba(12, 14, 22, 1.000); --nocturne-raised: rgba(18, 21, 31, 1.000); --nocturne-text-secondary: rgba(164, 170, 191, 1.000); --nocturne-text-tertiary: rgba(137, 144, 169, 1.000); --nocturne-r-card: 3px; --nocturne-page-pad: 48px; }\n";

pub(crate) fn default_css() -> String {
    format!("{ROOT}{}", include_str!("../../themes/default.css"))
}

#[test]
fn the_default_sheet_styles_what_the_builders_emit() {
    let css = default_css();
    let sheet = Sheet::parse_default(&css);
    let mut d = Doc::new("page");
    let hero = d.add(0, "section.hero");
    let chip = d.add(hero, "div.chip.accent");
    d.text(chip, "MOVIE");
    let b = d.add(hero, "a.button.primary");
    d.text(b, "Play");
    let header = d.add(0, "header.header.header-fixed");
    let s = scene_with(&d, sheet, (1600.0, 900.0), Interaction::default());
    let bg = |i: Id| s.style(i).bg.len();
    assert_eq!(
        (bg(chip), bg(b), bg(header)),
        (1, 1, 1),
        "chip, button and header have a background: {:?}",
        s.warnings().lines()
    );
    assert!(
        s.warnings().lines().is_empty(),
        "{:?}",
        s.warnings().lines()
    );
}
