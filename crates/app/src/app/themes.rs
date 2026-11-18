//! Themes page (Settings > Themes): the saved CSS themes, a multi-line editor with the warnings of
//! the translation, import from a file or a typed https address (fetched once, remembered for a
//! manual Refresh), a 15 second preview on the whole app, and the per-server override. Storage and
//! rules live in `nocturne_core::customize`; the tokens are applied through `theme::set`.
use super::App;
use super::message::Message;
use crate::style::{self, border};
use crate::task::run;
use crate::theme as t;
use crate::widget::press::Press;
use crate::widget::{action_button, mono, rule, section, tracked};
use iced::widget::button::{Status, Style};
use iced::widget::{
    column, container, row, scrollable as scroll, space, text, text_editor, text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::customize::{self as cz, MAX_BYTES, Parsed};
use nocturne_core::profile::ProfileKind;
use nocturne_core::source::SourceId;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

type El<'a> = Element<'a, Message>;

/// How long a preview of the whole app lasts unless it is kept.
const PREVIEW: Duration = Duration::from_secs(15);
/// Warnings shown in the list (the summary line counts all of them).
const SHOWN: usize = 80;

#[derive(Clone)]
pub enum Msg {
    Open,
    Close,
    /// A saved theme, or `None` for a new one.
    Pick(Option<u32>),
    Name(String),
    Edit(text_editor::Action),
    /// The translation of the editor text at that epoch.
    Parsed(u64, Result<Arc<Parsed>, String>),
    Path(String),
    Choose,
    Picked(Option<PathBuf>),
    Import,
    Loaded(Result<String, String>),
    Url(String),
    Download,
    Fetched(Result<String, String>),
    Preview,
    Revert,
    Save,
    /// Save and make it the global theme.
    Apply,
    /// Back to the built-in look (the saved themes stay).
    Reset,
    Delete,
    ConfirmDelete,
    /// Next theme for that server: global, then each saved theme.
    Cycle(SourceId),
}

pub struct State {
    pub open: bool,
    sel: Option<u32>,
    name: String,
    content: text_editor::Content,
    path: String,
    url: String,
    parsed: Option<Arc<Parsed>>,
    epoch: u64,
    note: Option<(String, bool)>,
    confirm_delete: bool,
    busy: bool,
    preview: Option<Instant>,
    /// The theme id last handed to `theme::set` (`None` = not yet / must be redone).
    applied: Option<Option<u32>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            sel: None,
            name: String::new(),
            content: text_editor::Content::new(),
            path: String::new(),
            url: String::new(),
            parsed: None,
            epoch: 0,
            note: None,
            confirm_delete: false,
            busy: false,
            preview: None,
            applied: None,
        }
    }
}

fn file_name_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    last.strip_suffix(".css").unwrap_or(last).to_string()
}

impl App {
    /// The server whose override applies to what is on screen: a detail page's own server, else
    /// the single server picked in the source filter. The shell and local folders follow global.
    fn theme_source(&self) -> Option<SourceId> {
        use nocturne_core::sources::SourceFilter;
        match self.stack.last() {
            Some(p) if p.view == super::View::Detail => p.media.as_ref().map(|m| m.origin),
            _ => match self.filter {
                SourceFilter::One(id) => Some(id),
                SourceFilter::All => None,
            },
        }
    }

    /// Make the active theme match the selection; cheap when nothing changed. Called after every
    /// message, never while drawing.
    pub(super) fn sync_theme(&mut self) {
        if self.th.preview.is_some() {
            return;
        }
        let want = self.themes.active_id(self.theme_source());
        if self.th.applied != Some(want) {
            crate::theme::set(&self.themes.resolve(self.theme_source()));
            self.th.applied = Some(want);
        }
    }

    /// End an expired preview (root tick).
    pub(super) fn theme_tick(&mut self, now: Instant) {
        if self.th.preview.is_some_and(|d| now >= d) {
            self.th.preview = None;
            self.th.applied = None;
        }
    }

    pub(super) fn is_previewing(&self) -> bool {
        self.th.preview.is_some()
    }

    fn reparse(&mut self) -> Task<Message> {
        self.th.epoch += 1;
        let (epoch, text) = (self.th.epoch, self.th.content.text());
        run(
            async move { cz::parse(&text).map(Arc::new).map_err(|e| e.to_string()) },
            move |r| Message::Themes(Msg::Parsed(epoch, r)),
        )
    }

    fn set_text(&mut self, s: &str) -> Task<Message> {
        self.th.content = text_editor::Content::with_text(s);
        self.reparse()
    }

    fn note(&mut self, s: impl Into<String>, err: bool) {
        self.th.note = Some((s.into(), err));
    }

    /// Store the editor's theme (new or existing); returns its id.
    fn save_theme(&mut self) -> u32 {
        let css = self.th.content.text();
        let url = Some(self.th.url.trim().to_string()).filter(|u| !u.is_empty());
        let id = match self.th.sel {
            Some(id) if self.themes.update(id, &self.th.name, &css, url.clone()) => id,
            _ => self.themes.add(&self.th.name, &css, url),
        };
        self.th.sel = Some(id);
        self.th.name = self
            .themes
            .get(id)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        self.themes.save();
        self.th.applied = None;
        id
    }

    pub(super) fn themes_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Open => {
                self.close_admin();
                self.th.open = true;
                let start = self.themes.global;
                return self.themes_msg(Msg::Pick(start));
            }
            Msg::Close => {
                self.th.open = false;
                self.th.preview = None;
                self.th.applied = None;
                self.settings_open = true;
            }
            Msg::Pick(id) => {
                self.th.preview = None;
                self.th.applied = None;
                self.th.confirm_delete = false;
                self.th.note = None;
                self.th.sel = id;
                let saved = id.and_then(|i| self.themes.get(i)).cloned();
                self.th.name = saved.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                self.th.url = saved
                    .as_ref()
                    .and_then(|s| s.url.clone())
                    .unwrap_or_default();
                return self.set_text(&saved.map(|s| s.css).unwrap_or_default());
            }
            Msg::Name(s) => self.th.name = s.chars().take(48).collect(),
            Msg::Edit(a) => {
                let edit = a.is_edit();
                self.th.content.perform(a);
                if edit {
                    return self.reparse();
                }
            }
            Msg::Parsed(e, r) if e == self.th.epoch => match r {
                Ok(p) => {
                    self.th.parsed = Some(p);
                    self.th.note = None;
                }
                Err(e) => {
                    self.th.parsed = None;
                    self.note(e, true);
                }
            },
            Msg::Parsed(..) => {}
            Msg::Path(s) => self.th.path = s,
            Msg::Choose => {
                return run(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose a CSS file")
                            .add_filter("CSS", &["css"])
                            .pick_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |p| Message::Themes(Msg::Picked(p)),
                );
            }
            Msg::Picked(Some(p)) => {
                self.th.path = p.display().to_string();
                return self.themes_msg(Msg::Import);
            }
            Msg::Picked(None) => {}
            Msg::Import => {
                let p = PathBuf::from(self.th.path.trim());
                if self.th.name.trim().is_empty() {
                    self.th.name = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                }
                return run(read_css(p), |r| Message::Themes(Msg::Loaded(r)));
            }
            Msg::Loaded(Ok(s)) => {
                self.note(
                    "Imported. Check the result below, then Preview or Save.",
                    false,
                );
                return self.set_text(&s);
            }
            Msg::Loaded(Err(e)) => self.note(e, true),
            Msg::Url(s) => self.th.url = s,
            Msg::Download => match cz::validate_url(&self.th.url) {
                Ok(u) => {
                    self.th.url = u.clone();
                    self.th.busy = true;
                    self.note("Downloading...", false);
                    return run(async move { cz::fetch(&u).await }, |r| {
                        Message::Themes(Msg::Fetched(r))
                    });
                }
                Err(e) => self.note(e, true),
            },
            Msg::Fetched(r) => {
                self.th.busy = false;
                match r {
                    Ok(s) => {
                        if self.th.name.trim().is_empty() {
                            self.th.name = file_name_of(&self.th.url);
                        }
                        self.note(
                            "Downloaded once. Nothing updates on its own; use Refresh to fetch again.",
                            false,
                        );
                        return self.set_text(&s);
                    }
                    Err(e) => self.note(e, true),
                }
            }
            Msg::Preview => {
                if let Some(p) = &self.th.parsed {
                    crate::theme::set(&p.theme);
                    self.th.preview = Some(Instant::now() + PREVIEW);
                }
            }
            Msg::Revert => {
                self.th.preview = None;
                self.th.applied = None;
            }
            Msg::Save => {
                self.save_theme();
                self.th.preview = None;
                self.note("Saved.", false);
            }
            Msg::Apply => {
                let id = self.save_theme();
                self.themes.set_global(Some(id));
                self.themes.save();
                self.th.preview = None;
                self.th.applied = None;
                self.note("Applied to the whole app.", false);
            }
            Msg::Reset => {
                self.themes.set_global(None);
                self.themes.save();
                self.th.preview = None;
                self.th.applied = None;
                self.note("Using the built-in look.", false);
            }
            Msg::Delete => self.th.confirm_delete = self.th.sel.is_some(),
            Msg::ConfirmDelete => {
                if let Some(id) = self.th.sel.take() {
                    self.themes.delete(id);
                    self.themes.save();
                }
                self.th.applied = None;
                return self.themes_msg(Msg::Pick(None));
            }
            Msg::Cycle(src) => {
                let mut order = vec![None];
                order.extend(self.themes.themes.iter().map(|t| Some(t.id)));
                let at = order
                    .iter()
                    .position(|i| *i == self.themes.override_of(src))
                    .unwrap_or(0);
                self.themes.set_override(src, order[(at + 1) % order.len()]);
                self.themes.save();
                self.th.applied = None;
            }
        }
        Task::none()
    }

    // ---- view ----

    pub(super) fn themes_page(&self) -> El<'_> {
        let head = section(
            "Themes",
            "Colours, corner radii and a font from a CSS file. Only what NOCTURNE understands is used; everything else is listed below. Nothing is downloaded except the one address you type.",
            "Esc to go back".into(),
        );
        let body = row![self.theme_list(), self.theme_editor()]
            .spacing(t::S6)
            .height(Length::Fill);
        column![
            container(head).padding(Padding {
                top: t::HEADER_H + t::S6,
                ..Padding::ZERO
            }),
            container(body)
                .padding(Padding {
                    top: t::S4,
                    bottom: t::S6,
                    left: t::PAGE_PAD,
                    right: t::PAGE_PAD,
                })
                .height(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    fn theme_list(&self) -> El<'_> {
        let th = &self.th;
        let global = self.themes.global;
        let entry = |label: String, on: bool, tag: &'static str, m: Msg| -> El<'_> {
            Press::new(
                row![
                    text(label)
                        .size(t::CONTROL_TEXT)
                        .font(if on { t::semibold() } else { t::base() })
                        .width(Length::Fill),
                    mono(tag),
                ]
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([t::S2, t::S3])
            .on_press(Message::Themes(m))
            .style(move |st| Style {
                background: (on || matches!(st, Status::Hovered | Status::Pressed))
                    .then(|| if on { t::accent_tint() } else { t::GLASS }.into()),
                text_color: t::text(),
                border: border(t::NONE, 0.0, t::r_xs()),
                ..Default::default()
            })
            .into()
        };
        let mut list = column![entry(
            "New theme".into(),
            th.sel.is_none(),
            "",
            Msg::Pick(None)
        )]
        .spacing(2);
        for s in &self.themes.themes {
            list = list.push(entry(
                s.name.clone(),
                th.sel == Some(s.id),
                if global == Some(s.id) { "ACTIVE" } else { "" },
                Msg::Pick(Some(s.id)),
            ));
        }
        let mut col = column![
            text(tracked("Saved themes"))
                .shaping(text::Shaping::Advanced)
                .size(t::CONTROL_TEXT)
                .font(t::bold())
                .color(t::text_secondary()),
            list,
        ]
        .spacing(t::S3);
        if global.is_none() {
            col = col.push(mono("USING THE BUILT-IN LOOK"));
        }
        // One line per server: the override applies while that server is the one on screen.
        let servers: Vec<_> = self
            .sources
            .profiles
            .iter()
            .filter(|p| matches!(p.kind, ProfileKind::Jellyfin { .. }))
            .collect();
        if !servers.is_empty() && !self.themes.themes.is_empty() {
            col = col.push(rule()).push(
                text(tracked("Per server"))
                    .shaping(text::Shaping::Advanced)
                    .size(t::CONTROL_TEXT)
                    .font(t::bold())
                    .color(t::text_secondary()),
            );
            col = col.push(
                text("Used while that server is the one you are browsing, on its detail pages. The rest of the app and local folders follow the global theme.")
                    .size(t::CAPTION)
                    .color(t::text_tertiary()),
            );
            for p in servers {
                let now = self
                    .themes
                    .override_of(p.id)
                    .and_then(|i| self.themes.get(i))
                    .map_or("Global".to_string(), |s| s.name.clone());
                col = col.push(
                    row![
                        text(p.label.clone())
                            .size(t::CONTROL_TEXT)
                            .width(Length::Fill),
                        action_button(
                            &now,
                            false,
                            false,
                            false,
                            false,
                            Message::Themes(Msg::Cycle(p.id))
                        ),
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                );
            }
        }
        scroll(col)
            .style(style::scroll)
            .width(300)
            .height(Length::Fill)
            .into()
    }

    fn theme_editor(&self) -> El<'_> {
        let th = &self.th;
        let msg = |m: Msg| Message::Themes(m);
        let field =
            |ph: &str, v: &str, f: fn(String) -> Msg| -> text_input::TextInput<'_, Message> {
                text_input(ph, v)
                    .on_input(move |s| Message::Themes(f(s)))
                    .size(t::CONTROL_TEXT)
                    .padding([t::S2, t::S3])
                    .style(style::input)
            };
        let editor = text_editor(&th.content)
            .placeholder(":root { --nocturne-accent: #e5a00d; --nocturne-radius-card: 8px; }")
            .on_action(|a| Message::Themes(Msg::Edit(a)))
            .font(t::MONO)
            .size(t::CONTROL_TEXT)
            .padding(t::S3)
            .height(Length::Fill)
            .style(|_, st| {
                let ring = match st {
                    text_editor::Status::Focused { .. } => t::accent(),
                    text_editor::Status::Hovered => t::text_secondary(),
                    _ => t::edge(),
                };
                text_editor::Style {
                    background: iced::Background::Color(t::NONE),
                    border: border(ring, 1.0, 6.0),
                    placeholder: t::text_tertiary(),
                    value: t::text(),
                    selection: t::accent_tint(),
                }
            });

        let saved_url = th
            .sel
            .and_then(|i| self.themes.get(i))
            .and_then(|s| s.url.as_deref());
        let refresh = saved_url.is_some_and(|u| u == th.url.trim());
        let import = column![
            row![
                field("File path (.css)", &th.path, Msg::Path)
                    .on_submit(msg(Msg::Import))
                    .width(Length::Fill),
                action_button("Choose file", false, false, false, false, msg(Msg::Choose)),
                action_button(
                    "Import",
                    false,
                    false,
                    false,
                    th.path.trim().is_empty(),
                    msg(Msg::Import)
                ),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
            row![
                field("https:// address of a .css file", &th.url, Msg::Url)
                    .on_submit(msg(Msg::Download))
                    .width(Length::Fill),
                action_button(
                    if refresh { "Refresh" } else { "Download" },
                    false,
                    false,
                    false,
                    th.busy || th.url.trim().is_empty(),
                    msg(Msg::Download)
                ),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        ]
        .spacing(t::S2);

        let acts = {
            let previewing = th.preview.is_some();
            let can = th.parsed.is_some();
            let mut r = row![].spacing(t::S2).align_y(Alignment::Center);
            if let Some(d) = th.preview {
                let left = d.saturating_duration_since(self.now).as_secs() + 1;
                r = r
                    .push(action_button(
                        "Keep",
                        true,
                        false,
                        false,
                        false,
                        msg(Msg::Apply),
                    ))
                    .push(action_button(
                        "Revert",
                        false,
                        false,
                        false,
                        false,
                        msg(Msg::Revert),
                    ))
                    .push(
                        text(format!("Previewing the whole app, reverting in {left} s"))
                            .size(t::CAPTION)
                            .color(t::text_secondary()),
                    );
            } else {
                r = r
                    .push(action_button(
                        "Preview",
                        false,
                        false,
                        false,
                        !can,
                        msg(Msg::Preview),
                    ))
                    .push(action_button(
                        "Save",
                        false,
                        false,
                        false,
                        !can,
                        msg(Msg::Save),
                    ))
                    .push(action_button(
                        "Apply",
                        true,
                        false,
                        false,
                        !can,
                        msg(Msg::Apply),
                    ))
                    .push(action_button(
                        "Reset to built-in",
                        false,
                        false,
                        false,
                        false,
                        msg(Msg::Reset),
                    ));
            }
            let del = if th.confirm_delete {
                action_button(
                    "Delete this theme?",
                    false,
                    true,
                    true,
                    false,
                    msg(Msg::ConfirmDelete),
                )
            } else {
                action_button(
                    "Delete",
                    false,
                    true,
                    false,
                    th.sel.is_none() || previewing,
                    msg(Msg::Delete),
                )
            };
            r.push(space().width(Length::Fill)).push(del)
        };

        let mut col = column![
            field("Theme name", &th.name, Msg::Name).width(Length::Fill),
            editor,
            import,
        ]
        .spacing(t::S3);
        if let Some((n, err)) = &th.note {
            col = col.push(text(n.clone()).size(t::CAPTION).color(if *err {
                t::danger()
            } else {
                t::text_secondary()
            }));
        }
        col = col.push(self.theme_report()).push(acts);
        col.width(Length::Fill).height(Length::Fill).into()
    }

    /// "N settings applied", the counts per kind, the contrast notes and the list with positions.
    fn theme_report(&self) -> El<'_> {
        let Some(p) = &self.th.parsed else {
            return space().into();
        };
        let mut lines: Vec<El<'_>> = vec![];
        let mut head = format!("{} settings applied.", p.theme.count());
        let mut kinds: Vec<(cz::WarningKind, usize)> = vec![];
        for w in &p.warnings {
            match kinds.iter_mut().find(|(k, _)| *k == w.kind) {
                Some((_, n)) => *n += 1,
                None => kinds.push((w.kind, 1)),
            }
        }
        for (k, n) in &kinds {
            head.push_str(&format!(" {n} {}.", k.label()));
        }
        if p.dropped_warnings > 0 {
            head.push_str(&format!(" {} more not listed.", p.dropped_warnings));
        }
        lines.push(text(head).size(t::CAPTION).color(t::text()).into());
        for a in crate::theme::advisories(&crate::theme::Tokens::resolve(&p.theme)) {
            lines.push(text(a).size(t::CAPTION).color(t::warn()).into());
        }
        for w in p.warnings.iter().take(SHOWN) {
            let c = if w.kind.informational() {
                t::text_tertiary()
            } else {
                t::text_secondary()
            };
            lines.push(
                row![
                    mono(format!("{}:{}", w.line, w.col)).width(56),
                    text(w.message.clone()).size(t::CAPTION).color(c),
                ]
                .spacing(t::S2)
                .into(),
            );
        }
        container(scroll(column(lines).spacing(2).padding(Padding {
            right: t::S3,
            ..Padding::ZERO
        })))
        .style(style::panel)
        .padding(t::S2)
        .height(150)
        .width(Length::Fill)
        .into()
    }
}

/// Read a stylesheet from disk, UTF-8 only, at most [`MAX_BYTES`].
async fn read_css(p: PathBuf) -> Result<String, String> {
    use std::io::Read;
    let f = std::fs::File::open(&p).map_err(|_| "That file could not be opened".to_string())?;
    let mut buf = vec![];
    f.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| "That file could not be read".to_string())?;
    if buf.len() > MAX_BYTES {
        return Err(format!("That file is larger than {} KB", MAX_BYTES / 1024));
    }
    String::from_utf8(buf).map_err(|_| "That file is not UTF-8 text".into())
}
