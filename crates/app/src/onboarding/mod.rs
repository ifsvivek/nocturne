//! First-run walkthrough (0.3): Welcome, Where is your media, Set up, How NOCTURNE behaves,
//! You're set. Upgraders from 0.2 get a two-step "What's new in 0.3" instead. Replaces
//! `app/welcome.rs` (same three choices, same arrow / Enter behaviour). Self-contained: it owns its
//! step state and focus, never touches a source or a setting itself, and talks to the root only
//! through [`Outcome`]s and [`World`] snapshots.
//!
//! ## Root hook (for the master)
//!
//! ```ignore
//! // 1. Decide at startup (after the sources load, replacing `needs_welcome`):
//! //    done = settings.onboarding, migrated = the profile list came from the 0.2 servers.toml / keyring
//! //    migration on this launch (any source + onboarding == 0 + migrated -> upgrader path).
//! if let Some(mode) = onboarding::due(settings.onboarding, !sources.is_empty(), migrated) {
//!     self.onboarding = Some(onboarding::Onboarding::new(mode, self.world()));   // Phase::Account-like page, no header
//! }
//! // "Settings > Run setup again" (new row/link in app/settings.rs):  Onboarding::new(Mode::Fresh, world)
//!
//! // 2. Forwarding
//! Message::Onboarding(m) => { ob.update(m); self.drain_onboarding() }
//! key                    => if ob.key(&key) { self.drain_onboarding() }   // owns ALL keys while it is up
//! view                   => ob.view(Instant::now()).map(Message::Onboarding)   // paints no background: the root's
//!                           // backdrop (dots + glow) goes behind it, like welcome.rs; no header, no breadcrumbs
//! subscription          => if ob.animating(now) { window::frames() }     // slide/fade and the indexing pulse
//! resize / every change => ob.set_win(size); ob.set_world(self.world())   // cheap, call after any source,
//!                           // indexing or settings change (see `World`)
//! ob.capturing()        => always true: no global shortcuts (search, ?, account) while it is up
//!
//! // 3. Drain `ob.outcomes` after every update/key:
//! Outcome::AddFolders      -> hide onboarding (keep it in `self.onboarding`), open the existing local-folders
//!                             screen / chooser. When that screen closes (Done, Back or Esc, with or without
//!                             folders added) call `ob.returned(Return::Folders)` and show onboarding again.
//!                             Indexing keeps running; report it through `World.indexing`.
//! Outcome::AddServer       -> same with the server address + sign-in flow (`account/`); on its end (signed in or
//!                             cancelled) `ob.returned(Return::Server)`. The new server shows up in
//!                             `World.servers` ("Connected to <name> as <user>").
//! Outcome::SetWrites(on)   -> settings.allow_server_writes = on; save. (SetSync only comes while writes are on;
//!                             turning writes off leaves `sync_progress` stored but inert, as everywhere else.)
//! Outcome::SetSync(on)     -> settings.sync_progress = on; save.
//! Outcome::Finish{skipped, add_folders}
//!                          -> settings.onboarding = nocturne_core::settings::ONBOARDING; save; drop `self.onboarding`;
//!                             go Home (or, with no source at all, the empty-library state / Sources screen: the
//!                             walkthrough comes back only through "Run setup again" or when the user later
//!                             removes every source). `add_folders` (upgrader's button): then open the local-folders
//!                             screen as a normal page.
//! ```
//!
//! Resume handshake: an Outcome that leaves the walkthrough (`AddFolders`, `AddServer`) sets a
//! private `pending` marker; `returned` with the matching [`Return`] clears it and refreshes the
//! focus (next unfinished row, else Continue). A stale or mismatched `returned` is ignored, so a
//! root that forgets to call it cannot wedge the flow (the user simply lands back on the same
//! step), and one that calls it twice cannot double-advance.
mod view;

#[cfg(debug_assertions)]
pub mod preview;

use crate::keys::Key;
use crate::{motion, theme as t};
use iced::Size;
use nocturne_core::sources::FirstRun;
use std::time::Instant;

/// The revision this build writes to `Settings::onboarding` when the walkthrough ends.
pub const VERSION: u32 = nocturne_core::settings::ONBOARDING;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Fresh,
    /// 0.2 user: a migrated source exists, onboarding never done.
    Upgrade,
}

/// Should the walkthrough show at startup? No source at all always shows it (first run, or the
/// user removed everything). Otherwise it is due only if never finished; then the upgrader
/// variant when the sources came from the 0.2 migration, the full flow when someone quit
/// half-way through setup.
pub fn due(done: u32, has_sources: bool, migrated: bool) -> Option<Mode> {
    if !has_sources {
        return Some(Mode::Fresh);
    }
    if done >= VERSION {
        return None;
    }
    Some(if migrated { Mode::Upgrade } else { Mode::Fresh })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Where,
    SetUp,
    Behave,
    Ready,
    News1,
    News2,
}

impl Step {
    fn order(m: Mode) -> &'static [Step] {
        match m {
            Mode::Fresh => &[
                Step::Welcome,
                Step::Where,
                Step::SetUp,
                Step::Behave,
                Step::Ready,
            ],
            Mode::Upgrade => &[Step::News1, Step::News2],
        }
    }
}

/// One connected server, as the root knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub name: String,
    pub user: String,
}

/// What the root tells the walkthrough about the library so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    /// Display names of the local folders added so far.
    pub folders: Vec<String>,
    /// Files found so far while the local index runs (`None` = not indexing).
    pub indexing: Option<usize>,
    /// Files in the local index when idle.
    pub files: usize,
    pub servers: Vec<Server>,
    pub writes: bool,
    pub sync: bool,
    /// In-window video (Wayland) or the external mpv window (X11).
    pub embedded_video: bool,
}

impl Default for World {
    fn default() -> Self {
        Self {
            folders: vec![],
            indexing: None,
            files: 0,
            servers: vec![],
            writes: false,
            sync: false,
            embedded_video: true,
        }
    }
}

/// What the walkthrough asks the root to do (see the hook at the top).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    AddFolders,
    AddServer,
    SetWrites(bool),
    SetSync(bool),
    Finish { skipped: bool, add_folders: bool },
}

/// How a trip to another screen ended; answers `AddFolders` / `AddServer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Return {
    Folders,
    Server,
}

/// A focusable, activatable thing on a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Next,
    Choose(FirstRun),
    AddFolders,
    AddServer,
    Writes,
    Sync,
    NewsFolders,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Pointer over item `i`: focus follows it.
    Hover(usize),
    Press(usize),
    Back,
    Skip,
}

pub struct Onboarding {
    pub mode: Mode,
    pub step: Step,
    pub choice: Option<FirstRun>,
    pub focus: usize,
    pub world: World,
    pub win: Size,
    pub outcomes: Vec<Outcome>,
    pending: Option<Return>,
    moved: Instant,
    /// +1 forward, -1 back: the direction the new step slides in from.
    dir: f32,
}

impl Onboarding {
    pub fn new(mode: Mode, world: World) -> Self {
        let mut o = Self {
            mode,
            step: Step::order(mode)[0],
            choice: None,
            focus: 0,
            world,
            win: Size::new(1280.0, 800.0),
            outcomes: vec![],
            pending: None,
            moved: Instant::now(),
            dir: 0.0,
        };
        o.focus = o.default_focus();
        o
    }

    pub fn capturing(&self) -> bool {
        true
    }

    pub fn set_win(&mut self, s: Size) {
        self.win = s;
    }

    pub fn set_world(&mut self, w: World) {
        self.world = w;
        self.focus = self.focus.min(self.items().len() - 1);
    }

    // ---- steps ----

    fn pos(&self) -> usize {
        Step::order(self.mode)
            .iter()
            .position(|s| *s == self.step)
            .unwrap_or(0)
    }

    pub fn total(&self) -> usize {
        Step::order(self.mode).len()
    }

    pub fn index(&self) -> usize {
        self.pos()
    }

    pub fn can_back(&self) -> bool {
        self.pos() > 0
    }

    /// The last step has no Skip: its button already ends the walkthrough.
    pub fn can_skip(&self) -> bool {
        self.pos() + 1 < self.total()
    }

    /// Does this build/world have a server in play? Local-only setups never see the write switches.
    pub fn server_in_play(&self) -> bool {
        self.mode == Mode::Upgrade
            || self.choice != Some(FirstRun::Local)
            || !self.world.servers.is_empty()
    }

    fn go(&mut self, to: Step, dir: f32) {
        self.step = to;
        self.dir = dir;
        self.moved = Instant::now();
        self.focus = self.default_focus();
    }

    fn next(&mut self) {
        match Step::order(self.mode).get(self.pos() + 1) {
            Some(s) => self.go(*s, 1.0),
            None => self.finish(false, false),
        }
    }

    fn back(&mut self) {
        if let Some(p) = self.pos().checked_sub(1) {
            self.go(Step::order(self.mode)[p], -1.0);
        }
    }

    fn finish(&mut self, skipped: bool, add_folders: bool) {
        self.outcomes.push(Outcome::Finish {
            skipped,
            add_folders,
        });
    }

    // ---- items ----

    pub fn items(&self) -> Vec<Item> {
        match self.step {
            Step::Welcome | Step::Ready | Step::News1 => vec![Item::Next],
            Step::Where => vec![
                Item::Choose(FirstRun::Local),
                Item::Choose(FirstRun::Server),
                Item::Choose(FirstRun::Both),
            ],
            Step::SetUp => {
                let mut v = match self.choice {
                    Some(FirstRun::Local) => vec![Item::AddFolders],
                    Some(FirstRun::Server) => vec![Item::AddServer],
                    // Both: the server first (its sign-in is the longer part), as `FirstRun::first` says.
                    _ => vec![Item::AddServer, Item::AddFolders],
                };
                v.push(Item::Next);
                v
            }
            Step::Behave if self.server_in_play() => vec![Item::Writes, Item::Sync, Item::Next],
            Step::Behave => vec![Item::Next],
            Step::News2 => vec![Item::NewsFolders, Item::Next],
        }
    }

    fn done(&self, i: Item) -> bool {
        match i {
            Item::AddFolders => !self.world.folders.is_empty(),
            Item::AddServer => !self.world.servers.is_empty(),
            _ => false,
        }
    }

    /// Where the focus starts: the card the person most likely wants (a server, as before), the
    /// first row still to do, else the way forward.
    fn default_focus(&self) -> usize {
        let it = self.items();
        match self.step {
            Step::Where => 1,
            Step::SetUp => it
                .iter()
                .position(|i| !self.done(*i))
                .unwrap_or(0)
                .min(it.len() - 1),
            Step::Behave => it.len() - 1,
            Step::News2 => 1,
            _ => 0,
        }
    }

    pub fn focused(&self) -> Item {
        let it = self.items();
        it[self.focus.min(it.len() - 1)]
    }

    fn activate(&mut self, i: Item) {
        match i {
            Item::Next => self.next(),
            Item::Choose(c) => {
                self.choice = Some(c);
                self.go(Step::SetUp, 1.0);
            }
            Item::AddFolders => self.ask(Return::Folders, Outcome::AddFolders),
            Item::AddServer => self.ask(Return::Server, Outcome::AddServer),
            Item::Writes => {
                self.world.writes = !self.world.writes;
                self.outcomes.push(Outcome::SetWrites(self.world.writes));
            }
            Item::Sync if self.world.writes => {
                self.world.sync = !self.world.sync;
                self.outcomes.push(Outcome::SetSync(self.world.sync));
            }
            Item::Sync => {}
            Item::NewsFolders => self.finish(false, true),
        }
    }

    fn ask(&mut self, r: Return, o: Outcome) {
        self.pending = Some(r);
        self.outcomes.push(o);
    }

    /// The root is back from the folders / server screen.
    pub fn returned(&mut self, r: Return) {
        if self.pending != Some(r) {
            return;
        }
        self.pending = None;
        self.focus = self.default_focus();
    }

    // ---- input ----

    pub fn update(&mut self, m: Msg) {
        match m {
            Msg::Hover(i) => self.focus = i.min(self.items().len() - 1),
            Msg::Press(i) => {
                let it = self.items();
                if let Some(item) = it.get(i).copied() {
                    self.focus = i;
                    self.activate(item);
                }
            }
            Msg::Back => self.back(),
            Msg::Skip => {
                if self.can_skip() {
                    self.finish(true, false)
                }
            }
        }
    }

    /// Owns every key while up. `true` when something may have changed (drain the outcomes).
    pub fn key(&mut self, k: &Key) -> bool {
        let n = self.items().len();
        match k.name.as_str() {
            "left" | "up" => self.focus = (self.focus + n - 1) % n,
            "right" | "down" => self.focus = (self.focus + 1) % n,
            "tab" => {
                self.focus = if k.shift {
                    (self.focus + n - 1) % n
                } else {
                    (self.focus + 1) % n
                }
            }
            "home" => self.focus = 0,
            "end" => self.focus = n - 1,
            "enter" | "space" => {
                let i = self.focused();
                self.activate(i);
            }
            "escape" | "backspace" => self.back(),
            "s" if k.plain() => self.update(Msg::Skip),
            _ => return false,
        }
        true
    }

    // ---- motion ----

    /// 0 at the moment the step changed, 1 once it has settled (always 1 with reduced motion).
    pub(crate) fn settle(&self, now: Instant) -> f32 {
        let d = motion::dur(t::SLOW);
        if d.is_zero() || self.dir == 0.0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.moved).as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Needs frames: a step is sliding in, or the indexing dot is pulsing.
    pub fn animating(&self, now: Instant) -> bool {
        !motion::reduced()
            && (self.settle(now) < 1.0
                || (self.step == Step::SetUp && self.world.indexing.is_some()))
    }
}

/// 12345 -> "12,345".
pub fn count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Onboarding {
        Onboarding::new(Mode::Fresh, World::default())
    }
    fn key(n: &str) -> Key {
        Key {
            name: n.into(),
            ..Default::default()
        }
    }
    fn take(o: &mut Onboarding) -> Vec<Outcome> {
        std::mem::take(&mut o.outcomes)
    }
    fn to_setup(o: &mut Onboarding, c: FirstRun) {
        o.update(Msg::Press(0)); // get started
        assert_eq!(o.step, Step::Where);
        let i = o
            .items()
            .iter()
            .position(|i| *i == Item::Choose(c))
            .unwrap();
        o.update(Msg::Press(i));
        assert_eq!(o.step, Step::SetUp);
    }

    #[test]
    fn visibility_rules() {
        // Nothing set up: always, even after a finished walkthrough (all sources removed).
        assert_eq!(due(0, false, false), Some(Mode::Fresh));
        assert_eq!(due(VERSION, false, false), Some(Mode::Fresh));
        // Done or skipped once: never again while a source exists.
        assert_eq!(due(VERSION, true, false), None);
        assert_eq!(due(VERSION, true, true), None);
        // 0.2 upgrader, and someone who quit half-way through.
        assert_eq!(due(0, true, true), Some(Mode::Upgrade));
        assert_eq!(due(0, true, false), Some(Mode::Fresh));
    }

    #[test]
    fn welcome_to_ready_in_order_then_finish() {
        let mut o = fresh();
        assert_eq!((o.step, o.total(), o.can_back()), (Step::Welcome, 5, false));
        o.update(Msg::Press(0));
        assert_eq!(o.step, Step::Where);
        assert_eq!(
            o.focused(),
            Item::Choose(FirstRun::Server),
            "starts on the server card"
        );
        o.update(Msg::Press(0));
        assert_eq!((o.step, o.choice), (Step::SetUp, Some(FirstRun::Local)));
        o.update(Msg::Press(1)); // Continue
        assert_eq!(o.step, Step::Behave);
        // Local only, nothing connected: no write switches.
        assert_eq!(o.items(), vec![Item::Next]);
        o.update(Msg::Press(0));
        assert_eq!(o.step, Step::Ready);
        assert!(!o.can_skip(), "the last step has no Skip");
        o.update(Msg::Press(0));
        assert_eq!(
            take(&mut o),
            vec![Outcome::Finish {
                skipped: false,
                add_folders: false
            }]
        );
    }

    #[test]
    fn each_choice_routes_to_its_rows() {
        for (c, rows) in [
            (FirstRun::Local, vec![Item::AddFolders, Item::Next]),
            (FirstRun::Server, vec![Item::AddServer, Item::Next]),
            (
                FirstRun::Both,
                vec![Item::AddServer, Item::AddFolders, Item::Next],
            ),
        ] {
            let mut o = fresh();
            to_setup(&mut o, c);
            assert_eq!(o.items(), rows, "{c:?}");
            assert_eq!(o.focused(), rows[0], "focus on the first thing to do");
        }
    }

    #[test]
    fn adding_asks_the_root_and_resumes_on_return() {
        let mut o = fresh();
        to_setup(&mut o, FirstRun::Both);
        o.update(Msg::Press(0));
        assert_eq!(take(&mut o), vec![Outcome::AddServer]);
        // Back from the server screen with one server connected: focus moves on to the folders row.
        let mut w = o.world.clone();
        w.servers.push(Server {
            name: "Home".into(),
            user: "ana".into(),
        });
        o.set_world(w);
        o.returned(Return::Server);
        assert_eq!(o.focused(), Item::AddFolders);
        o.update(Msg::Press(1));
        assert_eq!(take(&mut o), vec![Outcome::AddFolders]);
        let mut w = o.world.clone();
        w.folders.push("Films".into());
        o.set_world(w);
        o.returned(Return::Folders);
        assert_eq!(o.focused(), Item::Next, "everything done: Continue");
        assert_eq!(o.step, Step::SetUp, "resumes on the same step");
    }

    #[test]
    fn a_stale_or_wrong_return_is_ignored() {
        let mut o = fresh();
        to_setup(&mut o, FirstRun::Both);
        o.focus = 2;
        o.returned(Return::Folders); // nothing pending
        assert_eq!(o.focus, 2);
        o.update(Msg::Press(0)); // AddServer pending
        o.focus = 2;
        o.returned(Return::Folders); // wrong one
        assert_eq!(o.focus, 2);
        o.returned(Return::Server);
        o.focus = 2;
        o.returned(Return::Server); // twice
        assert_eq!(o.focus, 2);
    }

    #[test]
    fn more_than_one_folder_or_server_stays_possible() {
        let mut o = fresh();
        to_setup(&mut o, FirstRun::Local);
        let mut w = o.world.clone();
        w.folders = vec!["Films".into()];
        o.set_world(w);
        // The add row remains an item after something was added.
        assert_eq!(o.items(), vec![Item::AddFolders, Item::Next]);
        o.update(Msg::Press(0));
        assert_eq!(take(&mut o), vec![Outcome::AddFolders]);
    }

    #[test]
    fn back_walks_the_steps_and_keeps_the_choice() {
        let mut o = fresh();
        to_setup(&mut o, FirstRun::Local);
        o.update(Msg::Back);
        assert_eq!((o.step, o.choice), (Step::Where, Some(FirstRun::Local)));
        o.update(Msg::Back);
        assert_eq!(o.step, Step::Welcome);
        o.update(Msg::Back);
        assert_eq!(o.step, Step::Welcome, "no step before the first");
        assert!(take(&mut o).is_empty());
    }

    #[test]
    fn skip_ends_it_from_any_step_but_the_last() {
        for hops in 0..4 {
            let mut o = fresh();
            for _ in 0..hops {
                o.next();
            }
            o.update(Msg::Skip);
            assert_eq!(
                take(&mut o),
                vec![Outcome::Finish {
                    skipped: true,
                    add_folders: false
                }],
                "step {:?}",
                o.step
            );
        }
        let mut o = fresh();
        for _ in 0..4 {
            o.next();
        }
        assert_eq!(o.step, Step::Ready);
        o.update(Msg::Skip);
        assert!(take(&mut o).is_empty());
    }

    #[test]
    fn write_switches_are_off_by_default_and_gate_each_other() {
        let mut o = fresh();
        to_setup(&mut o, FirstRun::Server);
        o.update(Msg::Press(1)); // Continue
        assert_eq!(o.step, Step::Behave);
        assert_eq!(o.items(), vec![Item::Writes, Item::Sync, Item::Next]);
        assert!(!o.world.writes && !o.world.sync);
        o.update(Msg::Press(1)); // sync while writes are off: nothing
        assert!(take(&mut o).is_empty() && !o.world.sync);
        o.update(Msg::Press(0));
        o.update(Msg::Press(1));
        assert_eq!(
            take(&mut o),
            vec![Outcome::SetWrites(true), Outcome::SetSync(true)]
        );
        o.update(Msg::Press(0));
        assert_eq!(take(&mut o), vec![Outcome::SetWrites(false)]);
    }

    #[test]
    fn keyboard_covers_everything() {
        let mut o = fresh();
        assert!(o.key(&key("enter")));
        assert_eq!(o.step, Step::Where);
        // Arrow keys wrap over the three cards, Enter picks (the old welcome behaviour).
        assert!(o.key(&key("right")));
        assert_eq!(o.focused(), Item::Choose(FirstRun::Both));
        o.key(&key("right"));
        assert_eq!(o.focused(), Item::Choose(FirstRun::Local));
        o.key(&key("left"));
        assert_eq!(o.focused(), Item::Choose(FirstRun::Both));
        o.key(&key("home"));
        o.key(&key("space"));
        assert_eq!((o.step, o.choice), (Step::SetUp, Some(FirstRun::Local)));
        o.key(&key("escape"));
        assert_eq!(o.step, Step::Where);
        assert!(!o.key(&key("f9")), "unknown keys are not claimed");
        o.key(&key("s"));
        assert_eq!(
            take(&mut o),
            vec![Outcome::Finish {
                skipped: true,
                add_folders: false
            }]
        );
        let shift_tab = Key {
            name: "tab".into(),
            shift: true,
            ..Default::default()
        };
        let f = o.focus;
        o.key(&shift_tab);
        assert_eq!(o.focus, (f + 2) % 3);
    }

    #[test]
    fn upgrader_gets_two_steps() {
        let mut o = Onboarding::new(Mode::Upgrade, World::default());
        assert_eq!((o.step, o.total()), (Step::News1, 2));
        assert!(o.can_skip());
        o.update(Msg::Press(0));
        assert_eq!(o.step, Step::News2);
        assert_eq!(o.items(), vec![Item::NewsFolders, Item::Next]);
        assert_eq!(o.focused(), Item::Next, "Continue is the default");
        o.update(Msg::Press(1));
        assert_eq!(
            take(&mut o),
            vec![Outcome::Finish {
                skipped: false,
                add_folders: false
            }]
        );
        o.update(Msg::Press(0));
        assert_eq!(
            take(&mut o),
            vec![Outcome::Finish {
                skipped: false,
                add_folders: true
            }]
        );
    }

    #[test]
    fn upgrader_with_a_server_always_sees_server_copy_only_in_fresh_flow() {
        let o = Onboarding::new(Mode::Upgrade, World::default());
        assert!(o.server_in_play());
        let mut o = fresh();
        to_setup(&mut o, FirstRun::Local);
        assert!(!o.server_in_play());
        let mut w = o.world.clone();
        w.servers.push(Server {
            name: "S".into(),
            user: "u".into(),
        });
        o.set_world(w);
        assert!(
            o.server_in_play(),
            "a connected server brings the switches back"
        );
    }

    #[test]
    fn settle_is_instant_when_nothing_moved_and_eases_in_after() {
        let o = fresh();
        assert_eq!(
            o.settle(Instant::now()),
            1.0,
            "no slide on the first screen"
        );
        let mut o = fresh();
        o.next();
        let s = o.settle(o.moved);
        assert!(s < 1.0 || motion::reduced());
        assert_eq!(o.settle(o.moved + t::SLOW * 2), 1.0);
    }

    #[test]
    fn counts_get_separators() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1,000");
        assert_eq!(count(1234567), "1,234,567");
    }
}
