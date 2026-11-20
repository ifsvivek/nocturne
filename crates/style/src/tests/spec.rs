//! The building blocks of a measured, glass-style dark theme (translucent chrome, scrims over artwork, tracked uppercase titles,
//! gutters by breakpoint, eased transitions): each one the engine core has to express. Values here are derived from such a
//! specification, not copied from any product.
use super::*;
use crate::anim::Anims;
use crate::paint::{self, Ease, Fill, Fit};
use nocturne_core::customize::value::Color;
use std::time::{Duration, Instant};

fn solo(css: &str) -> (Scene, Id) {
    let mut d = Doc::new("page");
    let n = d.add(0, "div.t");
    d.text(n, "A long title that needs more than one line to show");
    (scene(&d, css, (1000.0, 600.0)), n)
}

#[test]
fn gradients_with_six_and_more_stops_double_positions_and_alpha_hex() {
    let l = paint::layers("linear-gradient(#000 0% 58%, #000000d6 72%, #0009 82%, #0000005c 90%, #00000029 96%, #0000 100%)").unwrap();
    let r = paint::resolve(&l[0], 100.0, 100.0).unwrap();
    // `#000 0% 58%` is two stops: 7 in all.
    assert_eq!(r.stops.len(), 7);
    assert_eq!(r.angle, 180.0);
    assert_eq!((r.stops[0].1, r.stops[1].1), (0.0, 0.58));
    assert_eq!(r.stops[2].0, Color::rgba(0, 0, 0, 0xd6));
    assert_eq!(r.stops[3].0, Color::rgba(0, 0, 0, 0x99));
    assert_eq!(r.stops[6].0.a, 0);
    assert!(r.stops.windows(2).all(|w| w[0].1 <= w[1].1));
    // Seven stops, an angle, rgba() with a leading dot.
    let l = paint::layers("linear-gradient(90deg, #000 0%, rgba(0,0,0,.7244) 16%, rgba(0,0,0,.4739) 32%, rgba(0,0,0,.2614) 46%, rgba(0,0,0,.1253) 57%, rgba(0,0,0,.0436) 64%, transparent 70%)").unwrap();
    let r = paint::resolve(&l[0], 400.0, 200.0).unwrap();
    assert_eq!((r.angle, r.stops.len()), (90.0, 7));
    assert_eq!(r.stops[1].0.a, (0.7244f32 * 255.0).round() as u8);
}

#[test]
fn more_than_eight_stops_keep_the_ends_and_stay_monotonic() {
    let stops: Vec<String> = (0..12)
        .map(|i| format!("rgba(0,0,0,.{i}) {}%", i * 9))
        .collect();
    let l = paint::layers(&format!("linear-gradient(180deg, {})", stops.join(", "))).unwrap();
    let r = paint::resolve(&l[0], 10.0, 10.0).unwrap();
    assert_eq!(r.stops.len(), paint::MAX_STOPS);
    assert_eq!((r.stops[0].1, r.stops[7].1), (0.0, 0.99));
    assert!(r.stops.windows(2).all(|w| w[0].1 <= w[1].1));
}

#[test]
fn pixel_stops_resolve_against_the_box() {
    let l = paint::layers(
        "linear-gradient(90deg, transparent 0, rgba(0,0,0,.78) 268px, rgba(0,0,0,.96) 55%)",
    )
    .unwrap();
    let r = paint::resolve(&l[0], 1000.0, 100.0).unwrap();
    assert!((r.stops[1].1 - 0.268).abs() < 1e-4 && (r.stops[2].1 - 0.55).abs() < 1e-4);
    // A stop that would go backwards is pulled up to the previous one (CSS rule).
    let l = paint::layers("linear-gradient(90deg, red 60%, blue 20%)").unwrap();
    assert_eq!(paint::resolve(&l[0], 10.0, 10.0).unwrap().stops[1].1, 0.6);
    assert_eq!(paint::line_length(90.0, 300.0, 50.0).round(), 300.0);
    assert_eq!(paint::line_length(180.0, 300.0, 50.0).round(), 50.0);
}

#[test]
fn layered_backgrounds_gradient_over_colour() {
    let (s, n) = solo(
        ".t { background: linear-gradient(180deg, rgba(255,255,255,.055) 0%, rgba(255,255,255,.012) 100%), rgba(22,22,24,.82) }",
    );
    let bg = &s.style(n).bg;
    assert_eq!(bg.len(), 2);
    assert!(matches!(bg[0], Fill::Linear { .. }));
    assert_eq!(bg[1], Fill::Solid(Color::rgba(22, 22, 24, 209)));
    // Plain colour and `none` still work.
    let (s, n) = solo(".t { background: #101113 }");
    assert_eq!(s.style(n).bg.len(), 1);
    let (s, n) = solo(".t { background: #101113; background: none }");
    assert!(s.style(n).bg.is_empty());
    // A shorthand with extra words keeps the fill.
    let (s, n) = solo(
        ".t { background: linear-gradient(115deg, #101113 15%, #1a1c20 42%, #101113 70%) center / cover no-repeat }",
    );
    assert_eq!(s.style(n).bg.len(), 1);
}

#[test]
fn inset_and_outer_box_shadows_with_spread() {
    let (s, n) = solo(
        ".t { box-shadow: inset 0 1px rgba(255,255,255,.08), 0 6px 14px -6px rgba(0,0,0,.55) }",
    );
    let sh = &s.style(n).shadows;
    assert_eq!(sh.len(), 2);
    assert!(sh[0].inset && (sh[0].dx, sh[0].dy, sh[0].blur) == (0.0, 1.0, 0.0));
    assert!(!sh[1].inset && (sh[1].dy, sh[1].blur, sh[1].spread) == (6.0, 14.0, -6.0));
    let (s, n) = solo(
        ".t { box-shadow: inset 0 1px rgba(255,255,255,.075), 0 16px 40px -12px rgba(0,0,0,.72), 0 4px 10px rgba(0,0,0,.22) }",
    );
    assert_eq!(s.style(n).shadows.len(), 3);
    let (s, n) = solo(".t { box-shadow: none }");
    assert!(s.style(n).shadows.is_empty());
}

#[test]
fn text_shadow_and_drop_shadow_one_each() {
    let (s, n) = solo(
        ".t { text-shadow: 0 3px 14px rgba(0,0,0,.45); filter: drop-shadow(0 3px 10px rgba(0,0,0,.35)) }",
    );
    let c = s.style(n);
    let ts = c.text_shadow.unwrap();
    assert_eq!(
        (ts.dx, ts.dy, ts.blur, ts.color.a),
        (0.0, 3.0, 14.0, (0.45f32 * 255.0).round() as u8)
    );
    assert_eq!(c.drop_shadow.unwrap().blur, 10.0);
    // Text shadow inherits; the drop shadow does not.
    let mut d = Doc::new("page");
    let p = d.add(0, "div.p");
    let k = d.add(p, "span.k");
    let s = scene(
        &d,
        ".p { text-shadow: 1px 1px 2px #000; filter: drop-shadow(1px 1px 2px #000) }",
        (800.0, 600.0),
    );
    assert!(s.style(k).text_shadow.is_some() && s.style(k).drop_shadow.is_none());
    // `inset` is meaningless on text.
    let (s, _) = solo(".t { text-shadow: inset 1px 1px red }");
    assert!(s.warnings().count(Kind::BadValue) >= 1);
}

#[test]
fn letter_spacing_tracks_characters() {
    let mut d = Doc::new("page");
    let t = d.add(0, "h1.t");
    d.text(t, "ABC DEF");
    let plain = scene(&d, ".t { font-size: 10px }", (800.0, 600.0));
    let spaced = scene(
        &d,
        ".t { font-size: 10px; letter-spacing: 2px }",
        (800.0, 600.0),
    );
    let tracked = spaced
        .block(t)
        .expect("spaced text is placed by the engine");
    assert!(plain.block(t).is_none());
    assert_eq!(tracked.lines.len(), 1);
    let line = &tracked.lines[0];
    // Seven characters, each followed by 2px.
    assert_eq!(line.x.len(), 7);
    assert!(line.x.windows(2).all(|w| w[1] > w[0]));
    assert!(line.width >= 7.0 * 2.0);
    // approx: 5px per glyph: 4px wider per... 2px per character on top of the natural width.
    let natural = 7.0 * 5.0;
    assert!(
        (line.width - (natural + 14.0)).abs() < 3.0,
        "{} vs {}",
        line.width,
        natural + 14.0
    );
    // The box is as wide as the tracked line.
    assert!(spaced.rects[t][2] >= 1.0);
    // Negative tracking in em narrows it.
    let tight = scene(
        &d,
        ".t { font-size: 10px; letter-spacing: -0.03em }",
        (800.0, 600.0),
    );
    assert!(tight.block(t).unwrap().lines[0].width < natural);
    assert!((tight.style(t).letter_spacing + 0.3).abs() < 1e-5);
}

#[test]
fn line_clamp_ends_with_an_ellipsis_that_fits() {
    let mut d = Doc::new("page");
    let t = d.add(0, "p.t");
    let long = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen";
    d.text(t, long);
    let s = scene(
        &d,
        ".t { width: 100px; font-size: 10px; line-height: 1; -webkit-line-clamp: 2 }",
        (800.0, 600.0),
    );
    let b = s.block(t).expect("clamped text is placed by the engine");
    assert_eq!(b.lines.len(), 2);
    assert!(
        b.truncated && b.lines[1].text.ends_with('\u{2026}'),
        "{:?}",
        b.lines
    );
    assert!(b.lines[1].width <= 100.0 + 0.01);
    assert_eq!(s.rects[t][3], 20.0);
    // Text that fits is untouched.
    let mut d2 = Doc::new("page");
    let t2 = d2.add(0, "p.t");
    d2.text(t2, "short");
    let s = scene(
        &d2,
        ".t { width: 100px; font-size: 10px; -webkit-line-clamp: 3 }",
        (800.0, 600.0),
    );
    let b = s.block(t2).unwrap();
    assert!(!b.truncated && b.lines[0].text == "short");
    // `nowrap` + `text-overflow: ellipsis` is a one-line clamp; without it the text is just cut.
    let s = scene(
        &d,
        ".t { width: 100px; font-size: 10px; white-space: nowrap; text-overflow: ellipsis }",
        (800.0, 600.0),
    );
    let b = s.block(t).unwrap();
    assert!(
        b.lines.len() == 1 && b.lines[0].text.ends_with('\u{2026}') && b.lines[0].width <= 100.01
    );
    let s = scene(
        &d,
        ".t { width: 100px; font-size: 10px; white-space: nowrap }",
        (800.0, 600.0),
    );
    let b = s.block(t).unwrap();
    assert!(
        b.lines.len() == 1 && !b.lines[0].text.contains('\u{2026}') && b.lines[0].width <= 100.01
    );
}

#[test]
fn ellipsis_with_letter_spacing_still_fits() {
    let mut d = Doc::new("page");
    let t = d.add(0, "p.t");
    d.text(t, "A very long uppercase heading that cannot fit");
    let s = scene(
        &d,
        ".t { width: 120px; font-size: 10px; letter-spacing: 1px; white-space: nowrap; text-overflow: ellipsis }",
        (800.0, 600.0),
    );
    let b = s.block(t).unwrap();
    assert!(b.lines[0].width <= 120.01 && b.lines[0].text.ends_with('\u{2026}'));
    assert_eq!(b.lines[0].x.len(), b.lines[0].text.chars().count());
}

#[test]
fn object_fit_cover_and_position() {
    // A 16:9 picture in a 100x100 box.
    let [x, y, w, h] = paint::fit_rect(Fit::Cover, (0.5, 0.5), 100.0, 100.0, 1600.0, 900.0);
    assert!(
        (h - 100.0).abs() < 1e-3
            && (w - 177.78).abs() < 0.01
            && (x + 38.89).abs() < 0.01
            && y == 0.0
    );
    let [x, ..] = paint::fit_rect(Fit::Cover, (0.0, 0.5), 100.0, 100.0, 1600.0, 900.0);
    assert_eq!(x, 0.0);
    let [x, y, w, h] = paint::fit_rect(Fit::Contain, (0.5, 1.0), 100.0, 100.0, 1600.0, 900.0);
    assert_eq!((w, h.round(), x, y.round()), (100.0, 56.0, 0.0, 44.0));
    assert_eq!(
        paint::fit_rect(Fit::Fill, (0.5, 0.5), 80.0, 40.0, 10.0, 10.0),
        [0.0, 0.0, 80.0, 40.0]
    );
    assert_eq!(
        paint::fit_rect(Fit::ScaleDown, (0.5, 0.5), 1000.0, 1000.0, 10.0, 10.0),
        [495.0, 495.0, 10.0, 10.0]
    );
    // From CSS.
    let mut d = Doc::new("page");
    let img = d.add(0, "img.p");
    d.image(img, 1, 1600, 900);
    let s = scene(
        &d,
        ".p { width: 100px; height: 100px; object-fit: cover; object-position: left top }",
        (800.0, 600.0),
    );
    assert_eq!(
        (s.style(img).fit, s.style(img).object_pos),
        (Fit::Cover, (0.0, 0.0))
    );
    let s = scene(
        &d,
        ".p { object-position: 25% 75%; object-fit: contain }",
        (800.0, 600.0),
    );
    assert_eq!(s.style(img).object_pos, (0.25, 0.75));
}

#[test]
fn easing_curves() {
    let e = Ease::parse("cubic-bezier(0.22,1,0.36,1)").unwrap();
    assert_eq!((e.at(0.0), e.at(1.0)), (0.0, 1.0));
    assert!(e.at(0.2) > 0.5 && e.at(0.5) > 0.9);
    assert!(Ease::Linear.at(0.25) == 0.25);
    assert!(Ease::EaseIn.at(0.5) < 0.5 && Ease::EaseOut.at(0.5) > 0.5);
    assert!(Ease::parse("cubic-bezier(2,0,0,1)").is_none());
    let e = Ease::Ease;
    assert!(e.at(0.5) > 0.5 && e.at(0.5) < 1.0);
    assert!(e.at(0.1) <= e.at(0.2) && e.at(0.2) <= e.at(0.9));
}

#[test]
fn transition_shorthand_forms() {
    let t = paint::transition(
        "opacity 260ms cubic-bezier(0.22,1,0.36,1), transform .3s ease 50ms, width 2s",
    )
    .unwrap();
    assert_eq!(t.for_prop(paint::AProp::Opacity).unwrap().0, 260.0);
    let (d, delay, _) = t.for_prop(paint::AProp::Transform).unwrap();
    assert_eq!((d, delay), (300.0, 50.0));
    assert!(
        t.for_prop(paint::AProp::Color).is_none(),
        "not listed: no transition"
    );
    let all = paint::transition("all 200ms ease").unwrap();
    assert_eq!(all.for_prop(paint::AProp::Background).unwrap().0, 200.0);
    assert!(paint::transition("none").unwrap().is_none());
}

fn faded() -> (Scene, Id) {
    let mut d = Doc::new("page");
    let n = d.add(0, "div.slide");
    let css = ".slide { opacity: 0; transform: translateX(3%); transition: opacity 380ms cubic-bezier(0.22,1,0.36,1), transform 380ms cubic-bezier(0.22,1,0.36,1); width: 100px; height: 40px } .slide.on { opacity: 1; transform: translateX(0) }";
    (scene(&d, css, (800.0, 600.0)), n)
}

#[test]
fn opacity_and_transform_transitions_ease_between_states() {
    let (mut s, n) = faded();
    assert!(!s.animating());
    assert_eq!(s.style(n).opacity, 0.0);
    let t0 = Instant::now();
    s.doc.nodes[n].classes.push("on".into());
    s.touch(&[n]);
    let up = s.refresh(t0, &mut approx).unwrap();
    assert!(!up.relaid, "paint-only change");
    assert!(s.animating());
    // The style says "on"; the animated value starts at the old one.
    assert_eq!(s.style(n).opacity, 1.0);
    s.tick(t0);
    assert_eq!(s.eff(n).opacity, Some(0.0));
    let xf0 = s.eff(n).xf.unwrap();
    assert!((xf0.tx.1 - 0.03).abs() < 1e-6);
    s.tick(t0 + Duration::from_millis(190));
    let mid = s.eff(n).opacity.unwrap();
    assert!(mid > 0.9 && mid < 1.0, "ease-out is fast early: {mid}");
    assert!(s.eff(n).xf.unwrap().tx.1 < 0.003);
    assert!(s.tick(t0 + Duration::from_millis(300)));
    assert!(!s.tick(t0 + Duration::from_millis(400)));
    assert!(!s.animating());
    assert_eq!(s.eff(n), crate::Eff::default());
}

#[test]
fn an_interrupted_transition_starts_from_where_it_is() {
    let (mut s, n) = faded();
    let t0 = Instant::now();
    s.doc.nodes[n].classes.push("on".into());
    s.touch(&[n]);
    s.refresh(t0, &mut approx).unwrap();
    s.tick(t0 + Duration::from_millis(100));
    let at = s.eff(n).opacity.unwrap();
    // Reverse it half way: the new run starts from `at`, not from the end state.
    s.doc.nodes[n].classes.pop();
    s.touch(&[n]);
    s.refresh(t0 + Duration::from_millis(100), &mut approx)
        .unwrap();
    s.tick(t0 + Duration::from_millis(100));
    let start = s.eff(n).opacity.unwrap();
    assert!((start - at).abs() < 1e-4, "{start} vs {at}");
    s.tick(t0 + Duration::from_millis(1000));
    assert!(!s.animating());
}

#[test]
fn colour_and_gradient_backgrounds_transition_when_shapes_match() {
    let mut d = Doc::new("page");
    let n = d.add(0, "div.b");
    let css = ".b { width: 10px; height: 10px; color: #fff; background: linear-gradient(rgba(255,255,255,.055), rgba(255,255,255,.01)), rgba(22,22,24,.82); border: 1px solid #26292e; transition: background 200ms, color 180ms, border-color 220ms } .b:hover { color: #f00; background: linear-gradient(rgba(255,255,255,.10), rgba(255,255,255,.03)), rgba(36,36,40,.90); border-color: rgba(241,242,244,.22) }";
    let mut s = scene(&d, css, (800.0, 600.0));
    let t0 = Instant::now();
    s.update(
        Interaction {
            hover: Some(n),
            ..Default::default()
        },
        t0,
        &mut approx,
    )
    .unwrap();
    assert!(s.animating());
    s.tick(t0 + Duration::from_millis(90));
    let e = s.eff(n);
    let bg = e.bg.expect("background animates");
    assert_eq!(bg.len(), 2);
    let Fill::Solid(c) = bg[1] else { panic!() };
    assert!(c.r > 22 && c.r < 36, "{c:?}");
    let col = e.color.unwrap();
    assert!(col.g < 255 && col.g > 0);
    assert!(e.border.is_some());
    s.tick(t0 + Duration::from_millis(500));
    assert!(!s.animating());
    // Differently shaped backgrounds (one layer to two) switch at once rather than flicker.
    let css = ".b { width: 10px; height: 10px; background: #000; transition: background 200ms } .b:hover { background: linear-gradient(red, blue), #000 }";
    let mut s = scene(&d, css, (800.0, 600.0));
    s.update(
        Interaction {
            hover: Some(n),
            ..Default::default()
        },
        t0,
        &mut approx,
    )
    .unwrap();
    assert!(!s.animating());
}

#[test]
fn no_transition_means_no_animation() {
    let mut d = Doc::new("page");
    let n = d.add(0, "div.b");
    let mut s = scene(
        &d,
        ".b { width: 10px; height: 10px; opacity: 1 } .b:hover { opacity: 0.5 }",
        (800.0, 600.0),
    );
    s.update(
        Interaction {
            hover: Some(n),
            ..Default::default()
        },
        Instant::now(),
        &mut approx,
    )
    .unwrap();
    assert!(!s.animating());
    assert_eq!(s.style(n).opacity, 0.5);
    let _ = Anims::default();
}

#[test]
fn transform_functions_and_origin() {
    let (s, n) =
        solo(".t { transform: translate(10px, -4px) scale(1.06); transform-origin: left top }");
    let xf = s.style(n).xf;
    assert_eq!(
        (xf.tx, xf.ty, xf.sx, xf.sy, xf.origin),
        ((10.0, 0.0), (-4.0, 0.0), 1.06, 1.06, (0.0, 0.0))
    );
    let (px, py, k) = xf.resolve(200.0, 100.0);
    assert_eq!((px, py), (10.0, -4.0));
    assert!((k - 1.06).abs() < 1e-6);
    let (s, n) = solo(".t { transform: translateX(3%) translateY(-10%) }");
    let xf = s.style(n).xf;
    assert_eq!((xf.tx.1, xf.ty.1), (0.03, -0.1));
    assert_eq!(xf.resolve(200.0, 100.0).0, 6.0);
    let (s, n) = solo(".t { transform: scale(.98) }");
    assert_eq!(s.style(n).xf.sx, 0.98);
    let (s, n) = solo(".t { transform: translateX(10px) scale(2); transform: none }");
    assert!(s.style(n).xf.is_identity());
    // Rotation is something Iced cannot draw: reported, not silently dropped.
    let (s, _) = solo(".t { transform: rotate(45deg) }");
    assert!(s.warnings().count(Kind::Planned) >= 1);
}

#[test]
fn gutter_by_breakpoint_through_variables() {
    let mut d = Doc::new("page");
    let n = d.add(0, "div.t");
    let css = ":root { --gutter: 24px } @media (min-width: 768px) { :root { --gutter: 48px } } @media (min-width: 1024px) { :root { --gutter: 80px } } @media (min-width: 1280px) { :root { --gutter: 96px } } .t { padding: 0 var(--gutter); height: 10px }";
    let mut s = scene(&d, css, (500.0, 600.0));
    for (vw, gutter) in [
        (500.0, 24.0),
        (767.0, 24.0),
        (768.0, 48.0),
        (1023.0, 48.0),
        (1024.0, 80.0),
        (1279.0, 80.0),
        (1280.0, 96.0),
        (1440.0, 96.0),
        (600.0, 24.0),
    ] {
        s.resize((vw, 600.0), &mut approx);
        assert_eq!(s.insets[n][0], gutter, "at {vw}px");
    }
}

#[test]
fn backdrop_filter_and_mask_image_have_documented_fallbacks() {
    let (s, n) = solo(
        ".t { background: rgba(22,22,24,.82); -webkit-backdrop-filter: blur(18px) saturate(140%); backdrop-filter: blur(18px) saturate(140%); -webkit-mask-image: linear-gradient(#000 0% 58%, #0000 100%) }",
    );
    let r = s.warnings();
    assert_eq!(
        r.count(Kind::UnsupportedProperty) + r.count(Kind::BadValue),
        0,
        "{:?}",
        r.lines()
    );
    assert_eq!(r.count(Kind::Approximated), 3);
    assert_eq!(r.failures(), 0);
    // The translucent fill is what stays; the mask becomes a scrim gradient.
    assert_eq!(s.style(n).bg.len(), 1);
    assert!(matches!(s.style(n).mask, Some(Fill::Linear { .. })));
    let l = r.lines().join("\n");
    assert!(l.contains("drawn through a fallback"), "{l}");
}

#[test]
fn the_scrim_colour_is_the_surface_behind() {
    let mut d = Doc::new("page");
    let host = d.add(0, "div.host");
    let img = d.add(host, "img.i");
    let s = scene(
        &d,
        ".host { background: #101113 } .i { mask-image: linear-gradient(#000, #0000) }",
        (800.0, 600.0),
    );
    assert_eq!(s.scrim_color(img), Color::rgba(0x10, 0x11, 0x13, 255));
    let s = scene(
        &d,
        ".i { mask-image: linear-gradient(#000, #0000) }",
        (800.0, 600.0),
    );
    assert_eq!(s.scrim_color(img), Color::rgba(0, 0, 0, 255));
    // A translucent surface does not count: the scrim looks further up.
    let s = scene(
        &d,
        ".host { background: rgba(1,2,3,.5) } page { background: #202020 }",
        (800.0, 600.0),
    );
    assert_eq!(s.scrim_color(img), Color::rgba(0x20, 0x20, 0x20, 255));
}

#[test]
fn spec_style_calc_min_env_and_logical_properties() {
    let mut d = Doc::new("page");
    let n = d.add(0, "div.t");
    let css = ":root { --h: 80px; --g: 96px } .t { height: calc(var(--h) + 16px); width: calc(100vw - 2 * var(--g) - 16px); min-height: min(420px, var(--h)); margin-top: calc(-1 * var(--h)); padding: 6px 8px calc(6px + env(safe-area-inset-bottom)); max-width: min(340px, 70vw); margin-inline: 4px; padding-block: 2px }";
    let s = scene(&d, css, (1440.0, 900.0));
    let c = s.style(n);
    assert_eq!(s.rects[n][3], 96.0f32.max(80.0));
    assert_eq!(s.rects[n][2], 340.0f32.min(1440.0 - 192.0 - 16.0));
    let _ = c;
    assert_eq!(s.warnings().failures(), 0, "{:?}", s.warnings().lines());
}

#[test]
fn spec_style_selectors_and_generated_content() {
    let mut d = Doc::new("page");
    let card = d.add(0, "div.card");
    let acts = d.add(card, "div.actions");
    let btn = d.add(acts, "button.btn");
    let title = d.add(card, "div.title");
    d.text(title, "Name");
    let css = ".card:hover .title { color: #f00 } .card :is(.actions button, .actions a):not(.icon) { width: 33px } .title::before { content: \"\\201C\"; } .title::after { content: '\\00a0\\2192' }";
    let mut s = scene(&d, css, (800.0, 600.0));
    assert_eq!(s.rects[btn][2], 33.0);
    let before = s.doc.nodes[title].before.unwrap();
    assert_eq!(s.doc.nodes[before].text.as_deref(), Some("\u{201c}"));
    assert_eq!(
        s.doc.nodes[s.doc.nodes[title].after.unwrap()]
            .text
            .as_deref(),
        Some("\u{a0}\u{2192}")
    );
    s.update(
        Interaction {
            hover: Some(card),
            ..Default::default()
        },
        Instant::now(),
        &mut approx,
    )
    .unwrap();
    assert_eq!(s.style(title).color, Color::rgba(255, 0, 0, 255));
    assert_eq!(
        s.style(before).color,
        Color::rgba(255, 0, 0, 255),
        "generated leaves follow their element"
    );
}

#[test]
fn spec_style_font_shorthand_and_title_case_rules() {
    let (s, n) = solo(
        ".t { font: 800 48px/0.95 Inter, 'Noto Sans', sans-serif; text-transform: uppercase; letter-spacing: -0.03em }",
    );
    let c = s.style(n);
    assert_eq!((c.font_size, c.weight), (48.0, 800));
    assert!((c.letter_spacing + 1.44).abs() < 1e-4);
    assert_eq!(c.line_height, props::LineH::Mult(0.95));
    assert_eq!(s.text_style(n).line_px(), 48.0 * 0.95);
    let (s, n) = solo(".t { font-size: 13px; line-height: 17.875px }");
    assert!((s.text_style(n).line_height - 1.375).abs() < 1e-4);
}

#[test]
fn spec_style_active_press_and_z_layers() {
    let mut d = Doc::new("page");
    let b = d.add(0, "button.sp-btn");
    let css = ".sp-btn { width: 40px; height: 40px; transform: scale(1); transition: transform 160ms ease } .sp-btn:active { transform: scale(.98) }";
    let mut s = scene(&d, css, (800.0, 600.0));
    let t0 = Instant::now();
    s.update(
        Interaction {
            active: Some(b),
            ..Default::default()
        },
        t0,
        &mut approx,
    )
    .unwrap();
    assert!(s.animating());
    s.tick(t0 + Duration::from_millis(500));
    assert_eq!(s.style(b).xf.sx, 0.98);
}

#[test]
fn keyframes_animate_opacity_and_transform() {
    use std::time::{Duration, Instant};
    let (d, a, _, _) = super::small();
    let mut sc = super::scene(
        &d,
        "@keyframes pulse { from { opacity: 0 } 50% { opacity: 1; transform: scale(2) } to { opacity: 0.5 } } \
         .a { animation: pulse 1s linear infinite; width: 50px; height: 50px }",
        (400.0, 300.0),
    );
    assert!(sc.animating() && sc.decorative_only());
    let t0 = Instant::now();
    sc.tick(t0 + Duration::from_millis(500));
    let e = sc.eff(a);
    assert!((e.opacity.unwrap() - 1.0).abs() < 0.05, "{:?}", e.opacity);
    assert!((e.xf.unwrap().sx - 2.0).abs() < 0.05);
    // Endless: the next iteration starts over.
    sc.tick(t0 + Duration::from_millis(1250));
    let e = sc.eff(a);
    assert!((e.opacity.unwrap() - 0.5).abs() < 0.1, "{:?}", e.opacity);
}

#[test]
fn finite_animation_ends_and_forwards_holds() {
    use std::time::{Duration, Instant};
    let (d, a, _, _) = super::small();
    let mut sc = super::scene(
        &d,
        "@keyframes f { from { opacity: 0 } to { opacity: 0.25 } } .a { animation: f 100ms forwards; width: 5px; height: 5px }",
        (400.0, 300.0),
    );
    sc.tick(Instant::now() + Duration::from_millis(500));
    assert!(!sc.animating(), "finished: no more frames");
    assert_eq!(
        sc.eff(a).opacity,
        Some(0.25),
        "forwards keeps the last frame"
    );
}

#[test]
fn unset_and_initial_restore_the_default() {
    let (d, a, _, _) = super::small();
    let sc = super::scene(
        &d,
        ".a { margin-top: -40px; padding-left: 9px; height: 30px } .a { margin-top: unset; padding-left: initial }",
        (400.0, 300.0),
    );
    assert_eq!(sc.rects[a][1], 0.0);
    assert_eq!(sc.insets[a][0], 0.0);
}
