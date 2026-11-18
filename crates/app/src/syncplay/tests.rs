use super::*;
use nocturne_jellyfin::Config;
use nocturne_jellyfin_user::Session;

fn key(n: &str) -> Key {
    Key {
        name: n.into(),
        text: (n.chars().count() == 1).then(|| n.to_string()),
        ..Default::default()
    }
}

/// `task::after` creates a timer when the task is built, so tests need a runtime to be inside of.
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap()
}

/// A server on a dead loopback port: nothing can reach a real one.
fn target(o: u32, label: &str) -> Target {
    let url = "http://127.0.0.1:1";
    let s = Session {
        access_token: "x".into(),
        user_id: "u".into(),
        server_id: "s".into(),
        user_name: "alice".into(),
        is_admin: false,
    };
    Target {
        origin: SourceId(o),
        label: label.into(),
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

fn group() -> GroupInfo {
    serde_json::from_value(serde_json::json!({
        "GroupId": "g1", "GroupName": "Movie night", "State": "Idle", "Participants": ["alice"]
    }))
    .unwrap()
}

#[test]
fn the_token_is_made_in_one_place() {
    let call = ["i_have_asked", "_the_user("].concat();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/syncplay");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            n += std::fs::read_to_string(p).unwrap().matches(&call).count();
        }
    }
    assert_eq!(n, 1, "only `user_confirmed` may make a UserConfirmed");
}

#[test]
fn keys_do_what_the_gpui_screen_did() {
    use KeyAct::*;
    let k = |n: &str, g, f| key_act(&key(n), g, f);
    assert_eq!(k("escape", false, false), Close);
    assert_eq!(k("tab", false, false), SwitchFocus);
    assert_eq!(
        k("tab", true, false),
        None,
        "no list or name while in a group"
    );
    assert_eq!(k("r", false, false), Refresh);
    assert_eq!(k("r", false, true), None, "the name field takes typed keys");
    assert_eq!(
        k("r", true, true),
        Refresh,
        "in a group there is no name field"
    );
    assert_eq!(k("l", true, false), Leave);
    assert_eq!(k("l", false, false), None);
    assert_eq!(k("enter", true, false), Follow);
    assert_eq!(k("enter", false, true), Create);
    assert_eq!(k("enter", false, false), JoinSelected);
    assert_eq!(k("down", false, false), Down);
    assert_eq!(k("down", true, false), None);
    assert_eq!(k("]", false, false), Server(true));
    assert_eq!(k("[", false, true), None);
    let ctrl = |n: &str| Key {
        ctrl: true,
        ..key(n)
    };
    assert_eq!(
        key_act(&ctrl("l"), false, true),
        None,
        "ctrl chords type nothing"
    );
}

#[test]
fn gate_follows_dev_mode_writes_and_policy() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target(1, "A")], false);
    assert_eq!(sp.gate(), Gate::NeedWrites);
    sp.writes = true;
    assert_eq!(sp.gate(), Gate::Create);
    sp.access = Some(SyncAccess::JoinGroups);
    assert_eq!(sp.gate(), Gate::Join);
    sp.access = Some(SyncAccess::None);
    assert_eq!(sp.gate(), Gate::NoAccess);
    let dev = SyncPlay::new(vec![], true);
    assert_eq!(dev.gate(), Gate::Dev);
    assert!(dev.can_open(), "the screen explains why it is read-only");
    assert!(
        !SyncPlay::new(vec![], false).can_open()
            || SyncPlay::new(vec![], false).gate() == Gate::Dev
    );
}

#[test]
fn without_the_master_switch_nothing_is_sent() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target(1, "A")], false);
    assert!(
        sp.write_client().is_none(),
        "create / join need server changes on"
    );
    assert!(matches!(sp.outcomes.pop(), Some(Outcome::Toast(t)) if t == NEED_WRITES));
    sp.writes = true;
    assert!(sp.write_client().is_some());
    assert!(
        sp.group_client().is_none(),
        "group traffic only inside a group"
    );
    sp.engine.join(group(), now_ms());
    assert!(sp.group_client().is_some());
    let dev = SyncPlay::new(vec![target(1, "A")], true);
    assert!(dev.group_client().is_none());
}

#[test]
fn turning_writes_off_leaves_the_group_and_leaving_always_works() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target(1, "A")], false);
    sp.writes = true;
    sp.engine.join(group(), now_ms());
    assert!(sp.in_group());
    let _ = sp.set_writes(false, None);
    assert!(!sp.in_group(), "no group without the master switch");
    assert!(matches!(sp.outcomes.pop(), Some(Outcome::Toast(t)) if t == "You left the group."));
    // Leave with writes already off still works.
    sp.engine.join(group(), now_ms());
    let _ = sp.leave(None);
    assert!(!sp.in_group());
}

#[test]
fn the_group_decides_the_server() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target(1, "Home"), target(2, "Archive")], false);
    assert_eq!(sp.origin(), Some(SourceId(1)), "default: the first source");
    sp.use_source(SourceId(2));
    assert_eq!(sp.origin(), Some(SourceId(2)), "the playing item's server");
    let _ = sp.choose(0);
    assert_eq!(sp.origin(), Some(SourceId(1)), "the picker");
    sp.engine.join(group(), now_ms());
    sp.use_source(SourceId(2));
    let _ = sp.choose(1);
    assert_eq!(sp.origin(), Some(SourceId(1)), "locked while in a group");
    // Sources changing under a group keep its server too.
    sp.set_targets(vec![target(2, "Archive")]);
    assert_eq!(sp.targets.len(), 2);
    let _ = sp.leave(None);
    sp.set_targets(vec![target(2, "Archive")]);
    assert_eq!(sp.origin(), Some(SourceId(2)));
}

#[test]
fn answers_of_an_old_connection_are_dropped() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target(1, "Home"), target(2, "Archive")], false);
    sp.open = true;
    let old = sp.conn;
    let _ = sp.choose(1);
    assert!(sp.conn > old);
    let _ = sp.update(Msg::Groups(old, Some(vec![group()])), None);
    assert!(
        matches!(sp.groups, Load::Loading),
        "groups of the other server"
    );
    let _ = sp.update(Msg::Access(old, Some(SyncAccess::None)), None);
    assert!(sp.access.is_none());
    let _ = sp.update(Msg::Groups(sp.conn, Some(vec![group()])), None);
    assert!(matches!(&sp.groups, Load::Ready(v) if v.len() == 1));
}

#[test]
fn name_entry_is_limited_and_controls_are_ignored() {
    let rt = rt();
    let _g = rt.enter();
    let mut sp = SyncPlay::new(vec![target(1, "A")], false);
    let _ = sp.update(Msg::NameInput("Fri\u{7}day".into()), None);
    assert_eq!(sp.name, "Friday");
    assert!(sp.name_focus);
    sp.set_name(&"x".repeat(300));
    assert_eq!(sp.name.len(), 100);
    // The screen has no name field in a group.
    sp.engine.join(group(), now_ms());
    let _ = sp.key(&key("z"), None);
    assert_eq!(sp.name.len(), 100);
}

#[test]
fn the_socket_drain_exists_only_with_a_socket() {
    let rt = rt();
    let _g = rt.enter();
    let sp = SyncPlay::new(vec![target(1, "A")], false);
    assert!(sp.rx.is_none(), "no socket, no 50 ms timer");
}
