use super::*;
use nocturne_core::admin::state::L;

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

/// An admin screen on a dead loopback port: no request can reach any server.
fn admin(writes: bool) -> Admin {
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
    let (a, _) = Admin::with_client(t, AdminClient::new(cfg).unwrap(), writes);
    a
}

fn files(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
}

/// The `Confirmed` token must be minted in exactly one place (`Admin::on_confirm`), and nothing
/// else in the admin UI, its preview or the core admin modules may mint it.
#[test]
fn single_confirm_choke_point() {
    let call = ["i_have_asked", "_the_user("].concat();
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/admin");
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/src/admin");
    let mut all = vec![];
    files(&ui, &mut all);
    files(&core, &mut all);
    assert!(all.len() >= 10, "the scan found the admin files");
    let hits: Vec<&str> = all
        .iter()
        .filter(|(_, s)| s.contains(&call))
        .map(|(p, _)| p.as_str())
        .collect();
    assert_eq!(hits.len(), 1, "minted in: {hits:?}");
    assert!(hits[0].ends_with("admin/mod.rs"));
    let src = include_str!("mod.rs");
    assert_eq!(src.matches(&call).count(), 1);
    assert!(src.contains(&format!("Confirmed::{call})")));
    // ... and inside the one function that runs after the dialog.
    let at = src.find(&call).unwrap();
    assert!(src[..at].rfind("fn on_confirm").unwrap() > src[..at].rfind("fn dialog_out").unwrap());
}

/// Walk the dialog to its confirm button: Cancel is the default focus, Enter there never writes.
#[test]
fn dialog_needs_the_confirm_button_and_writes_only_then() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin(true);
    let _ = a.do_act(Act::RefreshAll);
    assert!(a.dialog_open(), "a write opens its dialog first");
    assert!(!a.writing);
    // Enter on the default focus (Cancel) closes without writing.
    let _ = a.key(&key("enter"));
    assert!(!a.dialog_open() && !a.writing);
    // Right moves to Confirm; only the next Enter confirms.
    let _ = a.do_act(Act::RefreshAll);
    let _ = a.key(&key("right"));
    assert!(a.dialog_open() && !a.writing, "focus alone does not write");
    let _ = a.key(&key("enter"));
    assert!(
        !a.dialog_open() && a.writing,
        "confirmed: the write started"
    );
}

#[test]
fn writes_off_is_view_only() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin(false);
    let _ = a.do_act(Act::RefreshAll);
    assert!(!a.dialog_open(), "no dialog without the master switch");
    assert_eq!(a.note.as_ref().map(|n| n.0.as_str()), Some(NEED_WRITES));
    // Turning the switch off while a dialog is up drops it, and a late confirm does nothing.
    let mut b = admin(true);
    let _ = b.do_act(Act::RefreshAll);
    let _ = b.key(&key("right"));
    b.set_writes(false);
    assert!(!b.dialog_open());
    let _ = b.on_confirm();
    assert!(!b.writing);
}

#[test]
fn invalid_input_blocks_the_write() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin(true);
    let _ = a.do_act(Act::Restart);
    // Restart has no inputs; shutdown too. Use a typed-word dialog: Networking needs NETWORK.
    a.dialog = Some(
        Act::RefreshAll
            .dialog()
            .field(nocturne_core::admin::confirm::Field::typed(
                "Type NETWORK",
                "NETWORK",
            ))
            .start(),
    );
    let _ = a.key(&key("down")); // Cancel
    let _ = a.key(&key("right")); // Confirm
    let _ = a.key(&key("enter"));
    assert!(!a.writing, "the typed word is missing");
}

#[test]
fn answers_for_another_server_or_page_are_dropped() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin(false);
    let g = a.gen_;
    let pid = a.st.pid;
    let _ = a.update(Msg::Data(
        g,
        pid,
        Handoff::new(state::Msg::Users(Some(vec![]))),
    ));
    assert!(matches!(a.st.data.users, L::Ready(_)));
    let _ = a.update(Msg::Data(
        g + 1,
        pid,
        Handoff::new(state::Msg::Tasks(Some(vec![]))),
    ));
    assert!(
        matches!(a.st.data.tasks, L::Loading),
        "another server generation"
    );
    let _ = a.update(Msg::Data(
        g,
        pid + 7,
        Handoff::new(state::Msg::Tasks(Some(vec![]))),
    ));
    assert!(matches!(a.st.data.tasks, L::Loading), "another page");
}

#[test]
fn keyboard_walks_the_sidebar_and_leaves() {
    let rt = rt();
    let _g = rt.enter();
    let mut a = admin(false);
    assert_eq!(a.st.section, Section::Dashboard);
    let _ = a.key(&key("down"));
    assert_eq!(a.st.section, Section::General);
    assert!(a.st.in_side);
    let _ = a.key(&key("escape"));
    assert!(matches!(a.outcomes.pop(), Some(Outcome::Close)));
    assert!(!a.st.open);
}

#[test]
fn picks_the_first_admin_and_switching_resets_the_screen() {
    let rt = rt();
    let _g = rt.enter();
    let cfg = |p: u16| Config {
        base_url: format!("http://127.0.0.1:{p}"),
        token: "x".into(),
    };
    let t = |o: u32, p: u16| Target {
        origin: SourceId(o),
        label: format!("S{o}"),
        cfg: cfg(p),
        pics: None,
    };
    let (mut a, _) = Admin::open(vec![t(1, 1), t(2, 2), t(3, 3)], false);
    let c = |p| Some(AdminClient::new(cfg(p)).unwrap());
    let _ = a.update(Msg::Probed(a.gen_, 1, c(2)));
    assert!(a.chosen.is_none(), "server 1 has not answered yet");
    let _ = a.update(Msg::Probed(a.gen_, 0, None));
    assert_eq!(a.origin(), Some(SourceId(2)), "first administrator wins");
    let _ = a.update(Msg::Probed(a.gen_, 2, c(3)));
    let g = a.gen_;
    let _ = a.key(&key("tab"));
    assert_eq!(a.origin(), Some(SourceId(3)));
    assert!(a.gen_ > g, "answers of the old server are now stale");
    assert_eq!(a.st.section, Section::Dashboard);
    // Nobody an administrator: the screen says so and Esc leaves.
    let (mut n, _) = Admin::open(vec![t(1, 1)], false);
    let _ = n.update(Msg::Probed(n.gen_, 0, None));
    assert!(n.client.is_none());
    let _ = n.key(&key("escape"));
    assert!(matches!(n.outcomes.pop(), Some(Outcome::Close)));
}

#[test]
fn selected_row_is_kept_in_view() {
    let rt = rt();
    let _g = rt.enter();
    use nocturne_core::admin::model::{Item, Model};
    let m = Model {
        items: (0..30)
            .map(|i| Item {
                title: format!("r{i}"),
                group: Some(if i < 10 { "A" } else { "B" }.into()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    assert_eq!(row_span(&m, 0), (0.0, GROUP_H + ROW_H));
    let (t1, b1) = row_span(&m, 1);
    assert_eq!(t1, GROUP_H + ROW_H);
    assert_eq!(b1 - t1, ROW_H);
    let (t10, _) = row_span(&m, 10);
    assert_eq!(
        t10,
        GROUP_H + 10.0 * ROW_H,
        "a new group starts with its header"
    );
}
