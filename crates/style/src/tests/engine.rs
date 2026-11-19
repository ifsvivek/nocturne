use super::*;
use std::time::Duration;

const DEFAULT_CSS: &str = ".x { width: 40px; height: 10px } .y { width: 60px; height: 10px }";

fn doc() -> Doc {
    let mut d = Doc::new("page");
    d.add(0, "div.x");
    d.add(0, "div.y");
    d
}

fn build(e: &mut Engine, d: &Doc) -> Scene {
    e.build(
        d,
        (800.0, 600.0),
        Interaction::default(),
        &Builders::new(),
        &mut approx,
    )
}

#[test]
fn user_sheet_layers_over_the_default() {
    let mut e = Engine::new(DEFAULT_CSS);
    let s = build(&mut e, &doc());
    assert_eq!((s.rects[1][2], s.rects[2][2]), (40.0, 60.0));
    e.set_user_css(Some(".x { width: 70px }"));
    let s = build(&mut e, &doc());
    assert_eq!((s.rects[1][2], s.rects[2][2]), (70.0, 60.0));
    assert!(e.user_sheet_active());
    e.set_user_css(None);
    assert_eq!(build(&mut e, &doc()).rects[1][2], 40.0);
}

#[test]
fn safe_mode_ignores_the_user_sheet_and_toggles() {
    let mut e = Engine::new(DEFAULT_CSS);
    e.set_user_css(Some(".x { width: 70px }"));
    e.set_safe_mode(true);
    assert!(e.safe_mode() && !e.user_sheet_active());
    assert_eq!(build(&mut e, &doc()).rects[1][2], 40.0);
    // The key chord handler.
    assert!(!e.toggle_safe_mode());
    assert_eq!(build(&mut e, &doc()).rects[1][2], 70.0);
    assert!(e.toggle_safe_mode());
    assert_eq!(build(&mut e, &doc()).rects[1][2], 40.0);
}

#[test]
fn safe_mode_environment_variable_values() {
    assert!(!safe_env_enabled(None));
    for off in ["", "0", " 0 ", "false", "FALSE"] {
        assert!(!safe_env_enabled(Some(off)), "{off:?}");
    }
    for on in ["1", "true", "yes", "anything"] {
        assert!(safe_env_enabled(Some(on)), "{on:?}");
    }
    assert_eq!(SAFE_ENV, "NOCTURNE_SAFE_THEME");
    assert_eq!(SAFE_CHORD, "Ctrl+Shift+0");
}

#[test]
fn a_panic_in_the_engine_falls_back_to_the_default_look() {
    let mut e = Engine::new(DEFAULT_CSS);
    e.set_user_css(Some(".x { width: 70px }"));
    let mut d = doc();
    d.add(0, "panic-test");
    // Silence the expected panic message.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let s = build(&mut e, &d);
    // The default scene also meets the panic-test element, so the engine ends on the bare sheet: still a scene.
    std::panic::set_hook(hook);
    assert!(s.rects[0][2] > 0.0);
    let notes = e.take_notices();
    assert!(
        notes.iter().any(|n| n.contains("layout engine fail")),
        "{notes:?}"
    );
    assert!(e.take_notices().is_empty());
}

#[test]
fn a_slow_theme_falls_back_and_is_switched_off_after_three_overruns() {
    let mut e = Engine::new(DEFAULT_CSS);
    e.set_user_css(Some(".x { width: 70px }"));
    e.set_budget(Duration::from_nanos(1));
    let mut d = Doc::new("page");
    for _ in 0..200 {
        d.add(0, "div.x");
    }
    for round in 1..=3 {
        let s = build(&mut e, &d);
        assert_eq!(
            s.rects[1][2], 40.0,
            "the default look is shown (round {round})"
        );
    }
    assert!(!e.user_sheet_active());
    let notes = e.take_notices();
    assert!(
        notes.iter().any(|n| n.contains("switched off")),
        "{notes:?}"
    );
    // Setting a theme again re-enables it (with a sane budget).
    e.set_budget(Duration::from_secs(5));
    e.set_user_css(Some(".x { width: 70px }"));
    assert_eq!(build(&mut e, &d).rects[1][2], 70.0);
}

#[test]
fn fonts_registered_at_runtime_reach_the_scene() {
    let mut e = Engine::new("");
    e.fonts_mut().set_installed(["Noto Sans".to_string()]);
    e.set_user_css(Some(
        "div { font-family: 'Inter', 'Noto Sans', sans-serif }",
    ));
    let mut d = Doc::new("page");
    let t = d.add(0, "div");
    d.text(t, "x");
    let mut s = build(&mut e, &d);
    assert_eq!(s.text_style(t).font, FontChoice::Named("Noto Sans".into()));
    e.fonts_mut().register("Inter");
    s.set_fonts(e.fonts.clone(), &mut approx);
    assert_eq!(s.text_style(t).font, FontChoice::Named("Inter".into()));
}

#[test]
fn warnings_survive_the_engine() {
    let mut e = Engine::new("");
    e.set_user_css(Some(".x { frob: 1; cursor: pointer }"));
    let s = build(&mut e, &doc());
    let r = s.warnings();
    assert_eq!(
        (r.count(Kind::UnsupportedProperty), r.count(Kind::NoEffect)),
        (1, 1)
    );
}
