//! Admin section of the app: a GPUI entity that owns all admin state, so `app.rs` only
//! forwards keys and swaps the body. Reads go through the shared tokio bridge; every response
//! carries the page id it was requested for and is dropped if the user has moved on.
mod act;
mod confirm;
pub(crate) mod fmt;
mod model;
mod state;
mod view;

use crate::bridge;
use crate::nav::Dir;
use act::Act;
use confirm::{Dialog, Out};
use gpui::*;
use nocturne_jellyfin_admin::{AdminClient, Config, Confirmed};
use state::{Msg, POLL_SECS, Section, Shape, State};
use std::future::Future;
use std::time::Duration;

pub(crate) use fmt::when;

/// Asks the app to open Settings (the view-only banner's link).
pub struct OpenSettings;

impl EventEmitter<OpenSettings> for Admin {}

pub struct Admin {
    client: AdminClient,
    /// Server changes are allowed (Settings master switch, real login). Off = view only.
    pub writes: bool,
    st: State,
    dialog: Option<Dialog>,
    note: Option<String>,
    note_gen: u64,
    list_scroll: ScrollHandle,
    log_scroll: ScrollHandle,
}

/// `Some(client)` only when the configured token has admin rights (see `state::admin_visible`).
pub async fn probe(cfg: Option<Config>) -> Option<AdminClient> {
    let c = AdminClient::new(cfg?).ok()?;
    state::admin_visible(&c.system_info().await).then_some(c)
}

macro_rules! rd {
    ($s:ident, $cx:ident, $m:ident, |$c:ident| $e:expr) => {{
        let $c = $s.client.clone();
        $s.fetch($cx, async move { Msg::$m($e.await.ok()) });
    }};
}

impl Admin {
    pub fn new(client: AdminClient) -> Self {
        Self {
            client,
            writes: false,
            st: State::new(),
            dialog: None,
            note: None,
            note_gen: 0,
            list_scroll: ScrollHandle::new(),
            log_scroll: ScrollHandle::new(),
        }
    }

    /// The admin token got a 401: the app must return to sign-in.
    pub fn rejected(&self) -> bool {
        self.client.rejected()
    }

    /// A confirmation dialog is up and owns the keyboard.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.st.in_side = true;
        self.go(Section::Dashboard, cx);
    }

    pub fn close(&mut self) {
        self.st.leave();
        self.dialog = None;
    }

    fn go(&mut self, s: Section, cx: &mut Context<Self>) {
        self.st.enter(s);
        self.entered(cx);
    }

    fn entered(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
        if self.st.section == Section::Dashboard {
            let epoch = self.st.epoch;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_secs(POLL_SECS))
                        .await;
                    let alive = this.update(cx, |a, cx| {
                        let on = a.st.polling(epoch);
                        if on {
                            a.load(cx);
                        }
                        on
                    });
                    if !matches!(alive, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    fn fetch(&self, cx: &mut Context<Self>, fut: impl Future<Output = Msg> + Send + 'static) {
        let (pid, h) = (self.st.pid, bridge::spawn(fut));
        cx.spawn(async move |this, cx| {
            if let Ok(m) = h.await {
                this.update(cx, |a, cx| {
                    if a.st.accept(pid, m) {
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        use Section::*;
        match self.st.section {
            Dashboard => {
                rd!(self, cx, System, |c| c.system_info());
                rd!(self, cx, Activity, |c| c.activity_log(0, 10));
                rd!(self, cx, Sessions, |c| c.sessions());
                rd!(self, cx, Tasks, |c| c.scheduled_tasks());
                rd!(self, cx, Folders, |c| c.virtual_folders());
                rd!(self, cx, Counts, |c| c.item_counts());
            }
            General => {
                rd!(self, cx, Config, |c| c.server_config());
                rd!(self, cx, System, |c| c.system_info());
            }
            Branding => rd!(self, cx, Branding, |c| c.branding()),
            Users => rd!(self, cx, Users, |c| c.users()),
            Libraries => rd!(self, cx, Folders, |c| c.virtual_folders()),
            Metadata | Resume | Streaming | Trickplay => {
                rd!(self, cx, Config, |c| c.server_config())
            }
            Transcoding => rd!(self, cx, Encoding, |c| c.encoding_config()),
            Backups => rd!(self, cx, Backups, |c| c.backups()),
            Devices => rd!(self, cx, Devices, |c| c.devices()),
            Activity => rd!(self, cx, Activity, |c| c.activity_log(0, 100)),
            Plugins => {
                rd!(self, cx, Plugins, |c| c.plugins());
                rd!(self, cx, Packages, |c| c.packages());
                rd!(self, cx, Repos, |c| c.plugin_repositories());
            }
            Networking => rd!(self, cx, Network, |c| c.network_config()),
            ApiKeys => rd!(self, cx, Keys, |c| c.api_keys()),
            Logs => rd!(self, cx, Logs, |c| c.log_files()),
            Tasks => rd!(self, cx, Tasks, |c| c.scheduled_tasks()),
        }
    }

    fn toast(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.note = Some(msg.into());
        self.note_gen += 1;
        let g = self.note_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(3000))
                .await;
            this.update(cx, |a, cx| {
                if a.note_gen == g {
                    a.note = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Keyboard entry point. Returns true when the user left the admin section.
    pub fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let key = ev.keystroke.key.as_str();
        if let Some(d) = &mut self.dialog {
            let ch =
                ev.keystroke.key_char.as_deref().filter(|_| {
                    !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.platform
                });
            let out = d.key(key, ch);
            self.dialog_out(out, cx);
            cx.notify();
            return false;
        }
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        let m = self.model();
        match (key, dir) {
            ("backspace", _) if !self.st.in_side => {
                (self.st.in_side, self.st.on_toolbar) = (true, false);
            }
            ("escape" | "backspace", _) => {
                self.close();
                return true;
            }
            ("r", _) => self.load(cx),
            ("enter", _) if self.st.in_side => self.st.nav(Dir::Right, shape(&self.st, &m)),
            ("enter", _) => {
                let acts: &[model::Btn] = if self.st.on_toolbar {
                    &m.toolbar
                } else {
                    m.items.get(self.st.sel).map_or(&[], |i| &i.acts)
                };
                if let Some(b) = acts.get(self.st.act) {
                    self.do_act(b.act.clone(), cx);
                }
            }
            (_, Some(d)) => {
                let before = self.st.section;
                self.st.nav(d, shape(&self.st, &m));
                if self.st.section != before {
                    self.entered(cx);
                }
                if !self.st.on_toolbar && !self.st.in_side && !m.items.is_empty() {
                    self.list_scroll.scroll_to_item(m.child_ix(self.st.sel));
                }
            }
            _ => {}
        }
        cx.notify();
        false
    }

    fn do_act(&mut self, a: Act, cx: &mut Context<Self>) {
        if let Act::ViewLog(name) = a {
            let c = self.client.clone();
            self.fetch(cx, async move {
                let t = c.log_tail(&name, 200).await.ok();
                Msg::Log(name, t)
            });
        } else if self.writes {
            self.dialog = Some(a.dialog());
        } else {
            self.toast(crate::sync::NEED_WRITES, cx);
        }
    }

    fn dialog_out(&mut self, out: Out, cx: &mut Context<Self>) {
        match out {
            Out::Stay => {}
            Out::Cancel => self.dialog = None,
            Out::Confirm => self.on_confirm(cx),
        }
    }

    /// THE single choke point for admin writes: the only place the `Confirmed` token is minted.
    /// Reached only through `Dialog::activate` returning `Out::Confirm` (confirm button focused,
    /// Enter or click, inputs valid). A test asserts no other call site exists.
    fn on_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dialog.take().filter(Dialog::valid) else {
            return;
        };
        if !self.writes {
            return;
        }
        let ok = Confirmed::i_have_asked_the_user();
        let (c, act, vals) = (self.client.clone(), d.act.clone(), d.values());
        let h = bridge::spawn(act::exec(c, act.clone(), vals, ok));
        cx.spawn(async move |this, cx| {
            let res = h.await.map(|r| r.is_ok()).unwrap_or(false);
            this.update(cx, |a, cx| {
                a.toast(act.done(res), cx);
                if res {
                    a.st.act = 0;
                    a.load(cx); // refresh the affected list
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Mouse click on dialog element `ix` (an input, Cancel or Confirm): same path as Enter.
    fn dialog_click(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(d) = &mut self.dialog else { return };
        d.focus = ix;
        let out = if ix >= d.cancel_ix() {
            d.activate()
        } else {
            Out::Stay
        };
        self.dialog_out(out, cx);
        cx.notify();
    }
}

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

#[cfg(test)]
mod tests {
    /// The `Confirmed` token must be minted in exactly one place (`Admin::on_confirm`).
    #[test]
    fn single_confirm_choke_point() {
        let call = ["i_have_asked", "_the_user("].concat();
        let srcs = [
            include_str!("mod.rs"),
            include_str!("act.rs"),
            include_str!("confirm.rs"),
            include_str!("model.rs"),
            include_str!("state.rs"),
            include_str!("view.rs"),
        ];
        let hits: usize = srcs.iter().map(|s| s.matches(&call).count()).sum();
        // One real call, plus this test's own concat-free mention is avoided by `concat`.
        assert_eq!(hits, 1);
        assert!(include_str!("mod.rs").contains(&format!("Confirmed::{call})")));
    }
}
