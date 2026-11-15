//! "Who's watching?": the full-screen profile picker shown at launch when the server has several
//! remembered accounts. A small pure model (start-up decision, selection, default choice) plus the
//! view. Choosing a tile is a local sign-in with the account's stored token (no server writes, no
//! dependence on the write switch); the default marker is a name/id flag in `accounts.toml`.
use crate::{theme as t, ui};
use gpui::prelude::*;
use gpui::*;
use nocturne_session::{Account, Accounts};
use std::sync::Arc;

pub const TILE_W: f32 = 168.0;
pub const GAP: f32 = 32.0;
const PIC: f32 = 120.0;

/// What to do at launch.
#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    /// As before: restore the stored session, or show the connect screen (0 or 1 account,
    /// developer-token mode, no known server).
    Restore,
    /// Several accounts and no (usable) default: show the picker.
    Picker,
    /// Several accounts and a default: sign in as `accounts[i]` straight away.
    Direct(usize),
}

/// `list` is the remembered accounts of the last used server (first remembered first). A default
/// flag that is not in `list` (other server, removed) never counts; with several flags (hand-edited
/// file) the first one wins.
pub fn decide(dev: bool, list: &[Account]) -> Start {
    if dev || list.len() < 2 {
        return Start::Restore;
    }
    match list.iter().position(|a| a.default) {
        Some(i) => Start::Direct(i),
        None => Start::Picker,
    }
}

/// The accounts of `server`, for [`decide`] and [`Picker::new`].
pub fn accounts_of(all: &Accounts, server: &str) -> Vec<Account> {
    all.for_server(server).into_iter().cloned().collect()
}

/// What a key or click asks the screen to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Stay,
    /// Sign in as profile `i`.
    Pick(usize),
    /// The "Add profile" tile: the sign-in form.
    Add,
    /// The "Use as default profile" box.
    Toggle,
}

/// What choosing a profile does to the stored default.
#[derive(Debug, PartialEq, Eq)]
pub enum DefaultChange {
    Keep,
    Set,
    Clear,
}

pub struct Picker {
    pub server: String,
    pub accounts: Vec<Account>,
    /// Highlighted tile; `accounts.len()` is the "Add profile" tile.
    pub sel: usize,
    /// Tiles per row at the current window width (kept by the caller).
    pub cols: usize,
    /// A sign-in is running: input is ignored until it ends.
    pub busy: bool,
    default: Option<usize>,
    want_default: bool,
    /// The box was changed by the user, so it no longer follows the highlight.
    touched: bool,
}

/// Tiles that fit a row in a window `width` px wide.
pub fn columns(width: f32) -> usize {
    (((width - 2.0 * t::PAGE_PAD + GAP) / (TILE_W + GAP)).floor() as usize).max(1)
}

impl Picker {
    /// Opens on the default profile, else on `current` (the signed-in one), else the first.
    pub fn new(server: &str, accounts: Vec<Account>, current: Option<&str>, cols: usize) -> Self {
        let default = accounts.iter().position(|a| a.default);
        let sel = default
            .or_else(|| {
                accounts
                    .iter()
                    .position(|a| Some(a.user_id.as_str()) == current)
            })
            .unwrap_or(0);
        let mut p = Self {
            server: server.into(),
            accounts,
            sel,
            cols: cols.max(1),
            busy: false,
            default,
            want_default: false,
            touched: false,
        };
        p.follow();
        p
    }

    /// Profiles plus the "Add profile" tile.
    pub fn tiles(&self) -> usize {
        self.accounts.len() + 1
    }

    /// The box mirrors "the highlighted profile is the default" until the user touches it.
    fn follow(&mut self) {
        if !self.touched {
            self.want_default = self.default == Some(self.sel);
        }
    }

    pub fn checked(&self) -> bool {
        self.want_default
    }

    pub fn toggle(&mut self) {
        self.want_default = !self.want_default;
        self.touched = true;
    }

    /// Left / Right wrap around all tiles; Up / Down move a row (staying put at the edge, and a
    /// short last row is reached from above); Enter chooses; Space / D toggles the default box.
    /// Esc does nothing: there is nothing behind this screen.
    pub fn key(&mut self, key: &str) -> Act {
        if self.busy {
            return Act::Stay;
        }
        let (n, c) = (self.tiles(), self.cols);
        match key {
            "right" | "tab" => self.sel = (self.sel + 1) % n,
            "left" => self.sel = (self.sel + n - 1) % n,
            "down" if self.sel + c < n => self.sel += c,
            "down" if self.sel / c < (n - 1) / c => self.sel = n - 1,
            "up" if self.sel >= c => self.sel -= c,
            "home" => self.sel = 0,
            "end" => self.sel = n - 1,
            "space" | "d" => self.toggle(),
            "enter" => return self.act_on(self.sel),
            _ => {}
        }
        self.follow();
        Act::Stay
    }

    /// Highlight tile `i` and say what choosing it means.
    pub fn act_on(&mut self, i: usize) -> Act {
        if self.busy || i >= self.tiles() {
            return Act::Stay;
        }
        self.sel = i;
        self.follow();
        if i == self.accounts.len() {
            Act::Add
        } else {
            Act::Pick(i)
        }
    }

    /// The change to the stored default when profile `i` is chosen: only an explicit change of the
    /// box counts, so opening the picker and choosing never alters the default by itself.
    pub fn default_change(&self, i: usize) -> DefaultChange {
        match (self.touched, self.want_default, self.default == Some(i)) {
            (true, true, false) => DefaultChange::Set,
            (true, false, true) => DefaultChange::Clear,
            _ => DefaultChange::Keep,
        }
    }
}

/// The screen. `pic` finds a loaded profile picture; `on` receives every click.
pub fn view<T: 'static>(
    p: &Picker,
    pic: &dyn Fn(&Account) -> Option<Arc<Image>>,
    on: fn(&mut T, Act, &mut Context<T>),
    cx: &mut Context<T>,
) -> AnyElement {
    let n_acc = p.accounts.len();
    let tile = |i: usize, label: String, face: Div| {
        let on_it = i == p.sel;
        div()
            .id(("pick-tile", i))
            .w(px(TILE_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(t::S3))
            .cursor_pointer()
            .on_click(cx.listener(move |t, _, _, cx| {
                let a = if i < n_acc { Act::Pick(i) } else { Act::Add };
                on(t, a, cx)
            }))
            .child(
                face.border_2()
                    .border_color(if on_it { t::accent() } else { t::edge() })
                    .when(on_it, |d| {
                        d.shadow(vec![BoxShadow {
                            color: t::glow().into(),
                            offset: point(px(0.0), px(0.0)),
                            blur_radius: px(24.0),
                            spread_radius: px(0.0),
                        }])
                    })
                    .hover(|s| {
                        s.border_color(if on_it {
                            t::accent()
                        } else {
                            t::text_tertiary()
                        })
                    })
                    .font_family(t::MONO)
                    .text_size(px(44.0))
                    .text_color(t::accent_text()),
            )
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(t::BODY_LG))
                    .font_weight(if on_it {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if on_it {
                        t::text()
                    } else {
                        t::text_secondary()
                    })
                    .child(label),
            )
    };
    let mut row = div()
        .w(px(p.cols.min(p.tiles()) as f32 * (TILE_W + GAP) - GAP))
        .flex()
        .flex_wrap()
        .justify_center()
        .gap(px(GAP));
    for (i, a) in p.accounts.iter().enumerate() {
        row = row.child(tile(
            i,
            a.user_name.clone(),
            ui::avatar(PIC, &a.user_name, pic(a)),
        ));
    }
    let add = div()
        .size(px(PIC))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(t::glass())
        .text_color(t::text_secondary())
        .child("+");
    row = row.child(tile(p.accounts.len(), "Add profile".into(), add));
    let adding = p.sel == p.accounts.len();
    let check = div()
        .id("pick-default")
        .flex()
        .items_center()
        .gap(px(t::S2))
        .cursor_pointer()
        .when(adding, |d| d.opacity(0.4))
        .on_click(cx.listener(move |t, _, _, cx| on(t, Act::Toggle, cx)))
        .child(
            div()
                .size(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(if p.checked() { t::accent() } else { t::edge() })
                .when(p.checked(), |d| d.bg(t::accent()))
                .text_size(px(t::CAPTION))
                .text_color(t::on_accent())
                .child(if p.checked() { "\u{2713}" } else { "" }),
        )
        .child(
            div()
                .text_size(px(t::BODY))
                .text_color(t::text_secondary())
                .child("Use as default profile"),
        );
    ui::fade_in(
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(t::S8))
            .child(ui::wordmark(t::HEADING))
            .child(
                div()
                    .text_size(px(36.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t::text())
                    .child("Who's watching?"),
            )
            .child(row)
            .child(check)
            .child(ui::mono(
                "\u{2190} \u{2192} move    ENTER choose    SPACE default profile",
            )),
        "picker".into(),
        t::SLOW,
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Act, DefaultChange, Picker, Start, accounts_of, columns, decide};
    use nocturne_session::{Account, Accounts};

    fn acct(id: &str, default: bool) -> Account {
        Account {
            server: "http://h:1".into(),
            user_id: id.into(),
            user_name: id.to_uppercase(),
            image_tag: None,
            default,
        }
    }

    fn list(n: usize) -> Vec<Account> {
        (0..n).map(|i| acct(&format!("u{i}"), false)).collect()
    }

    #[test]
    fn start_decision() {
        assert_eq!(decide(false, &list(0)), Start::Restore, "no account");
        assert_eq!(
            decide(false, &list(1)),
            Start::Restore,
            "one account: as today"
        );
        let mut one = list(1);
        one[0].default = true;
        assert_eq!(
            decide(false, &one),
            Start::Restore,
            "a default alone changes nothing"
        );
        assert_eq!(
            decide(false, &list(3)),
            Start::Picker,
            "several, no default"
        );
        let mut l = list(3);
        l[1].default = true;
        assert_eq!(
            decide(false, &l),
            Start::Direct(1),
            "default skips the picker"
        );
        assert_eq!(
            decide(true, &l),
            Start::Restore,
            "developer-token mode skips it"
        );
        assert_eq!(decide(true, &list(3)), Start::Restore);
        l[2].default = true; // hand-edited: first wins
        assert_eq!(decide(false, &l), Start::Direct(1));
    }

    #[test]
    fn stale_default_is_ignored() {
        // The default marker belongs to a profile of another server or one that was removed.
        let mut all = Accounts::default();
        let s = |id: &str, name: &str| nocturne_jellyfin_user::Session {
            access_token: "t".into(),
            user_id: id.into(),
            server_id: "s".into(),
            user_name: name.into(),
            is_admin: false,
        };
        all.remember("http://h:1", &s("a", "A"));
        all.remember("http://h:1", &s("b", "B"));
        all.remember("http://o:2", &s("c", "C"));
        assert!(all.set_default("http://o:2", Some("c")));
        assert_eq!(
            decide(false, &accounts_of(&all, "http://h:1")),
            Start::Picker,
            "another server's default does not apply"
        );
        assert!(all.set_default("http://h:1", Some("a")));
        all.forget("http://h:1", "a");
        assert_eq!(
            decide(false, &accounts_of(&all, "http://h:1")),
            Start::Restore
        );
        all.remember("http://h:1", &s("a", "A")); // comes back as a fresh, non-default profile
        assert_eq!(
            decide(false, &accounts_of(&all, "http://h:1")),
            Start::Picker
        );
    }

    #[test]
    fn arrows_wrap_enter_chooses_escape_does_nothing() {
        let mut p = Picker::new("s", list(3), None, 10);
        assert_eq!((p.tiles(), p.sel), (4, 0));
        assert_eq!(p.key("left"), Act::Stay);
        assert_eq!(p.sel, 3, "left wraps to Add profile");
        assert_eq!(p.key("enter"), Act::Add);
        p.key("right");
        assert_eq!(p.sel, 0, "right wraps to the first");
        p.key("right");
        assert_eq!(p.key("enter"), Act::Pick(1));
        assert_eq!(p.key("escape"), Act::Stay);
        assert_eq!(p.sel, 1, "Esc changes nothing");
        p.busy = true;
        assert_eq!(p.key("enter"), Act::Stay, "ignored while signing in");
        assert_eq!(p.act_on(0), Act::Stay);
    }

    #[test]
    fn up_down_move_by_rows_of_a_wrapped_layout() {
        // 7 tiles in rows of 3: [0 1 2] [3 4 5] [6]
        let mut p = Picker::new("s", list(6), None, 3);
        p.key("down");
        assert_eq!(p.sel, 3);
        p.key("down");
        assert_eq!(p.sel, 6);
        p.key("down");
        assert_eq!(p.sel, 6, "bottom edge stays");
        p.key("up");
        assert_eq!(p.sel, 3);
        p.key("up");
        p.key("up");
        assert_eq!(p.sel, 0, "top edge stays");
        p.sel = 5;
        p.key("down");
        assert_eq!(p.sel, 6, "a short last row is reached from above");
        let mut one_row = Picker::new("s", list(2), None, 5);
        one_row.key("down");
        assert_eq!(one_row.sel, 0, "single row: no vertical move");
        assert_eq!(columns(1280.0), 6);
        assert_eq!(columns(800.0), 3);
        assert_eq!(columns(100.0), 1);
    }

    #[test]
    fn opens_on_default_or_current_profile() {
        let mut l = list(3);
        assert_eq!(
            Picker::new("s", l.clone(), Some("u2"), 4).sel,
            2,
            "the signed-in one"
        );
        l[1].default = true;
        assert_eq!(
            Picker::new("s", l, Some("u2"), 4).sel,
            1,
            "the default wins"
        );
        assert_eq!(Picker::new("s", list(3), Some("gone"), 4).sel, 0);
    }

    #[test]
    fn default_box_sets_clears_and_never_changes_unasked() {
        let mut p = Picker::new("s", list(3), None, 4);
        assert!(!p.checked(), "off by default");
        assert_eq!(
            p.default_change(0),
            DefaultChange::Keep,
            "just choosing keeps it"
        );
        assert_eq!(p.key("space"), Act::Stay);
        assert!(p.checked());
        p.key("right");
        assert!(p.checked(), "a touched box stays as the user left it");
        assert_eq!(p.default_change(1), DefaultChange::Set);
        p.toggle();
        assert_eq!(
            p.default_change(1),
            DefaultChange::Keep,
            "off and not the default"
        );

        // Switch profile with a default set: the box shows it, choosing keeps it, unticking clears it.
        let mut l = list(3);
        l[0].default = true;
        let mut p = Picker::new("s", l, None, 4);
        assert!(p.checked() && p.sel == 0);
        assert_eq!(p.default_change(0), DefaultChange::Keep);
        p.key("right");
        assert!(!p.checked(), "untouched box follows the highlight");
        assert_eq!(
            p.default_change(1),
            DefaultChange::Keep,
            "other profile: default stays"
        );
        p.key("left");
        p.toggle();
        assert!(!p.checked());
        assert_eq!(p.default_change(0), DefaultChange::Clear);
        assert_eq!(p.default_change(2), DefaultChange::Keep);
        p.toggle();
        p.key("right");
        assert_eq!(p.default_change(1), DefaultChange::Set, "moves the default");
    }
}
