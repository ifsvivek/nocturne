//! Two small overlays opened from the keyboard or the account menu: the shortcut reference
//! (`?`) and the list of this account's own sessions.
use super::*;
use nocturne_jellyfin_user::OwnSession;

/// (heading, [(keys, what it does)]). The player has its own `?` panel with playback keys.
pub(super) const SHORTCUTS: [(&str, &[(&str, &str)]); 7] = [
    (
        "Everywhere",
        &[
            ("Arrow keys", "Move the focus"),
            ("Enter", "Open or activate"),
            ("Esc / Backspace", "Go back, close a panel"),
            ("Alt + Left / Right", "Back / forward"),
            ("Ctrl + K or /", "Search"),
            ("a", "Account menu"),
            ("?", "This help"),
        ],
    ),
    (
        "Library",
        &[
            ("o", "Sort"),
            ("l", "Filters"),
            ("v", "Poster / thumb / list view"),
            ("Tab", "Next tab"),
            ("w", "Mark watched / unwatched"),
            ("f", "Favourite"),
        ],
    ),
    (
        "Photos",
        &[
            ("Left / Right  p n", "Previous / next photo"),
            ("+ -  Up / Down", "Zoom in / out"),
            ("0", "Reset zoom"),
            ("Esc", "Close the viewer"),
        ],
    ),
    (
        "Playlists and music queue",
        &[
            ("n", "New playlist (on the Playlists page)"),
            (
                "x  [  ]",
                "Remove / move up / move down the selected entry of your playlist",
            ),
            ("More ...", "Add to playlist, Play next, Add to queue"),
            ("Account menu", "Play or clear the music queue"),
            (
                "u (player)",
                "Queue: Enter play, [ ] move, X remove, Z shuffle, R repeat",
            ),
        ],
    ),
    (
        "Admin",
        &[
            ("Up / Down", "Choose a section or row"),
            ("Left / Right", "Move between buttons, back to the sidebar"),
            ("Enter", "Press the focused button"),
            ("r", "Reload this page"),
            ("Esc", "Leave Admin"),
        ],
    ),
    (
        "Dialogs",
        &[
            ("Tab / Up / Down", "Next / previous field or button"),
            ("Enter", "Next field, or press the focused button"),
            ("Esc", "Cancel"),
        ],
    ),
    (
        "Player",
        &[("Space", "Play / pause"), ("?", "All playback keys")],
    ),
];

impl Root {
    pub(super) fn overlay_panel(&self, title: &str) -> Stateful<Div> {
        div()
            .id(ui::name(format!("ov-{title}")))
            .w(px(560.0))
            .max_h(px(f32::from(self.win.height) - 96.0))
            .overflow_y_scroll()
            .p(px(t::S6))
            .flex()
            .flex_col()
            .gap(px(t::S3))
            .rounded(px(t::R_PANEL))
            .border_1()
            .border_color(t::hairline_strong())
            .bg(t::surface())
            .occlude()
            .child(
                div()
                    .text_size(px(t::HEADING))
                    .font_weight(FontWeight::BOLD)
                    .child(title.to_string()),
            )
    }

    fn overlay_backdrop(
        &self,
        id: &'static str,
        panel: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(rgba(0x000000b8))
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, _, _, cx| {
                    t.close_overlays();
                    cx.notify();
                }),
            )
            .child(panel)
            .into_any_element()
    }

    pub(super) fn close_overlays(&mut self) {
        self.help_open = false;
        self.devices = None;
    }

    pub(super) fn help_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.help_open {
            return None;
        }
        let mut cols = div().flex().flex_wrap().gap_x(px(t::S8)).gap_y(px(t::S4));
        for (head, rows) in SHORTCUTS {
            let mut sec = div()
                .w(px(500.0))
                .flex()
                .flex_col()
                .gap(px(t::S1))
                .child(ui::mono(ui::tracked(head)));
            for (k, d) in rows {
                sec = sec.child(
                    div()
                        .flex()
                        .gap(px(t::S4))
                        .text_size(px(t::BODY))
                        .child(
                            div()
                                .w(px(170.0))
                                .flex_none()
                                .font_family(t::MONO)
                                .text_color(t::accent_text())
                                .child(*k),
                        )
                        .child(div().text_color(t::text_secondary()).child(*d)),
                );
            }
            cols = cols.child(sec);
        }
        let p = self
            .overlay_panel("Keyboard shortcuts")
            .w(px(1100.0))
            .child(cols)
            .child(ui::mono("Press Esc or ? to close".to_string()));
        Some(self.overlay_backdrop("help-away", p, cx))
    }

    /// Loads and shows this account's sessions (read only).
    pub(super) fn open_devices(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self
            .session
            .as_ref()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok())
        else {
            return;
        };
        self.devices = Some(Load::Loading);
        bridge::run(cx, async move { c.own_sessions().await.ok() }, |t, r, _| {
            if t.devices.is_some() {
                t.devices = Some(r.map_or(Load::Failed, Load::Ready));
            }
        });
    }

    pub(super) fn devices_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let l = self.devices.as_ref()?;
        let mut p = self.overlay_panel("Your devices").child(
            div()
                .text_size(px(t::BODY))
                .text_color(t::text_secondary())
                .child(
                    "Sessions this account has open on the server. To end one, sign out on that device.",
                ),
        );
        match l {
            Load::Loading => p = p.child(ui::mono("Loading".to_string())),
            Load::Failed => {
                p = p.child(ui::mono(
                    "This couldn't be loaded. Close and try again.".to_string(),
                ))
            }
            Load::Ready(v) => {
                for s in v {
                    p = p.child(device_row(s));
                }
            }
        }
        p = p.child(
            div()
                .pt(px(t::S2))
                .child(ui::mono("Press Esc to close".to_string())),
        );
        Some(self.overlay_backdrop("devices-away", p, cx))
    }
}

fn device_row(s: &OwnSession) -> Div {
    let last = if s.last_activity_date.is_empty() {
        "now".to_string()
    } else {
        crate::admin::when(Some(&s.last_activity_date))
    };
    div()
        .py(px(t::S2))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(t::S4))
        .border_t_1()
        .border_color(t::hairline())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(t::BODY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{} on {}", s.client, s.device_name)),
                )
                .child(ui::mono(format!(
                    "{} · active {last}",
                    s.application_version
                ))),
        )
        .when(s.current, |d| d.child(ui::mono("this device".to_string())))
}

#[cfg(test)]
mod tests {
    use super::SHORTCUTS;

    #[test]
    fn shortcut_reference_is_complete_and_unambiguous() {
        for (head, rows) in SHORTCUTS {
            assert!(!head.is_empty() && !rows.is_empty());
            for (k, d) in rows {
                assert!(!k.is_empty() && !d.is_empty(), "{head}");
            }
            let mut keys: Vec<_> = rows.iter().map(|r| r.0).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), rows.len(), "duplicate key in {head}");
        }
    }
}
