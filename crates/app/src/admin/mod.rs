//! Admin (step 4e): every section of the server administration UI, the confirmation dialog and the
//! single place admin writes are authorised. Port of the GPUI `admin/` module on the multi-source
//! model: the screen is opened for ONE chosen Jellyfin source, picked at the top among the sources
//! where the signed-in account is an administrator (default: the first). Reads go through the
//! shared tokio bridge; every response carries the server generation and page id it was requested
//! for and is dropped if the user moved on (`state::State::accept`).
//!
//! ## Root hook (for the master)
//!
//! ```ignore
//! // Header "Admin" entry: show it when ANY enabled Jellyfin source's session is_admin (cheap hint);
//! // open with one Target per such source, in priority order:
//! let targets = catalog Jellyfin slots (usable, signed in) -> admin::Target {
//!     origin, label: source label, cfg: nocturne_jellyfin::Config { base_url, token }, pics: Some(source.clone()) };
//! let (a, task) = admin::Admin::open(targets, self.settings.allow_server_writes && !self.dev);
//! self.admin = Some(a);                           // Page::Admin; task.map(Message::Admin)
//! Message::Admin(m)  => { let t = a.update(m); self.drain_admin(); t.map(Message::Admin) }
//! view               => a.view().map(Message::Admin)            // body; the header stays the root's
//! key (page current) => { let t = a.key(&key); self.drain_admin(); t.map(Message::Admin) }   // ALL keys while open
//! a.dialog_open()    => the confirmation dialog owns the keyboard (no global shortcuts)
//! settings change    => a.set_writes(allow_server_writes && !dev_token_mode)   // false = view only
//! tick               => if a.rejected() { sign-in again for that source }  (401 on the admin token)
//! // drain a.outcomes after every update/key:
//! Outcome::Close        -> leave the page (history back)
//! Outcome::OpenSettings -> open Settings (the view-only banner's link)
//! ```
//! `on_confirm` below is the ONLY place the `Confirmed` token is minted. It runs only when the
//! dialog state machine (`confirm::Dialog`) answers `Out::Confirm`: the confirm button focused,
//! Enter or a click, every input valid. `single_confirm_choke_point` scans this directory.
//!
//! Debug preview: `NOCTURNE_PREVIEW=admin` (see `preview.rs`).
mod pick;
#[cfg(debug_assertions)]
pub mod preview;
mod view;

use crate::keys::Key;
use crate::task::{Handoff, after, run};
use iced::widget::image::Handle;
use iced::widget::{operation, scrollable::AbsoluteOffset};
use iced::{Element, Task, clipboard};
use nocturne_core::admin::act::{self, Act};
use nocturne_core::admin::confirm::{Dialog, Out};
use nocturne_core::admin::model::{self, Btn};
use nocturne_core::admin::state::{self, POLL_SECS, Section, Shape, State};
use nocturne_core::nav::Dir;
use nocturne_core::source::{Source, SourceId, sniff};
use nocturne_core::sync::NEED_WRITES;
use nocturne_jellyfin_admin::{AdminClient, Config, Confirmed};
use pick::Probe;
use std::collections::HashMap;
use std::time::Duration;

/// A Jellyfin source the root offers to administer.
#[derive(Clone)]
pub struct Target {
    pub origin: SourceId,
    pub label: String,
    /// Server URL and the account's access token (never logged).
    pub cfg: Config,
    /// Reads profile pictures (GET only); without it avatars show initials.
    pub pics: Option<Source>,
}

pub enum Outcome {
    Close,
    OpenSettings,
}

/// Where a pressed button lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Toolbar,
    Row(usize),
}

#[derive(Clone)]
pub enum Msg {
    /// Admin probe of target `i` answered.
    Probed(u64, usize, Option<AdminClient>),
    /// A read for server generation `.0`, page `.1`.
    Data(u64, u64, Handoff<state::Msg>),
    Poll(u64, u64),
    Side(Section),
    Row(usize, bool),
    Press(Scope, usize),
    /// Dialog element `ix` clicked (an input, Cancel or Confirm).
    DialogClick(usize),
    Paste(Option<String>),
    Wrote(u64, bool, &'static str),
    NoteGone(u64),
    Pic(String, String, Option<Handle>),
    Scrolled(f32, f32),
    Choose(usize),
    OpenSettings,
}

pub struct Admin {
    targets: Vec<Target>,
    probes: Vec<Probe>,
    /// The admin client of each target that probed as an administrator.
    clients: Vec<Option<AdminClient>>,
    chosen: Option<usize>,
    client: Option<AdminClient>,
    /// Bumped when the chosen server changes: answers for the old one are dropped.
    gen_: u64,
    /// Server changes are allowed (Settings master switch, real login). Off = view only.
    pub writes: bool,
    st: State,
    dialog: Option<Dialog>,
    /// A confirmed write is running.
    writing: bool,
    note: Option<(String, u64)>,
    note_n: u64,
    /// Profile pictures by (user id, tag); `None` = requested, not (or never) loaded.
    pics: HashMap<(String, String), Option<Handle>>,
    list_off: f32,
    list_h: f32,
    pub outcomes: Vec<Outcome>,
}

/// `Some(client)` only when the token has admin rights (see `state::admin_visible`).
async fn probe(cfg: Config) -> Option<AdminClient> {
    let c = AdminClient::new(cfg).ok()?;
    state::admin_visible(&c.system_info().await).then_some(c)
}

pub(crate) const LIST_ID: &str = "adm-list";
/// Fixed heights, so keyboard selection can be scrolled into view without measuring.
pub(crate) const ROW_H: f32 = 61.0;
pub(crate) const GROUP_H: f32 = 33.0;

fn shape(st: &State, m: &model::Model) -> Shape {
    Shape {
        items: m.items.len(),
        toolbar: m.toolbar.len(),
        acts: if st.on_toolbar {
            m.toolbar.len()
        } else {
            m.items.get(st.sel).map_or(0, |i| i.acts.len())
        },
    }
}

/// Top and bottom edge of item `i` inside the list.
fn row_span(m: &model::Model, i: usize) -> (f32, f32) {
    let groups = (0..=i.min(m.items.len().saturating_sub(1)))
        .filter(|&k| m.group_start(k))
        .count();
    let top = groups as f32 * GROUP_H + i as f32 * ROW_H;
    (
        top - if m.group_start(i) { GROUP_H } else { 0.0 },
        top + ROW_H,
    )
}

impl Admin {
    /// Probes every target; the first administrator (once earlier ones have answered) becomes the
    /// chosen server and its dashboard loads.
    pub fn open(targets: Vec<Target>, writes: bool) -> (Self, Task<Msg>) {
        let mut me = Self {
            probes: vec![Probe::Pending; targets.len()],
            clients: vec![None; targets.len()],
            targets,
            chosen: None,
            client: None,
            gen_: 0,
            writes,
            st: State::new(),
            dialog: None,
            writing: false,
            note: None,
            note_n: 0,
            pics: HashMap::new(),
            list_off: 0.0,
            list_h: 400.0,
            outcomes: vec![],
        };
        me.st.in_side = true;
        let g = me.gen_;
        let tasks: Vec<_> = me
            .targets
            .iter()
            .enumerate()
            .map(|(i, t)| run(probe(t.cfg.clone()), move |c| Msg::Probed(g, i, c)))
            .collect();
        (me, Task::batch(tasks))
    }

    /// Same, for a ready client (preview, tests).
    #[cfg(any(test, debug_assertions))]
    pub fn with_client(t: Target, client: AdminClient, writes: bool) -> (Self, Task<Msg>) {
        let (mut me, _) = Self::open(vec![t], writes);
        me.probes = vec![Probe::Admin];
        me.clients = vec![Some(client)];
        let task = me.choose(0);
        (me, task)
    }

    pub fn set_writes(&mut self, on: bool) {
        self.writes = on;
        if !on {
            self.dialog = None;
        }
    }

    /// The admin token got a 401: the root returns that source to sign-in.
    pub fn rejected(&self) -> bool {
        self.client.as_ref().is_some_and(AdminClient::rejected)
    }

    /// The origin of the server being administered.
    pub fn origin(&self) -> Option<SourceId> {
        self.chosen.map(|i| self.targets[i].origin)
    }

    /// A confirmation dialog is up and owns the keyboard.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    fn pick_default(&mut self) -> Task<Msg> {
        if self.chosen.is_some() {
            return Task::none();
        }
        match pick::default_choice(&self.probes) {
            Some(i) => self.choose(i),
            None => Task::none(),
        }
    }

    fn choose(&mut self, i: usize) -> Task<Msg> {
        self.chosen = Some(i);
        self.client = self.clients[i].clone();
        self.gen_ += 1;
        self.dialog = None;
        self.pics.clear();
        self.st = State::new();
        self.st.in_side = true;
        self.go(Section::Dashboard)
    }

    fn go(&mut self, s: Section) -> Task<Msg> {
        self.st.enter(s);
        self.list_off = 0.0;
        self.entered()
    }

    fn entered(&mut self) -> Task<Msg> {
        let load = self.load();
        if self.st.section == Section::Dashboard {
            Task::batch([load, self.poll_later()])
        } else {
            load
        }
    }

    fn poll_later(&self) -> Task<Msg> {
        after(
            Duration::from_secs(POLL_SECS),
            Msg::Poll(self.gen_, self.st.epoch),
        )
    }

    fn fetch(
        &self,
        fut: impl std::future::Future<Output = state::Msg> + Send + 'static,
    ) -> Task<Msg> {
        let (g, pid) = (self.gen_, self.st.pid);
        run(fut, move |m| Msg::Data(g, pid, Handoff::new(m)))
    }

    fn load(&mut self) -> Task<Msg> {
        use Section::*;
        use state::Msg as D;
        let Some(base) = self.client.clone() else {
            return Task::none();
        };
        macro_rules! rd {
            ($m:ident, |$c:ident| $e:expr) => {{
                let $c = base.clone();
                self.fetch(async move { D::$m($e.await.ok()) })
            }};
        }
        let t: Vec<Task<Msg>> = match self.st.section {
            Dashboard => vec![
                rd!(System, |c| c.system_info()),
                rd!(Activity, |c| c.activity_log(0, 10)),
                rd!(Sessions, |c| c.sessions()),
                rd!(Tasks, |c| c.scheduled_tasks()),
                rd!(Folders, |c| c.virtual_folders()),
                rd!(Counts, |c| c.item_counts()),
            ],
            General => vec![
                rd!(Config, |c| c.server_config()),
                rd!(System, |c| c.system_info()),
            ],
            Branding => vec![rd!(Branding, |c| c.branding())],
            Users => vec![rd!(Users, |c| c.users())],
            Access => vec![
                rd!(Users, |c| c.users()),
                rd!(Folders, |c| c.virtual_folders()),
                rd!(Ratings, |c| c.parental_ratings()),
            ],
            Libraries => vec![
                rd!(Folders, |c| c.virtual_folders()),
                rd!(Tasks, |c| c.scheduled_tasks()),
            ],
            Providers => vec![rd!(Providers, |c| state::library_providers(&c))],
            Metadata | Resume | Streaming | Trickplay => {
                vec![rd!(Config, |c| c.server_config())]
            }
            Transcoding => vec![rd!(Encoding, |c| c.encoding_config())],
            Backups => vec![rd!(Backups, |c| c.backups())],
            Devices => vec![rd!(Devices, |c| c.devices())],
            Activity => vec![rd!(Activity, |c| c.activity_log(0, 100))],
            Plugins => vec![
                rd!(Plugins, |c| c.plugins()),
                rd!(Packages, |c| c.packages()),
                rd!(Repos, |c| c.plugin_repositories()),
            ],
            Networking => vec![rd!(Network, |c| c.network_config())],
            ApiKeys => vec![rd!(Keys, |c| c.api_keys())],
            Logs => vec![rd!(Logs, |c| c.log_files())],
            Tasks => vec![rd!(Tasks, |c| c.scheduled_tasks())],
        };
        Task::batch(t)
    }

    /// Start loading the profile pictures of the listed users (once per user and tag).
    fn load_pics(&mut self) -> Task<Msg> {
        let Some(src) = self.chosen.and_then(|i| self.targets[i].pics.clone()) else {
            return Task::none();
        };
        let m = model::model(&self.st);
        let mut tasks = vec![];
        for key in m.items.iter().filter_map(|i| i.pic.clone()) {
            if self.pics.contains_key(&key) {
                continue;
            }
            self.pics.insert(key.clone(), None);
            let (s, k) = (src.clone(), key.clone());
            tasks.push(run(
                async move {
                    let b = s.user_image(&k.0, &k.1, 96).await.ok()?;
                    sniff(&b).map(|_| Handle::from_bytes(b))
                },
                move |h| Msg::Pic(key.0.clone(), key.1.clone(), h),
            ));
        }
        Task::batch(tasks)
    }

    fn toast(&mut self, msg: &str) -> Task<Msg> {
        self.note_n += 1;
        self.note = Some((msg.into(), self.note_n));
        after(Duration::from_millis(3000), Msg::NoteGone(self.note_n))
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let t = match msg {
            Msg::Probed(g, i, c) => {
                if g != self.gen_ || i >= self.probes.len() {
                    return Task::none();
                }
                self.probes[i] = if c.is_some() { Probe::Admin } else { Probe::No };
                self.clients[i] = c;
                self.pick_default()
            }
            Msg::Data(g, pid, m) => {
                if g != self.gen_ {
                    return Task::none();
                }
                match m.take() {
                    Some(m) => {
                        if self.st.accept(pid, m) {
                            self.load_pics()
                        } else {
                            Task::none()
                        }
                    }
                    None => Task::none(),
                }
            }
            Msg::Poll(g, epoch) => {
                if g == self.gen_ && self.st.polling(epoch) {
                    Task::batch([self.load(), self.poll_later()])
                } else {
                    Task::none()
                }
            }
            Msg::Side(s) => {
                self.st.in_side = true;
                if self.st.section != s {
                    self.go(s)
                } else {
                    Task::none()
                }
            }
            Msg::Row(i, card) => {
                let _ = card;
                (
                    self.st.in_side,
                    self.st.on_toolbar,
                    self.st.sel,
                    self.st.act,
                ) = (false, false, i, 0);
                Task::none()
            }
            Msg::Press(scope, i) => self.press(scope, i),
            Msg::DialogClick(ix) => self.dialog_click(ix),
            Msg::Paste(Some(text)) => self.paste(&text),
            Msg::Paste(None) => Task::none(),
            Msg::Wrote(g, ok, text) => {
                self.writing = false;
                let mut t = vec![self.toast(text)];
                if ok && g == self.gen_ {
                    self.st.act = 0;
                    t.push(self.load()); // refresh the affected list
                }
                Task::batch(t)
            }
            Msg::NoteGone(n) => {
                if self.note.as_ref().is_some_and(|(_, k)| *k == n) {
                    self.note = None;
                }
                Task::none()
            }
            Msg::Pic(u, tag, h) => {
                if let Some(h) = h {
                    self.pics.insert((u, tag), Some(h));
                }
                Task::none()
            }
            Msg::Scrolled(off, h) => {
                (self.list_off, self.list_h) = (off, h);
                Task::none()
            }
            Msg::Choose(i) => {
                if self.chosen == Some(i) || self.probes.get(i) != Some(&Probe::Admin) {
                    return Task::none();
                }
                self.choose(i)
            }
            Msg::OpenSettings => {
                self.outcomes.push(Outcome::OpenSettings);
                Task::none()
            }
        };
        let n = model::model(&self.st).items.len();
        self.st.clamp(n);
        t
    }

    fn press(&mut self, scope: Scope, i: usize) -> Task<Msg> {
        let m = model::model(&self.st);
        let acts: &[Btn] = match scope {
            Scope::Toolbar => {
                (self.st.in_side, self.st.on_toolbar, self.st.act) = (false, true, i);
                &m.toolbar
            }
            Scope::Row(r) => {
                (
                    self.st.in_side,
                    self.st.on_toolbar,
                    self.st.sel,
                    self.st.act,
                ) = (false, false, r, i);
                m.items.get(r).map_or(&[], |it| &it.acts)
            }
        };
        match acts.get(i) {
            Some(b) => self.do_act(b.act.clone()),
            None => Task::none(),
        }
    }

    /// Keyboard entry point (every key while the admin page is current).
    pub fn key(&mut self, k: &Key) -> Task<Msg> {
        if let Some(d) = &mut self.dialog {
            if k.is_paste() {
                return clipboard::read().map(Msg::Paste);
            }
            let out = d.key(&k.name, k.typed());
            return self.dialog_out(out);
        }
        let name = k.name.as_str();
        if name == "tab" && self.probes.len() > 1 {
            let to = pick::cycle(&self.probes, self.chosen.unwrap_or(0), !k.shift);
            return self.update(Msg::Choose(to));
        }
        if self.client.is_none() {
            if matches!(name, "escape" | "backspace") {
                self.outcomes.push(Outcome::Close);
            }
            return Task::none();
        }
        let dir = match name {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let m = model::model(&self.st);
        let mut task = Task::none();
        match (name, dir) {
            ("backspace", _) if !self.st.in_side => {
                (self.st.in_side, self.st.on_toolbar) = (true, false);
            }
            ("escape" | "backspace", _) => {
                self.close();
                self.outcomes.push(Outcome::Close);
                return Task::none();
            }
            ("r", _) => task = self.load(),
            ("enter", _) if self.st.in_side => self.st.nav(Dir::Right, shape(&self.st, &m)),
            ("enter", _) => {
                let acts: &[Btn] = if self.st.on_toolbar {
                    &m.toolbar
                } else {
                    m.items.get(self.st.sel).map_or(&[], |i| &i.acts)
                };
                if let Some(b) = acts.get(self.st.act) {
                    task = self.do_act(b.act.clone());
                }
            }
            (_, Some(d)) => {
                let before = self.st.section;
                self.st.nav(d, shape(&self.st, &m));
                if self.st.section != before {
                    task = self.entered();
                }
                if !self.st.on_toolbar && !self.st.in_side && !m.items.is_empty() {
                    task = Task::batch([task, self.scroll_to(&m)]);
                }
            }
            _ => {}
        }
        let n = model::model(&self.st).items.len();
        self.st.clamp(n);
        task
    }

    /// Keep the selected row inside the list's viewport.
    fn scroll_to(&self, m: &model::Model) -> Task<Msg> {
        let (top, bottom) = row_span(m, self.st.sel);
        let y = if top < self.list_off {
            top
        } else if bottom > self.list_off + self.list_h {
            bottom - self.list_h
        } else {
            return Task::none();
        };
        operation::scroll_to(
            LIST_ID,
            AbsoluteOffset {
                x: 0.0,
                y: y.max(0.0),
            },
        )
    }

    pub fn close(&mut self) {
        self.st.leave();
        self.dialog = None;
    }

    fn do_act(&mut self, a: Act) -> Task<Msg> {
        let Some(c) = self.client.clone() else {
            return Task::none();
        };
        match a {
            // Reads: always allowed.
            Act::ViewPrefs((id, _)) => self.fetch(async move {
                let t = c.display_preferences(&id, "emby").await.ok();
                state::Msg::Prefs(id, t.map(|v| state::pretty_prefs(&v)))
            }),
            Act::ViewLog(name) => self.fetch(async move {
                let t = c.log_tail(&name, 200).await.ok();
                state::Msg::Log(name, t)
            }),
            a if self.writes => {
                self.dialog = Some(a.dialog());
                Task::none()
            }
            _ => self.toast(NEED_WRITES),
        }
    }

    fn dialog_out(&mut self, out: Out) -> Task<Msg> {
        match out {
            Out::Stay => Task::none(),
            Out::Cancel => {
                self.dialog = None;
                Task::none()
            }
            Out::Confirm => self.on_confirm(),
        }
    }

    fn paste(&mut self, text: &str) -> Task<Msg> {
        if let Some(d) = &mut self.dialog {
            // Through the same per-character path as typing: numeric and control rules apply.
            for c in text.chars().filter(|c| !c.is_control()) {
                let s = c.to_string();
                d.key(&s, Some(&s));
            }
        }
        Task::none()
    }

    /// THE single choke point for admin writes: the only place the `Confirmed` token is minted.
    /// Reached only through `Dialog::activate` returning `Out::Confirm` (confirm button focused,
    /// Enter or click, inputs valid). A test asserts no other call site exists.
    fn on_confirm(&mut self) -> Task<Msg> {
        let Some(d) = self.dialog.take().filter(Dialog::valid) else {
            return Task::none();
        };
        let Some(c) = self.client.clone().filter(|_| self.writes) else {
            return Task::none();
        };
        let ok = Confirmed::i_have_asked_the_user();
        let (act, vals, g) = (d.act.clone(), d.values(), self.gen_);
        self.writing = true;
        run(act::exec(c, act.clone(), vals, ok), move |r| {
            let done = r.is_ok();
            Msg::Wrote(g, done, act.done(done))
        })
    }

    /// Mouse click on dialog element `ix` (an input, Cancel or Confirm): same path as Enter.
    fn dialog_click(&mut self, ix: usize) -> Task<Msg> {
        let Some(d) = &mut self.dialog else {
            return Task::none();
        };
        d.focus = ix;
        let out = if ix >= d.cancel_ix() {
            d.activate()
        } else {
            Out::Stay
        };
        self.dialog_out(out)
    }

    pub fn view(&self) -> Element<'_, Msg> {
        view::view(self)
    }
}

#[cfg(test)]
mod tests;
