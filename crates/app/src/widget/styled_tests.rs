//! Real input through Iced's headless harness (`iced_test`) against the styled widget and its slots: typing into a `text_input`
//! slot, a hole slot reporting its bounds, hover, press, click and wheel mapped to elements, navigation keys, the safe-mode chord.
use super::hole::hole;
use super::styled::{Nav, Styled, StyledEvent};
use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::widget::{Id as WidgetId, text_input};
use iced::{Element, Event, Point, Rectangle, Size, mouse, window};
use iced_test::{Simulator, selector};
use nocturne_style::{Builders, Dir, Doc, Fonts, Interaction, Scene, Sheet, approx};
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

/// The headless renderer is not safe to start from many threads at once (a parallel run crashed): one simulator at a time.
fn serial() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug, PartialEq)]
enum T {
    Ev(StyledEvent),
    Typed(String),
    Hole(Rectangle),
}

const CSS: &str = ".bar { height: 60px } .search { margin: 10px; width: 200px; height: 30px } .btn { width: 80px; height: 30px; margin: 10px }";

/// page > .bar > [.search (slot 1, input), .btn (focusable)], page > .video (slot 2, hole)
fn scene(css: &str) -> Scene {
    let mut d = Doc::new("page");
    let bar = d.add(0, "div.bar");
    let s = d.add(bar, "div.search");
    d.slot(s, 1);
    let b = d.add(bar, "a.btn");
    d.focusable(b);
    let v = d.add(0, "div.video");
    d.inline(v, "height: 100px; margin: 20px");
    d.slot(v, 2);
    Scene::build(
        d,
        Rc::new(Sheet::parse(css)),
        Rc::new(Fonts::new()),
        (1024.0, 768.0),
        Interaction::default(),
        None,
        &Builders::new(),
        &mut approx,
    )
    .expect("builds")
}

fn view<'a>(s: &'a Scene, typed: &'a str) -> Element<'a, T> {
    Styled::new(s, T::Ev)
        .slot(
            1,
            text_input("Search", typed)
                .id(WidgetId::new("in"))
                .on_input(T::Typed),
        )
        .slot(2, hole(Rectangle::default(), true, T::Hole))
        .into()
}

fn node_of(s: &Scene, f: impl Fn(&nocturne_style::doc::Node) -> bool) -> usize {
    (0..s.doc.len())
        .find(|&i| f(&s.doc.nodes[i]))
        .expect("node")
}

/// One character at a time with a fresh view each time, like the real app's redraws.
fn type_into(s: &Scene, typed: &mut String, text: &str) -> Vec<T> {
    let mut all = vec![];
    for c in text.chars() {
        let mut sim = Simulator::new(view(s, typed));
        sim.click(selector::id(WidgetId::new("in")))
            .expect("the slot is on screen");
        sim.tap_key(Key::Named(Named::End));
        sim.typewrite(&c.to_string());
        for m in sim.into_messages().collect::<Vec<_>>() {
            if let T::Typed(t) = &m {
                typed.clone_from(t);
            }
            all.push(m);
        }
    }
    all
}

#[test]
fn typing_works_in_a_text_input_slot() {
    let _g = serial();
    let s = scene(CSS);
    let mut typed = String::new();
    let msgs = type_into(&s, &mut typed, "hey");
    assert_eq!(typed, "hey");
    // The styled tree saw none of it as navigation.
    assert!(
        !msgs
            .iter()
            .any(|m| matches!(m, T::Ev(StyledEvent::Nav(_) | StyledEvent::SafeMode)))
    );
}

#[test]
fn the_slot_sits_at_the_rectangle_of_its_element() {
    let _g = serial();
    let s = scene(CSS);
    let n = node_of(&s, |n| n.slot == Some(1));
    let [x, y, w, h] = s.screen_rect(n);
    assert_eq!((x, y, w, h), (10.0, 10.0, 200.0, 30.0));
    let mut sim = Simulator::new(view(&s, ""));
    let target = sim.find(selector::id(WidgetId::new("in"))).expect("found");
    assert_eq!(
        target.visible_bounds().expect("visible"),
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    );
}

#[test]
fn a_hole_slot_reports_its_bounds() {
    let _g = serial();
    let s = scene(CSS);
    let mut sim = Simulator::new(view(&s, ""));
    sim.simulate([Event::Window(
        window::Event::RedrawRequested(Instant::now()),
    )]);
    let msgs: Vec<T> = sim.into_messages().collect();
    let n = node_of(&s, |n| n.slot == Some(2));
    let [x, y, w, h] = s.screen_rect(n);
    assert!(
        msgs.contains(&T::Hole(Rectangle::new(Point::new(x, y), Size::new(w, h)))),
        "{msgs:?}"
    );
    // The widget also reports its own size on the first redraw.
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Resize(Size::new(1024.0, 768.0)))),
        "{msgs:?}"
    );
}

#[test]
fn hover_press_click_and_wheel_are_reported_with_the_element() {
    let _g = serial();
    let s = scene(CSS);
    let btn = node_of(&s, |n| n.classes.iter().any(|c| c == "btn"));
    let [x, y, w, h] = s.screen_rect(btn);
    let at = Point::new(x + w / 2.0, y + h / 2.0);
    let mut sim = Simulator::new(view(&s, ""));
    sim.point_at(at);
    sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: at })]);
    sim.simulate(iced_test::simulator::click());
    sim.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -30.0 },
    })]);
    let msgs: Vec<T> = sim.into_messages().collect();
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Hover(Some(btn)))),
        "{msgs:?}"
    );
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Active(Some(btn)))),
        "{msgs:?}"
    );
    assert!(msgs.contains(&T::Ev(StyledEvent::Click(btn))), "{msgs:?}");
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Wheel {
            target: Some(btn),
            dx: 0.0,
            dy: -30.0
        })),
        "{msgs:?}"
    );
}

#[test]
fn keys_the_slot_uses_are_not_navigation_but_others_are() {
    let _g = serial();
    let s = scene(CSS);
    let mut sim = Simulator::new(view(&s, ""));
    sim.click(selector::id(WidgetId::new("in"))).unwrap();
    sim.typewrite("a");
    sim.tap_key(Key::Named(Named::Tab));
    let msgs: Vec<T> = sim.into_messages().collect();
    // Typing is the input's; Tab, which it does not use, reaches the page as navigation.
    assert!(msgs.contains(&T::Typed("a".into())));
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Nav(Nav::Next))),
        "{msgs:?}"
    );
    // Without a focused input every navigation key is the page's.
    let mut sim = Simulator::new(view(&s, ""));
    sim.tap_key(Key::Named(Named::ArrowRight));
    sim.tap_key(Key::Named(Named::Enter));
    let msgs: Vec<T> = sim.into_messages().collect();
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Nav(Nav::Dir(Dir::Right)))),
        "{msgs:?}"
    );
    assert!(
        msgs.contains(&T::Ev(StyledEvent::Nav(Nav::Activate))),
        "{msgs:?}"
    );
}

#[test]
fn the_safe_mode_chord_wins_over_a_focused_slot() {
    let _g = serial();
    let s = scene(CSS);
    let mut sim = Simulator::new(view(&s, ""));
    sim.click(selector::id(WidgetId::new("in"))).unwrap();
    let chord = Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character("t".into()),
        modified_key: Key::Character("t".into()),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyT),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::CTRL | Modifiers::ALT,
        text: None,
        repeat: false,
    });
    sim.simulate([chord]);
    let msgs: Vec<T> = sim.into_messages().collect();
    assert!(msgs.contains(&T::Ev(StyledEvent::SafeMode)), "{msgs:?}");
    assert!(!msgs.iter().any(|m| matches!(m, T::Typed(_))));
}

#[test]
fn a_hidden_slot_takes_no_space_and_no_input() {
    let _g = serial();
    let s = scene(&format!("{CSS} .search {{ display: none }}"));
    let mut sim = Simulator::new(view(&s, ""));
    assert!(sim.click(selector::id(WidgetId::new("in"))).is_err());
}
