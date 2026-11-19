use super::*;
use crate::select::{ACTIVE, FOCUS, FOCUS_VISIBLE, FOCUS_WITHIN};

#[test]
fn the_documented_forms_match() {
    let (d, a, b, c) = small();
    assert!(matches(&d, ".a", a) && !matches(&d, ".a", b));
    assert!(
        matches(&d, "page > .a > .c", c)
            && !matches(&d, "page > .c", c)
            && matches(&d, "page .c", c)
    );
    assert!(matches(&d, ".a + .b", b) && matches(&d, ".a ~ .b", b) && !matches(&d, ".b + .a", a));
    assert!(matches(&d, "[data-kind=movie]", b) && matches(&d, "[data-kind^=mo]", b));
    assert!(matches(&d, "[data-kind$=vie]", b) && matches(&d, "[data-kind*=ovi]", b));
    assert!(!matches(&d, "[data-kind=show]", b) && matches(&d, "[data-kind]", b));
    assert!(matches(&d, "[data-kind=MOVIE i]", b) && !matches(&d, "[data-kind=MOVIE]", b));
    assert!(matches(&d, "[data-kind|=movie]", b) && matches(&d, "[data-kind~=movie]", b));
    assert!(matches(&d, "page:root", 0) && !matches(&d, ":root", a));
}

#[test]
fn structural_pseudo_classes() {
    let mut d = Doc::new("page");
    let ul = d.add(0, "ul");
    let items: Vec<Id> = (0..6)
        .map(|i| d.add(ul, if i % 2 == 0 { "li.even" } else { "b.odd" }))
        .collect();
    assert!(matches(&d, ":first-child", items[0]) && matches(&d, ":last-child", items[5]));
    assert!(!matches(&d, ":only-child", items[0]) && matches(&d, ":only-child", ul));
    assert!(matches(&d, ":nth-child(2)", items[1]) && matches(&d, ":nth-child(odd)", items[0]));
    assert!(matches(&d, ":nth-child(even)", items[1]) && matches(&d, ":nth-child(3n+1)", items[3]));
    assert!(
        matches(&d, ":nth-child(-n+2)", items[1]) && !matches(&d, ":nth-child(-n+2)", items[2])
    );
    assert!(
        matches(&d, ":nth-last-child(1)", items[5]) && matches(&d, ":nth-last-child(2)", items[4])
    );
    assert!(
        matches(&d, "li:first-of-type", items[0]) && !matches(&d, "li:first-of-type", items[2])
    );
    assert!(matches(&d, "b:last-of-type", items[5]) && matches(&d, "li:nth-of-type(2)", items[2]));
    assert!(matches(&d, ":empty", items[0]) && !matches(&d, ":empty", ul));
}

#[test]
fn is_where_not_and_lists() {
    let (d, a, b, c) = small();
    assert!(
        matches(&d, ":is(.a, .b)", a)
            && matches(&d, ":is(.a, .b)", b)
            && !matches(&d, ":is(.a, .b)", c)
    );
    assert!(matches(&d, ":where(.a, .b)", b) && matches(&d, "page > :is(.a, .b) > span", c));
    assert!(matches(&d, ":not(.a, .b)", c) && !matches(&d, ":not(.a, .b)", a));
    assert!(matches(&d, ":not(.a)", b) && !matches(&d, ":not(.a)", a));
    assert!(matches(&d, ":is(page .a, .zzz)", a));
    assert!(matches(&d, ":not(page > .a)", b) && !matches(&d, ":not(page > .a)", a));
}

#[test]
fn specificity_of_functional_pseudos() {
    let sp = |s: &str| select::parse(s).unwrap().specificity();
    assert_eq!(sp(":where(.a, #b)"), 0);
    assert_eq!(sp(":is(.a, #b)"), sp("#b"));
    assert_eq!(sp(":not(.a)"), sp(".a"));
    assert_eq!(sp("div.a"), (1 << 8) | 1);
    assert!(sp("#x") > sp(".a.b.c.d"));
    assert_eq!(sp(":has(> .a)"), sp(".a"));
    assert_eq!(sp("a::before"), sp("a") + 1);
}

#[test]
fn interaction_state_pseudo_classes() {
    let (d, a, b, _) = small();
    let mut f = vec![0u8; d.len()];
    f[a] |= HOVER;
    assert!(matches_with(&d, ".a:hover", a, &f) && !matches_with(&d, ".b:hover", b, &f));
    f[b] |= FOCUS | FOCUS_VISIBLE;
    assert!(matches_with(&d, ".b:focus", b, &f) && matches_with(&d, ".b:focus-visible", b, &f));
    f[0] |= FOCUS_WITHIN;
    assert!(
        matches_with(&d, "page:focus-within", 0, &f) && !matches_with(&d, ".a:focus-within", a, &f)
    );
    f[a] |= ACTIVE;
    assert!(matches_with(&d, ".a:active", a, &f));
    // `:focus` without the keyboard bit is not `:focus-visible`.
    let mut g = vec![0u8; d.len()];
    g[b] |= FOCUS;
    assert!(!matches_with(&d, ".b:focus-visible", b, &g));
}

#[test]
fn has_with_relative_selectors() {
    let (d, a, b, c) = small();
    assert!(
        matches(&d, ".a:has(.c)", a)
            && matches(&d, ".a:has(> .c)", a)
            && !matches(&d, ".b:has(.c)", b)
    );
    assert!(matches(&d, "page:has(span)", 0) && !matches(&d, "page:has(> span)", 0));
    assert!(
        matches(&d, ".a:has(+ .b)", a)
            && matches(&d, ".a:has(~ .b)", a)
            && !matches(&d, ".b:has(+ .a)", b)
    );
    assert!(matches(&d, ":is(.a, .b):has(.c, .zzz)", a));
    assert!(!matches(&d, ".c:has(*)", c));
    let f = hover_flags(&d, c);
    assert!(
        matches_with(&d, ".a:has(.c:hover)", a, &f) && !matches_with(&d, ".b:has(.c:hover)", b, &f)
    );
}

#[test]
fn rejects_what_it_cannot_do() {
    for bad in [
        "a:visited",
        ">",
        "",
        "a:nth-child(x)",
        ":is()",
        ":has(:has(a))",
        "a::selection",
        "a::before b",
        "a:has()",
        "[",
        "a:is(",
        ".",
        "#",
    ] {
        assert!(select::parse(bad).is_err(), "{bad:?} should not parse");
    }
    assert!(select::parse("a::before").is_ok() && select::parse("a:after").is_ok());
    assert_eq!(
        select::parse("a::before").unwrap().pseudo,
        Some(Gen::Before)
    );
}

#[test]
fn complexity_limits() {
    let long = format!(".a{}", " .b".repeat(20));
    assert!(select::parse(&long).is_err());
    assert!(select::parse(":is(:is(:is(:is(.a))))").is_err());
    let wide = format!(
        ":is({})",
        (0..40)
            .map(|i| format!(".c{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(select::parse(&wide).is_err());
    assert!(select::parse(&"a".repeat(400)).is_err());
}
