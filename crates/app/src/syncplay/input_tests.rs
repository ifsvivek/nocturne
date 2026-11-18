//! Real input through Iced's headless harness (`iced_test`): the group-name field.
use super::*;
use iced::keyboard::{Key, key::Named};
use iced_test::{Simulator, selector};
use nocturne_jellyfin::Config;
use nocturne_jellyfin_user::Session;

/// `task::after` creates a timer when the task is built, so tests need a runtime to be inside of.
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap()
}

/// A server on a dead loopback port: nothing can reach a real one.
fn target() -> Target {
    let url = "http://127.0.0.1:1";
    let s = Session {
        access_token: "x".into(),
        user_id: "u".into(),
        server_id: "s".into(),
        user_name: "alice".into(),
        is_admin: false,
    };
    Target {
        origin: SourceId(1),
        label: "A".into(),
        client: UserClient::from_session(url, &s).unwrap(),
        user_name: "alice".into(),
        source: Source::with_user(
            Config {
                base_url: url.into(),
                token: "x".into(),
            },
            "u",
        )
        .unwrap(),
    }
}

#[test]
fn typing_a_group_name_reaches_the_name_field_and_enter_creates() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target()], false);
    sp.open = true;
    sp.writes = true;
    sp.access = Some(SyncAccess::CreateAndJoinGroups);
    assert!(sp.gate().can_create());
    let win = Size::new(1280.0, 800.0);
    for c in "Movie night".chars() {
        let mut sim = Simulator::new(sp.view(win).unwrap());
        sim.click(selector::id(NAME_ID))
            .expect("name field is on screen");
        sim.tap_key(Key::Named(Named::End));
        sim.typewrite(&c.to_string());
        for m in sim.into_messages().collect::<Vec<_>>() {
            let _ = sp.update(m, None);
        }
    }
    assert_eq!(sp.name, "Movie night");
    let mut sim = Simulator::new(sp.view(win).unwrap());
    sim.click(selector::id(NAME_ID)).unwrap();
    sim.tap_key(Key::Named(Named::Enter));
    assert!(sim.into_messages().any(|m| matches!(m, Msg::Create)));
}
