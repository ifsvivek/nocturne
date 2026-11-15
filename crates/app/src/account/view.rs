//! Account screens in the app's design language: centred panel on the grain/glow backdrop,
//! hairline fields, mono annotations, one primary action per screen.
use super::state::*;
use super::{Accounts, DlgKind, PwDialog};
use crate::{theme as t, ui};
use gpui::prelude::*;
use gpui::*;

fn frame(content: impl IntoElement) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(t::S4))
        .child(ui::wordmark(t::DISPLAY))
        .child(ui::label("Your media. Your night."))
        .child(div().h(px(t::S4)))
        .child(content)
}

fn panel() -> Stateful<Div> {
    div()
        .id("acct-panel")
        .w(px(440.0))
        .p(px(t::S6))
        .flex()
        .flex_col()
        .gap(px(t::S4))
        .rounded(px(t::R_PANEL))
        .border_1()
        .border_color(t::hairline_strong())
        .bg(t::surface())
        .occlude()
}

fn heading(eyebrow: &str, title: String, body: Option<String>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(t::S2))
        .child(ui::mono(ui::tracked(eyebrow)))
        .child(
            div()
                .text_size(px(t::HEADING))
                .font_weight(FontWeight::BOLD)
                .child(title),
        )
        .children(body.map(|b| {
            div()
                .text_size(px(t::BODY))
                .line_height(px(t::BODY * 1.5))
                .text_color(t::text_secondary())
                .child(b)
        }))
}

/// Where the README's getting-started section lives.
const DOCS_URL: &str = "https://github.com/ifsvivek/nocturne#readme";

/// Three short steps for a first launch.
fn steps() -> Div {
    let line = |n: &'static str, s: &'static str| {
        div()
            .flex()
            .gap(px(t::S3))
            .text_size(px(t::BODY))
            .text_color(t::text_secondary())
            .child(div().flex_none().child(ui::mono(n)))
            .child(div().flex_1().min_w_0().child(s))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(t::S1))
        .child(line(
            "01",
            "Type your server's address, like http://192.168.1.10:8096.",
        ))
        .child(line(
            "02",
            "Sign in with your account, or approve with Quick Connect.",
        ))
        .child(line("03", "Pick something and press play."))
}

fn note(msg: Option<&'static str>) -> Option<Div> {
    msg.map(|m| {
        div()
            .text_size(px(t::BODY))
            .line_height(px(t::BODY * 1.4))
            .text_color(t::warn())
            .child(m)
    })
}

impl Accounts {
    fn field(
        &self,
        id: &'static str,
        i: usize,
        f: &Fld,
        focused: bool,
        on_focus: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let shown = if f.secret {
            "\u{2022}".repeat(f.value.chars().count())
        } else {
            f.value.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(t::S1))
            .child(ui::mono(f.label.to_string()))
            .child(
                div()
                    .id(ui::name(format!("{id}-{i}")))
                    .h(px(40.0))
                    .px(px(t::S3))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(if focused {
                        t::accent()
                    } else {
                        t::hairline_strong()
                    })
                    .font_family(t::MONO)
                    .text_size(px(t::BODY))
                    .overflow_hidden()
                    .child(format!("{shown}{}", if focused { "\u{2595}" } else { "" }))
                    .on_click(cx.listener(move |a, _, _, cx| {
                        on_focus(a, cx);
                        cx.notify();
                    })),
            )
    }

    fn connect_screen(&self, cx: &mut Context<Self>) -> Div {
        let st = &self.st;
        let form = &st.connect;
        let mut p = panel()
            .child(heading(
                "Connect",
                "Where is your media?".into(),
                Some("Enter the address of your server.".into()),
            ))
            .child(steps())
            .child(self.field(
                "conn-f",
                0,
                &form.fields[0],
                form.focus == 0,
                |a, _| a.st.connect.focus = 0,
                cx,
            ))
            .children(note(st.msg))
            .child(
                div().flex().items_center().justify_between().child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(t::S3))
                        .child(ui::action_button(
                            "conn-go".into(),
                            "Connect",
                            true,
                            false,
                            form.focus == 1,
                            st.busy,
                            cx.listener(|a, _, _, cx| a.connect_go(cx)),
                        ))
                        .children(st.busy.then(|| {
                            div().child(ui::mono("CHECKING")).with_animation(
                                "chk",
                                Animation::new(std::time::Duration::from_millis(1200))
                                    .repeat()
                                    .with_easing(ease_in_out),
                                |d, v| d.opacity(0.35 + 0.65 * (1.0 - (2.0 * v - 1.0).abs())),
                            )
                        })),
                ),
            );
        if !st.servers.is_empty() {
            p = p
                .child(div().h(px(1.0)).bg(t::hairline()))
                .child(ui::mono(ui::tracked("Remembered servers")));
            for (i, s) in st.servers.iter().enumerate() {
                let on = form.focus == form.fields.len() + 1 + i;
                p = p.child(
                    div()
                        .id(ui::name(format!("srv-{i}")))
                        .h(px(36.0))
                        .px(px(t::S3))
                        .flex()
                        .items_center()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(if on { t::accent() } else { t::hairline() })
                        .bg(if on { t::accent_tint() } else { rgba(0) })
                        .cursor_pointer()
                        .hover(|s| s.bg(t::glass()))
                        .justify_between()
                        .child(ui::mono(s.clone()).text_color(t::text()))
                        .child(
                            div()
                                .id(ui::name(format!("srv-forget-{i}")))
                                .px(px(t::S2))
                                .text_size(px(t::CAPTION + 1.0))
                                .text_color(t::text_secondary())
                                .cursor_pointer()
                                .hover(|s| s.text_color(t::danger()))
                                .tooltip(ui::tip(
                                    "Forget this server and its saved sign-in (Delete)",
                                ))
                                .on_click(cx.listener(move |a, _, _, cx| {
                                    a.forget(i, cx);
                                    cx.stop_propagation();
                                }))
                                .child("Forget"),
                        )
                        .on_click(cx.listener(move |a, _, _, cx| a.pick(i, cx))),
                );
            }
        }
        p = p.child(div().h(px(1.0)).bg(t::hairline())).child(
            div()
                .id("conn-docs")
                .text_size(px(t::CAPTION + 1.0))
                .text_color(t::text_secondary())
                .cursor_pointer()
                .hover(|s| s.text_color(t::accent_text()))
                .tooltip(ui::tip("Open the setup guide in your browser"))
                .on_click(|_, _, _| {
                    let _ = std::process::Command::new("xdg-open").arg(DOCS_URL).spawn();
                })
                .child("New here? Read the setup guide in the README."),
        );
        frame(p)
    }

    fn login_screen(&self, cx: &mut Context<Self>) -> Div {
        let st = &self.st;
        if let Qc::Waiting(code) = &st.qc {
            return frame(self.qc_panel(Some(code.clone()), cx));
        }
        if st.qc == Qc::Starting {
            return frame(self.qc_panel(None, cx));
        }
        let f = &st.login;
        let mut p = panel()
            .child(heading(
                "Sign in",
                st.server_name.clone(),
                Some(st.url.clone()),
            ))
            .child(self.field(
                "login-f",
                0,
                &f.fields[0],
                f.focus == 0,
                |a, _| a.st.login.focus = 0,
                cx,
            ))
            .child(self.field(
                "login-f",
                1,
                &f.fields[1],
                f.focus == 1,
                |a, _| a.st.login.focus = 1,
                cx,
            ))
            .children(note(st.msg));
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(t::S2))
            .child(ui::action_button(
                "login-go".into(),
                "Sign in",
                true,
                false,
                f.focus == 2,
                st.busy,
                cx.listener(|a, _, _, cx| a.login_go(cx)),
            ));
        if st.qc_available {
            row = row.child(ui::action_button(
                "login-qc".into(),
                "Quick Connect",
                false,
                false,
                f.focus == 3,
                st.busy,
                cx.listener(|a, _, _, cx| a.qc_go(cx)),
            ));
        }
        row = row.child(ui::action_button(
            "login-back".into(),
            "Change server",
            false,
            false,
            f.focus == 2 + f.buttons - 1,
            false,
            cx.listener(|a, _, _, cx| a.back_to_servers(cx)),
        ));
        p = p.child(row);
        if st.keyring_down {
            p = p.child(ui::mono(KEYRING_NOTE));
        }
        frame(p)
    }

    fn qc_panel(&self, code: Option<String>, cx: &mut Context<Self>) -> Stateful<Div> {
        let big = div()
            .h(px(88.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(t::accent())
            .bg(t::accent_tint())
            .font_family(t::MONO)
            .text_size(px(48.0))
            .font_weight(FontWeight::BOLD)
            .child(code.clone().unwrap_or_else(|| "\u{2026}".into()));
        panel()
            .child(heading(
                "Quick Connect",
                "Enter this code".into(),
                Some(
                    "On a device where you're already signed in, open Quick Connect and type the code."
                        .into(),
                ),
            ))
            .child(big)
            .child(
                div()
                    .child(ui::mono(if code.is_some() {
                        "WAITING FOR APPROVAL"
                    } else {
                        "GETTING A CODE"
                    }))
                    .with_animation(
                        "qc-wait",
                        Animation::new(std::time::Duration::from_millis(1600))
                            .repeat()
                            .with_easing(ease_in_out),
                        |d, v| d.opacity(0.35 + 0.65 * (1.0 - (2.0 * v - 1.0).abs())),
                    ),
            )
            .child(
                div().flex().justify_end().child(ui::action_button(
                    "qc-cancel".into(),
                    "Cancel",
                    false,
                    false,
                    true,
                    false,
                    cx.listener(|a, _, _, cx| a.qc_stop(cx)),
                )),
            )
    }

    fn pw_dialog(&self, d: &PwDialog, cx: &mut Context<Self>) -> Div {
        let f = &d.form;
        let (title, body, ok_label) = match d.kind {
            DlgKind::Password => (
                "Change your password",
                "You'll stay signed in on this device.",
                "Change password",
            ),
            DlgKind::Authorize => (
                "Authorize a device",
                "On the other device choose Quick Connect, then type the code it shows. It signs in as you.",
                "Authorize",
            ),
            DlgKind::ProfileImage => (
                "Profile image",
                "Uploads a picture from this computer as your profile image on the server.",
                "Upload",
            ),
            DlgKind::RemoveImage => (
                "Remove profile image",
                "Your profile image is deleted from the server.",
                "Remove image",
            ),
            DlgKind::Language => (
                "Display language",
                "The language the server keeps for your account. Other Jellyfin apps you use read it too.",
                "Save",
            ),
            DlgKind::NewPlaylist => ("New playlist", "Make an empty playlist you own.", "Create"),
            DlgKind::RenamePlaylist => {
                ("Rename playlist", "Give the playlist a new name.", "Rename")
            }
            DlgKind::DeletePlaylist => ("Delete playlist", "", "Delete playlist"),
            DlgKind::AddToPlaylist => (
                "Add to a playlist",
                "Type the name of one of your playlists to add to it, or a new name to start one.",
                "Add",
            ),
        };
        let body = match &d.hint {
            Some(h) if body.is_empty() => h.clone(),
            Some(h) => format!("{body} {h}"),
            None => body.to_string(),
        };
        let mut p = panel()
            .w(px(480.0))
            .child(heading("Please confirm", title.into(), Some(body)));
        for (i, fld) in f.fields.iter().enumerate() {
            p = p.child(self.field(
                "pw-f",
                i,
                fld,
                f.focus == i,
                move |a, _| {
                    if let Some(d) = a.dialog.as_mut() {
                        d.form.focus = i
                    }
                },
                cx,
            ));
        }
        let (ci, oi) = (f.fields.len(), f.fields.len() + 1);
        p = p.children(note(d.msg)).child(
            div()
                .flex()
                .justify_end()
                .gap(px(t::S2))
                .child(ui::action_button(
                    "pw-cancel".into(),
                    "Cancel",
                    false,
                    false,
                    f.focus == ci,
                    false,
                    cx.listener(|a, _, _, cx| a.dialog_press(0, cx)),
                ))
                .child(ui::action_button(
                    "pw-ok".into(),
                    ok_label,
                    true,
                    false,
                    f.focus == oi,
                    d.busy,
                    cx.listener(|a, _, _, cx| a.dialog_press(1, cx)),
                )),
        );
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(rgba(0x000000b8))
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(p)
    }
}

impl Render for Accounts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(d) = &self.dialog {
            return self.pw_dialog(d, cx).into_any_element();
        }
        if self.st.restoring && self.st.busy {
            return frame(
                div()
                    .child(ui::label("Restoring your session"))
                    .with_animation(
                        "restore",
                        Animation::new(std::time::Duration::from_millis(1600))
                            .repeat()
                            .with_easing(ease_in_out),
                        |d, v| d.opacity(0.35 + 0.65 * (1.0 - (2.0 * v - 1.0).abs())),
                    ),
            )
            .into_any_element();
        }
        match self.st.stage {
            Stage::Connect => self.connect_screen(cx).into_any_element(),
            Stage::Login => self.login_screen(cx).into_any_element(),
            Stage::Active => div().into_any_element(),
        }
    }
}
