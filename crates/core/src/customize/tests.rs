use super::*;

fn p(css: &str) -> Parsed {
    parse(css).unwrap()
}

fn kinds(r: &Parsed) -> Vec<WarningKind> {
    r.warnings.iter().map(|w| w.kind).collect()
}

fn c(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color::rgba(r, g, b, 255))
}

#[test]
fn empty_and_comment_only_sheets_set_nothing() {
    for s in ["", "   \n", "/* nothing */", "/* a */ /* b */"] {
        let r = p(s);
        assert_eq!(r.theme, Theme::default());
        assert!(r.warnings.is_empty(), "{s:?}: {:?}", r.warnings);
    }
}

#[test]
fn root_variables_set_tokens() {
    let r = p(
        ":root { --nocturne-accent: #ff0066; --nocturne-radius-panel: 12px; --nocturne-density: 1.25; --nocturne-backdrop-blur: 8px; }",
    );
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.theme.accent, c(255, 0, 102));
    assert_eq!(r.theme.radius_panel, Some(12.0));
    assert_eq!(r.theme.density, Some(1.25));
    assert_eq!(r.theme.backdrop_blur, Some(8.0));
    assert_eq!(r.theme.surface, None);
}

#[test]
fn jellyfin_variable_aliases_map_to_tokens() {
    let r = p(
        ":root{--accentColor:#5d55e7;--textColor:#d1d5db;--dimTextColor:#9ca3af;--smallRadius:0.5em;--largeRadius:1em;--rounding:4px;--someOther:1}",
    );
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.theme.accent, c(0x5d, 0x55, 0xe7));
    assert_eq!(r.theme.text, c(0xd1, 0xd5, 0xdb));
    assert_eq!(r.theme.text_secondary, c(0x9c, 0xa3, 0xaf));
    assert_eq!(r.theme.radius_small, Some(8.0));
    assert_eq!(r.theme.radius_panel, Some(16.0));
    assert_eq!(r.theme.radius_card, Some(4.0));
}

#[test]
fn semantic_selectors_and_jellyfin_classes() {
    let r = p("
        body { background-color: #101820; color: #eee; font-size: 110%; }
        .card, .cardBox { border-radius: 6px; }
        .skinHeader { background: rgba(0,0,0,.5) }
        button, .emby-button { background-color: #222; border-color: #888; border-radius: 4px; }
        .dialog { background: #181818; border-radius: 14px; }
        .backdropImage { opacity: 60%; filter: blur(22px) saturate(95%); }
    ");
    let t = &r.theme;
    assert_eq!(
        t.background,
        Some(Fill::Solid(Color::rgba(0x10, 0x18, 0x20, 255)))
    );
    assert_eq!(t.text, c(0xee, 0xee, 0xee));
    assert_eq!(t.font_size, Some(17.6));
    assert_eq!(t.radius_card, Some(6.0));
    assert_eq!(t.header, Some(Color::rgba(0, 0, 0, 128)));
    assert_eq!(t.control, c(0x22, 0x22, 0x22));
    assert_eq!(t.edge, c(0x88, 0x88, 0x88));
    assert_eq!(t.radius_small, Some(4.0));
    assert_eq!(t.surface, c(0x18, 0x18, 0x18));
    assert_eq!(t.radius_panel, Some(14.0));
    assert_eq!(t.backdrop_opacity, Some(0.6));
    assert_eq!(t.backdrop_blur, Some(22.0));
    // saturate() is the only thing left over.
    assert_eq!(kinds(&r), [WarningKind::UnsupportedValue]);
}

#[test]
fn var_resolution_fallback_and_chains() {
    let r = p(
        ":root{--a:#112233;--b:var(--a);--nocturne-accent:var(--b);--nocturne-edge:var(--missing,#445566);--nocturne-warn:var(--nope, var(--a))}",
    );
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.theme.accent, c(0x11, 0x22, 0x33));
    assert_eq!(r.theme.edge, c(0x44, 0x55, 0x66));
    assert_eq!(r.theme.warn, c(0x11, 0x22, 0x33));
}

#[test]
fn var_inside_a_function_argument() {
    let r = p(":root{--accent:0,164,220;--nocturne-accent:rgba(var(--accent),.5)}");
    assert_eq!(r.theme.accent, Some(Color::rgba(0, 164, 220, 128)));
}

#[test]
fn var_cycle_and_undefined_warn() {
    let r =
        p(":root{--a:var(--b);--b:var(--a);--nocturne-accent:var(--a);--nocturne-text:var(--x)}");
    assert_eq!(r.theme, Theme::default());
    assert_eq!(
        kinds(&r),
        [WarningKind::VarCycle, WarningKind::UnresolvedVar]
    );
    let r = p(":root{--a:var(--a);--nocturne-accent:var(--a)}");
    assert_eq!(kinds(&r), [WarningKind::VarCycle]);
}

#[test]
fn var_blowup_is_bounded() {
    let mut css = String::from(":root{--v0:aaaaaaaa;");
    for i in 1..40 {
        css.push_str(&format!("--v{i}:var(--v{}) var(--v{});", i - 1, i - 1));
    }
    css.push_str("--nocturne-accent:var(--v39)}");
    let r = p(&css);
    assert_eq!(kinds(&r), [WarningKind::UnsupportedValue]);
}

#[test]
fn important_is_ignored_without_error() {
    let r = p("body{color:#fff!important;background:#000 ! IMPORTANT}");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.theme.text, c(255, 255, 255));
    assert_eq!(
        r.theme.background,
        Some(Fill::Solid(Color::rgba(0, 0, 0, 255)))
    );
}

#[test]
fn gradient_background() {
    let r = p("body{background:linear-gradient(135deg,#000 0%,#223 50%,#446)}");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let Some(Fill::Linear { angle, stops }) = r.theme.background else {
        panic!()
    };
    assert_eq!((angle, stops.len()), (135.0, 3));
    assert_eq!(stops[2].1, 1.0);
}

#[test]
fn gradient_on_a_colour_token_warns() {
    let r = p(".dialog{background:linear-gradient(red,blue)}");
    assert_eq!(kinds(&r), [WarningKind::UnsupportedValue]);
    assert!(r.warnings[0].message.contains("page background"));
}

#[test]
fn font_family_takes_first_named_and_warns_it_is_not_downloaded() {
    let r = p("body{font-family:'Inter', \"Noto Sans\", sans-serif}");
    assert_eq!(r.theme.font_family.as_deref(), Some("Inter"));
    assert_eq!(kinds(&r), [WarningKind::FontNotDownloaded]);
    let r = p("body{font-family:sans-serif}");
    assert_eq!(r.theme.font_family, None);
    assert_eq!(kinds(&r), [WarningKind::Approximated]);
}

#[test]
fn clamping_and_multi_value_radius_are_reported() {
    let r =
        p(".card{border-radius:100px}.dialog{border-radius:4px 8px 4px 8px}body{font-size:2px}");
    assert_eq!(r.theme.radius_card, Some(48.0));
    assert_eq!(r.theme.radius_panel, Some(4.0));
    assert_eq!(r.theme.font_size, Some(10.0));
    let mut k = kinds(&r);
    k.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(
        k,
        [
            WarningKind::Approximated,
            WarningKind::Clamped,
            WarningKind::Clamped
        ]
    );
}

#[test]
fn percent_radius_is_of_a_control_box() {
    assert_eq!(p(".card{border-radius:50%}").theme.radius_card, Some(20.0));
}

// ---- every warning kind ----------------------------------------------------------------------

#[test]
fn warning_syntax_error() {
    for css in [
        "body{color}",
        "body{: red}",
        "}",
        "body{color:}",
        "body",
        "/* open",
    ] {
        assert!(kinds(&p(css)).contains(&WarningKind::SyntaxError), "{css}");
    }
}

#[test]
fn warning_unsupported_at_rules() {
    let r = p(
        "@media (max-width: 600px){body{color:red}} @keyframes x{from{top:0}} @supports (a:b){a{b:c}} @namespace x; @charset \"utf-8\";",
    );
    assert_eq!(
        kinds(&r),
        [
            WarningKind::UnsupportedAtRule,
            WarningKind::UnsupportedAtRule,
            WarningKind::UnsupportedAtRule,
            WarningKind::UnsupportedAtRule
        ]
    );
    assert_eq!(r.theme, Theme::default());
}

#[test]
fn warning_font_face_and_import_are_never_fetched() {
    let r = p(
        "@import url('https://fonts.example/x.css');\n@font-face{font-family:X;src:url(x.woff2)}",
    );
    assert_eq!(
        kinds(&r),
        [WarningKind::RemoteResource, WarningKind::FontNotDownloaded]
    );
    assert_eq!((r.warnings[0].line, r.warnings[1].line), (1, 2));
}

#[test]
fn warning_unsupported_selectors_with_reason() {
    let cases = [
        (".a .b{color:red}", "descendant"),
        ("a:hover{color:red}", "pseudo-classes"),
        (".card::before{color:red}", "pseudo-elements"),
        ("[data-x]{color:red}", "attribute"),
        ("#id{color:red}", "id selectors"),
        (".card.x{color:red}", "compound"),
        (".unknown{color:red}", "NOCTURNE"),
        (".card:hover{color:red}", "pseudo-classes"),
    ];
    for (css, why) in cases {
        let r = p(css);
        assert_eq!(kinds(&r), [WarningKind::UnsupportedSelector], "{css}");
        assert!(
            r.warnings[0].message.contains(why),
            "{css}: {}",
            r.warnings[0].message
        );
        assert_eq!(r.theme, Theme::default());
    }
}

#[test]
fn partly_supported_selector_list_applies_and_warns() {
    let r = p(".card, .card:hover { border-radius: 9px }");
    assert_eq!(r.theme.radius_card, Some(9.0));
    assert_eq!(kinds(&r), [WarningKind::UnsupportedSelector]);
}

#[test]
fn warning_unsupported_properties() {
    let r = p(
        ".card{transition:all .2s;box-shadow:0 0 4px red;margin:4px;cursor:pointer;-webkit-transform:scale(1.1)}",
    );
    assert_eq!(kinds(&r), [WarningKind::UnsupportedProperty; 5]);
    assert!(r.warnings[0].message.contains("animations"));
    assert!(r.warnings[1].message.contains("shadows"));
    assert!(r.warnings[2].message.contains("layout"));
    // A role-specific property on the wrong role.
    assert_eq!(
        kinds(&p(".header{border-radius:4px}")),
        [WarningKind::UnsupportedProperty]
    );
}

#[test]
fn warning_unsupported_values() {
    for css in [
        "body{color:currentcolor}",
        ".card{border-radius:calc(1px + 2px)}",
        ".card{border-radius:2vw}",
        "body{background:radial-gradient(red,blue)}",
        "body{font-size:large}",
        ".backdrop{opacity:abc}",
        ".backdrop{filter:grayscale(1)}",
    ] {
        assert!(
            kinds(&p(css)).contains(&WarningKind::UnsupportedValue),
            "{css}"
        );
    }
}

#[test]
fn warning_remote_resources() {
    let r = p(
        "body{background:url(https://x.example/a.png)} .card{background-image:url(a.png)} :root{--nocturne-background:url(a.png)}",
    );
    assert_eq!(kinds(&r), [WarningKind::RemoteResource; 3]);
    assert_eq!(r.theme, Theme::default());
}

#[test]
fn warning_custom_property_outside_root_and_nested_rules() {
    let r = p(".card{--nocturne-accent:red} body{color:red; a{color:blue}}");
    assert_eq!(
        kinds(&r),
        [
            WarningKind::UnsupportedProperty,
            WarningKind::UnsupportedSelector
        ]
    );
    assert_eq!(r.theme.text, c(255, 0, 0));
    assert_eq!(r.theme.accent, None);
}

#[test]
fn warning_positions_are_one_based_line_and_column() {
    let r = p("body{color:red}\n  .a .b{color:red}\n.card{\n    margin:0\n}");
    assert_eq!((r.warnings[0].line, r.warnings[0].col), (2, 3));
    assert_eq!((r.warnings[1].line, r.warnings[1].col), (4, 5));
}

#[test]
fn comments_and_strings_do_not_confuse_the_reader() {
    let r = p(
        "/* body{color:red} */ body{color:blue /* x */; /* y */ background:#000}\n.card{content:\"}/*\"}",
    );
    assert_eq!(r.theme.text, c(0, 0, 255));
    assert_eq!(kinds(&r), [WarningKind::UnsupportedProperty]);
}

// ---- limits ----------------------------------------------------------------------------------

#[test]
fn oversize_input_is_rejected() {
    let big = "a".repeat(MAX_BYTES + 1);
    assert_eq!(
        parse(&big),
        Err(Error::TooLarge {
            bytes: MAX_BYTES + 1
        })
    );
    assert!(parse(&"/".repeat(MAX_BYTES)).is_ok());
    assert_eq!(parse_bytes(&[0xff, 0xfe]), Err(Error::NotUtf8));
    assert!(Error::TooLarge { bytes: 9 }.to_string().contains("262144"));
}

#[test]
fn rule_and_warning_caps() {
    let css = ".x{a:b}".repeat(MAX_RULES + 10);
    let r = p(&css);
    assert!(
        r.warnings
            .iter()
            .any(|w| w.kind == WarningKind::LimitReached)
    );
    assert!(r.warnings.len() <= 500 && r.dropped_warnings > 0);
}

#[test]
fn deep_nesting_does_not_recurse() {
    let open = "{".repeat(20_000);
    let _ = p(&format!("a{open}"));
    let _ = p(&format!("@media x{open}"));
    let _ = p(&format!(
        "body{{color:{}red{}}}",
        "(".repeat(20_000),
        ")".repeat(20_000)
    ));
}

#[test]
fn garbage_never_panics() {
    // xorshift: deterministic, no dependency.
    let mut s = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let alphabet: Vec<char> =
        "{}();:,@.#/*!'\"\\- \nvar(--)#fffrgb%0123456789px:root body .card linear-gradient"
            .chars()
            .collect();
    for _ in 0..300 {
        let n = (next() % 400) as usize;
        let css: String = (0..n)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .collect();
        let _ = parse(&css);
        let bytes: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        let _ = parse_bytes(&bytes);
        let _ = parse(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn overlay_keeps_unset_fields() {
    let mut base = p(":root{--nocturne-accent:#111;--nocturne-edge:#222}").theme;
    base.overlay(&p(":root{--nocturne-accent:#333}").theme);
    assert_eq!(base.accent, c(0x33, 0x33, 0x33));
    assert_eq!(base.edge, c(0x22, 0x22, 0x22));
}

#[test]
fn theme_round_trips_through_toml_without_unset_fields() {
    let t = p(":root{--nocturne-accent:#336699} body{background:linear-gradient(red,blue)}").theme;
    let s = toml::to_string(&t).unwrap();
    assert!(!s.contains("surface"));
    assert_eq!(toml::from_str::<Theme>(&s).unwrap(), t);
}

/// Paraphrased from the structure of the ElegantFin theme (a variables block, a gradient page,
/// card and button rules, a font import, hover and media rules); the values are illustrative.
#[test]
fn real_world_shaped_theme() {
    let css = r#"
@import url("https://fonts.googleapis.com/css2?family=Inter&display=swap");
:root {
  font-family: Inter, sans-serif;
  color-scheme: dark;
  --largeRadius: 1em; --smallRadius: 0.5em;
  --textColor: #d1d5db; --dimTextColor: #9ca3af;
  --accentColor: #5d55e7; --headerColor: rgba(30,40,54,.8);
  --borderColor: #47505c; --blurDefault: blur(5px);
  --defaultBorder: 0.06em solid var(--borderColor);
}
body, .backgroundContainer { background: linear-gradient(135deg, #111827 0%, #1d2635 100%) !important; }
.skinHeader { background: var(--headerColor); backdrop-filter: var(--blurDefault); border-bottom: var(--defaultBorder); }
.cardBox { border-radius: var(--largeRadius); transition: transform .3s; }
.cardBox:hover { transform: scale(1.05); }
.emby-button { border-radius: var(--smallRadius); background-color: var(--accentColor) }
@media (max-width: 600px) { .skinHeader { height: 4em } }
@keyframes pulse { from { opacity: 0 } to { opacity: 1 } }
"#;
    let r = p(css);
    let t = &r.theme;
    assert_eq!(t.accent, c(0x5d, 0x55, 0xe7));
    assert_eq!(t.text, c(0xd1, 0xd5, 0xdb));
    assert_eq!(t.text_secondary, c(0x9c, 0xa3, 0xaf));
    assert_eq!(t.edge, c(0x47, 0x50, 0x5c));
    assert_eq!(t.header, Some(Color::rgba(30, 40, 54, 204)));
    assert_eq!(t.control, c(0x5d, 0x55, 0xe7));
    assert_eq!(
        (t.radius_card, t.radius_panel, t.radius_small),
        (Some(16.0), Some(16.0), Some(8.0))
    );
    assert!(matches!(t.background, Some(Fill::Linear { angle, .. }) if angle == 135.0));
    assert_eq!(t.font_family.as_deref(), Some("Inter"));
    let mut got: Vec<(u32, WarningKind)> = r.warnings.iter().map(|w| (w.line, w.kind)).collect();
    got.sort_by_key(|(l, _)| *l);
    assert_eq!(
        got,
        [
            (2, WarningKind::RemoteResource),
            (4, WarningKind::FontNotDownloaded),
            (5, WarningKind::UnsupportedProperty), // color-scheme
            (13, WarningKind::UnsupportedProperty), // backdrop-filter
            (13, WarningKind::UnsupportedProperty), // border-bottom
            (14, WarningKind::UnsupportedProperty), // transition
            (15, WarningKind::UnsupportedSelector), // :hover
            (17, WarningKind::UnsupportedAtRule),
            (18, WarningKind::UnsupportedAtRule),
        ]
    );
}
