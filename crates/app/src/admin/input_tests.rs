//! Real input through Iced's headless harness (`iced_test`): the confirmation dialog's text inputs.
use super::*;
use iced::keyboard::{Key, key::Named};
use iced_test::{Simulator, selector};
use nocturne_core::admin::confirm::Field;

/// `task::after` creates a timer when the task is built, so tests need a runtime to be inside of.
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap()
}

/// An admin screen on a dead loopback port: no request can reach any server.
fn admin() -> Admin {
    let cfg = Config {
        base_url: "http://127.0.0.1:1".into(),
        token: "x".into(),
    };
    let t = Target {
        origin: SourceId(1),
        label: "Test".into(),
        cfg: cfg.clone(),
        pics: None,
    };
    Admin::with_client(t, AdminClient::new(cfg).unwrap(), true).0
}

fn with_fields(a: &mut Admin, fields: Vec<Field>) {
    let mut d = Act::RefreshAll.dialog();
    for f in fields {
        d = d.field(f);
    }
    a.dialog = Some(d.start());
}

/// One character at a time with a fresh view each time, like the real app's redraws.
fn type_into(a: &mut Admin, i: usize, text: &str) {
    for c in text.chars() {
        let mut sim = Simulator::new(a.view());
        sim.click(selector::id(field_id(i)))
            .expect("dialog field is on screen");
        sim.tap_key(Key::Named(Named::End));
        sim.typewrite(&c.to_string());
        for m in sim.into_messages().collect::<Vec<_>>() {
            let _ = a.update(m);
        }
    }
}

#[test]
fn the_confirmation_field_accepts_typing_and_validates_it() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin();
    with_fields(&mut a, vec![Field::typed("Type NETWORK", "NETWORK")]);
    type_into(&mut a, 0, "NETWORK");
    let f = &a.dialog.as_ref().unwrap().fields[0];
    assert_eq!(f.value, "NETWORK");
    assert!(f.ok());
}

#[test]
fn a_numeric_field_keeps_digits_only() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin();
    let mut port = Field::new("Port", "");
    port.numeric = true;
    with_fields(&mut a, vec![port]);
    type_into(&mut a, 0, "8a0-9 6");
    assert_eq!(a.dialog.as_ref().unwrap().fields[0].value, "8096");
}

#[test]
fn enter_in_a_dialog_field_reports_a_submit() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin();
    with_fields(&mut a, vec![Field::typed("Type NETWORK", "NETWORK")]);
    let mut sim = Simulator::new(a.view());
    sim.click(selector::id(field_id(0))).unwrap();
    sim.tap_key(Key::Named(Named::Enter));
    assert!(
        sim.into_messages()
            .any(|m| matches!(m, Msg::DialogSubmit(0)))
    );
}
