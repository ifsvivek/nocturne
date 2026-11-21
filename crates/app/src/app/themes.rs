//! Themes page (Settings > Themes): the saved CSS themes, a multi-line editor with the warnings of
//! the translation, import from a file or a typed https address (fetched once, remembered for a
//! manual Refresh), a preview on the whole app that lasts until Keep or Revert (pasting or importing
//! starts one), and the per-server override. Storage and rules live in `nocturne_core::customize`
//! (`import` fetches `@import` sheets and fonts through the bridge); the tokens are applied through
//! `theme::set` and the fonts are registered with Iced.
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
use nocturne_core::customize::import::{self, LoadedFont, Options, Resolved};
use nocturne_core::customize::{self as cz, MAX_BYTES, Parsed};
use nocturne_core::profile::ProfileKind;
use nocturne_core::source::SourceId;
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type El<'a> = Element<'a, Message>;

/// `State::applied` while the safe look is on.
const SAFE: u64 = u64::MAX;
/// The text under "How themes work".
const HOW: &str = "A theme is a CSS file: colours, corner radii, fonts and layout. Only what NOCTURNE understands is used; everything else is listed in the report. A theme's @import stylesheets and fonts are fetched over https when Settings > Themes > Fetch theme imports is on. Pasting or importing a theme previews it everywhere until you press Keep or Revert. If a theme makes the app hard to use, press Ctrl+Shift+0 or start with NOCTURNE_SAFE_THEME=1 to get the built-in look back.";
/// Warnings shown in the list (the summary line counts all of them).
const SHOWN: usize = 80;
/// Why Download and Refresh are off: the setting means the app fetches no theme files at all.
const OFF_NOTE: &str = "Downloading is off: Settings > Themes > Themes may load from the web.";

#[derive(Clone)]
pub enum Msg {
    Open,
    Close,
    /// A saved theme, or `None` for a new one.
    Pick(Option<u32>),
    /// A built-in example (index into `customize::PRESETS`): its CSS goes into a new theme.
    Preset(usize),
    /// A built-in theme (index into `import::BUILTIN`).
    Builtin(usize),
    Name(String),
    Edit(text_editor::Action),
    /// The translation of the editor text at that epoch, with what was fetched for it.
    Parsed(u64, Result<(Arc<Parsed>, Option<Arc<Resolved>>), String>),
    /// What was fetched for the stylesheet of the active theme (its original text).
    Resolved(String, Arc<Resolved>),
    /// Download everything remote again (imports and fonts), then re-apply.
    Refresh,
    /// The safe key or the button: the built-in look, whatever the themes say.
    Safe,
    LeaveSafe,
    /// Debug hook `theme-report` (`NOCTURNE_THEME_REPORT=<file>`): write what the active theme did as JSON.
    Report(String),
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
    /// Show or hide the "How themes work" text under the header.
    How,
    /// The report panel's tab: `false` this theme, `true` server branding.
    ReportTab(bool),
    /// Copy the open report tab as plain text.
    CopyReport,
    /// The gallery tabs and cards (`themes_gallery`).
    Gallery(super::themes_gallery::GMsg),
}

pub struct State {
    pub open: bool,
    pub(super) sel: Option<u32>,
    pub(super) name: String,
    pub(super) content: text_editor::Content,
    path: String,
    url: String,
    parsed: Option<Arc<Parsed>>,
    epoch: u64,
    note: Option<(String, bool)>,
    confirm_delete: bool,
    busy: bool,
    /// The editor's theme is shown on the whole app until Keep or Revert.
    pub preview: bool,
    /// Start a preview when the next translation arrives (a paste or an import).
    want_preview: bool,
    /// Safe look: the built-in theme, whatever is saved (Ctrl+Shift+0, `NOCTURNE_SAFE_THEME=1`).
    pub safe: bool,
    /// The theme id last handed to `theme::set` (`None` = not yet / must be redone).
    pub(super) applied: Option<u64>,
    /// What was fetched for the editor's text.
    resolved: Option<Arc<Resolved>>,
    /// The editor's imports are being downloaded (the report says so).
    fetching: bool,
    /// Stylesheets being fetched for the active theme, so each is started once.
    resolving: HashSet<String>,
    /// Fonts already handed to Iced (address and weight).
    fonts_loaded: HashSet<String>,
    /// "How themes work" is open.
    how: bool,
    /// The report panel shows the servers' branding and plugin stylesheets.
    brand_tab: bool,
    pub(super) gal: super::themes_gallery::Gallery,
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
            preview: false,
            want_preview: false,
            safe: std::env::var("NOCTURNE_SAFE_THEME").is_ok_and(|v| v == "1"),
            applied: None,
            resolved: None,
            fetching: false,
            resolving: HashSet::new(),
            fonts_loaded: HashSet::new(),
            how: false,
            brand_tab: false,
            gal: Default::default(),
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
    pub(super) fn theme_source(&self) -> Option<SourceId> {
        use nocturne_core::sources::SourceFilter;
        match self.stack.last() {
            Some(p) if p.view == super::View::Detail => p.media.as_ref().map(|m| m.origin),
            _ => match self.filter {
                SourceFilter::One(id) => Some(id),
                SourceFilter::All => None,
            },
        }
    }

    /// The server whose UI is borrowed for what is on screen (`None`: NOCTURNE's).
    pub(super) fn theme_ui(&self) -> Option<SourceId> {
        let item = match self.stack.last() {
            Some(p) if p.view == super::View::Detail => p.media.as_ref().map(|m| m.origin),
            _ => None,
        };
        self.ui_pick(item).branding_from()
    }

    /// The address a stylesheet came from, for its relative imports (saved themes only).
    fn base_of(&self, src: Option<SourceId>, ui: Option<SourceId>) -> Option<String> {
        let id = match src.and_then(|s| self.themes.override_of(s)) {
            Some(i) => i,
            None if ui.is_some_and(|u| self.themes.branding_of(u).is_some()) => return None,
            None => self.themes.global?,
        };
        self.themes.get(id).and_then(|t| t.url.clone())
    }

    /// Fetch what `css` imports and its fonts, off the UI thread.
    fn resolve_task(&self, css: String, base: Option<String>, refresh: bool) -> Task<Message> {
        let remote = self.themes.remote();
        let key = css.clone();
        run(
            async move { import::resolve(&css, base.as_deref(), &Options::new(remote, refresh)).await },
            move |r| Message::Themes(Msg::Resolved(key, Arc::new(r))),
        )
    }

    /// Hand fonts to Iced once each.
    fn load_fonts(&mut self, fonts: &[LoadedFont]) -> Task<Message> {
        if self.th.fonts_loaded.len() > 64 {
            self.th.fonts_loaded.clear();
        }
        let mut tasks = vec![];
        for f in fonts {
            if self
                .th
                .fonts_loaded
                .insert(format!("{}#{}", f.url, f.weight))
            {
                tasks.push(iced::font::load(Cow::Owned(f.bytes.to_vec())).discard());
            }
        }
        Task::batch(tasks)
    }

    /// Make the active theme match the selection; cheap when nothing changed. Called after every
    /// message, never while drawing. Starts the fetch of a stylesheet's imports once.
    pub(super) fn sync_theme(&mut self) -> Task<Message> {
        if self.th.safe {
            if self.th.applied != Some(SAFE) {
                crate::theme::map_fonts(&[]);
                crate::theme::set(&Default::default(), &Default::default());
                self.th.applied = Some(SAFE);
            }
            return Task::none();
        }
        let (src, ui) = (self.theme_source(), self.theme_ui());
        let mut task = Task::none();
        // The editor's text (a preview) is resolved by `reparse_with`, which also stores it; the
        // Studio's working copy and the active theme are resolved here, once per distinct text.
        let mut todo: Vec<(String, Option<String>)> = vec![];
        if !self.th.preview
            && let Some(css) = self.themes.active_css(src, ui)
        {
            todo.push((css.to_string(), self.base_of(src, ui)));
        }
        #[cfg(feature = "theme-engine")]
        if self.studio.open {
            todo.push((self.studio.css.clone(), None));
        }
        // The stylesheets the server's start page adds may import others.
        #[cfg(feature = "theme-engine")]
        if let Some(p) = self.jf_plugins(ui) {
            todo.push((p.css.clone(), None));
        }
        for (css, base) in todo {
            if self.themes.needs_resolve(&css) && self.th.resolving.insert(css.clone()) {
                task = Task::batch([task, self.resolve_task(css, base, false)]);
            }
        }
        if self.th.preview {
            return task;
        }
        let want = self.themes.active_key_with(src, ui);
        if self.th.applied != Some(want) {
            let (th, lay) = self.themes.resolve_all_with(src, ui);
            let fonts = self
                .themes
                .active_css(src, ui)
                .map_or(&[][..], |c| self.themes.fonts_for(c));
            crate::theme::map_fonts(fonts);
            crate::theme::set(&th, &lay);
            self.th.applied = Some(want);
        }
        task
    }

    /// Counts for the theme conformance harness: tokens applied, warnings by kind, what was fetched, fonts loaded.
    fn report_json(&self, src: Option<SourceId>, ui: Option<SourceId>) -> String {
        let text = self.th.preview.then(|| self.th.content.text());
        let Some(css) = text.as_deref().or_else(|| self.themes.active_css(src, ui)) else {
            return serde_json::json!({ "active": false }).to_string();
        };
        let r = self.themes.resolved_for(css);
        let p = self.themes.parse_text(css).ok();
        let mut kinds = std::collections::BTreeMap::new();
        for w in p.iter().flat_map(|p| &p.warnings) {
            *kinds.entry(w.kind.label()).or_insert(0usize) += 1;
        }
        serde_json::json!({
            "active": true,
            "previewing": self.th.preview,
            "fetching": self.th.fetching,
            "applied": p.as_ref().map_or(0, |p| p.theme.count()),
            "not_applied": p.as_ref().map_or(0, |p| p.warnings.len() + p.dropped_warnings),
            "not_applied_by_kind": kinds,
            "imports": {
                "downloaded": r.map_or(0, |r| r.downloaded),
                "from_cache": r.map_or(0, |r| r.from_cache),
                "notes": r.map_or_else(Vec::new, |r| r.notes.iter().map(|n| format!("{}: {}", n.url, n.reason)).collect()),
            },
            "fonts": self.themes.fonts_for(css).iter().map(|f| f.family.clone()).collect::<Vec<_>>(),
            "engine": engine_report(r.map_or(css, |r| r.css.as_str())),
        })
        .to_string()
    }

    /// Apply again on the next message (the fetch setting changed).
    pub(super) fn th_reapply(&mut self) {
        self.th.applied = None;
    }

    /// Keep / Revert, floating over every screen while a theme is previewed.
    pub(super) fn theme_preview_bar(&self) -> El<'_> {
        container(
            container(
                row![
                    text("Previewing a theme").size(t::BODY).font(t::semibold()),
                    action_button(
                        "Keep",
                        true,
                        false,
                        false,
                        false,
                        Message::Themes(Msg::Apply)
                    ),
                    action_button(
                        "Revert",
                        false,
                        false,
                        false,
                        false,
                        Message::Themes(Msg::Revert)
                    ),
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            )
            .padding([t::S3, t::S4])
            .style(style::raised_card),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .padding(t::S6)
        .into()
    }

    pub(super) fn is_previewing(&self) -> bool {
        self.th.preview
    }

    /// The safe key (Ctrl+Shift+0).
    pub(super) fn safe_theme(&mut self) -> Task<Message> {
        self.themes_msg(Msg::Safe)
    }

    fn reparse(&mut self) -> Task<Message> {
        self.reparse_with(false)
    }

    /// The stylesheet the CSS engine layers over its default: what is previewed, else the active
    /// theme, with imports inlined and `@font-face` removed once fetched (the raw text until then).
    #[cfg(feature = "theme-engine")]
    pub(super) fn engine_css(&self) -> Option<String> {
        let raw = if self.studio.open {
            self.studio.css.clone()
        } else if self.th.preview {
            self.th.content.text()
        } else {
            self.themes
                .active_css(self.theme_source(), self.theme_ui())?
                .to_string()
        };
        let css = self
            .themes
            .resolved_for(&raw)
            .map_or(raw, |r| r.css.clone());
        nocturne_style::is_engine_sheet(&css).then_some(css)
    }

    /// The families of the fetched fonts that the engine's active sheet may name.
    #[cfg(feature = "theme-engine")]
    pub(super) fn engine_fonts(&self) -> Vec<String> {
        let raw = if self.th.preview {
            Some(self.th.content.text())
        } else {
            self.themes
                .active_css(self.theme_source(), self.theme_ui())
                .map(str::to_string)
        };
        #[cfg(feature = "theme-engine")]
        let raw = if self.studio.open {
            Some(self.studio.css.clone())
        } else {
            raw
        };
        let mut fams: Vec<String> = raw.map_or_else(Vec::new, |r| {
            self.themes
                .fonts_for(&r)
                .iter()
                .map(|f| f.family.clone())
                .collect()
        });
        // The server web client's icon font, once the text shaper has it.
        if let Some(ui) = self.theme_ui()
            && self.eng.icons_ready.contains(&ui)
            && let Some((fam, _)) = self.server_ui.get(&ui).and_then(|u| u.web_font.as_ref())
        {
            fams.push(fam.clone());
        }
        fams
    }

    /// Translate the editor text, fetching its imports first (from the cache unless `refresh`).
    /// Typing waits a moment so that half-typed addresses are not fetched; an answer for text
    /// that has changed since is dropped by its epoch.
    fn reparse_with(&mut self, refresh: bool) -> Task<Message> {
        self.th.epoch += 1;
        let (epoch, text) = (self.th.epoch, self.th.content.text());
        let remote = self.themes.remote();
        let base = Some(self.th.url.trim().to_string()).filter(|u| !u.is_empty());
        let known = if refresh {
            None
        } else {
            self.themes.resolved_for(&text).cloned()
        };
        self.th.fetching = remote && known.is_none() && import::has_remote(&text);
        run(
            async move {
                if remote && import::has_remote(&text) {
                    let r = match known {
                        Some(r) => r,
                        None => {
                            if !refresh {
                                tokio::time::sleep(Duration::from_millis(400)).await;
                            }
                            import::resolve(&text, base.as_deref(), &Options::new(remote, refresh))
                                .await
                        }
                    };
                    import::parse_resolved(&r)
                        .map(|p| (Arc::new(p), Some(Arc::new(r))))
                        .map_err(|e| e.to_string())
                } else {
                    cz::parse(&text)
                        .map(|p| (Arc::new(p), None))
                        .map_err(|e| e.to_string())
                }
            },
            move |r| Message::Themes(Msg::Parsed(epoch, r)),
        )
    }

    /// Show the editor's theme on the whole app until Keep or Revert.
    fn start_preview(&mut self) -> Task<Message> {
        let Some(p) = self.th.parsed.clone() else {
            return Task::none();
        };
        self.th.safe = false;
        self.th.preview = true;
        let fonts = self
            .th
            .resolved
            .as_ref()
            .map_or_else(Vec::new, |r| r.fonts.clone());
        crate::theme::map_fonts(&fonts);
        crate::theme::set(&p.theme, &p.layout);
        self.load_fonts(&fonts)
    }

    fn end_preview(&mut self) {
        self.th.preview = false;
        self.th.want_preview = false;
        self.th.applied = None;
    }

    pub(super) fn set_text(&mut self, s: &str) -> Task<Message> {
        self.th.content = text_editor::Content::with_text(s);
        self.reparse()
    }

    fn note(&mut self, s: impl Into<String>, err: bool) {
        self.th.note = Some((s.into(), err));
    }

    /// An import fills the editor but never overwrites the selected saved theme: Save adds a new one.
    fn detach_import(&mut self, file: &str) {
        self.th.name = cz::import_name(self.th.sel.is_some(), &self.th.name, file);
        self.th.sel = None;
        self.th.confirm_delete = false;
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
                let pick = self.themes_msg(Msg::Pick(start));
                let gal = if self.th.gal.index.is_none()
                    && self.th.gal.tab != super::themes_gallery::Tab::Mine
                {
                    self.gallery_msg(super::themes_gallery::GMsg::Refresh)
                } else {
                    Task::none()
                };
                return Task::batch([pick, gal]);
            }
            Msg::Close => {
                self.th.open = false;
                self.end_preview();
                self.settings_open = true;
            }
            Msg::Pick(id) => {
                self.end_preview();
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
            Msg::Preset(i) => {
                #[cfg(feature = "theme-engine")]
                let all = cz::PRESETS.iter().chain(nocturne_style::STARTERS);
                #[cfg(not(feature = "theme-engine"))]
                let all = cz::PRESETS.iter();
                let Some((name, css)) = all.into_iter().nth(i) else {
                    return Task::none();
                };
                self.end_preview();
                self.th.confirm_delete = false;
                self.th.sel = None;
                self.th.url.clear();
                self.th.name = format!("{name} copy");
                self.note(
                    "A copy of the built-in example. Edit it, then Preview or Save.",
                    false,
                );
                return self.set_text(css);
            }
            Msg::Builtin(i) => {
                let Some(b) = import::BUILTIN.get(i) else {
                    return Task::none();
                };
                if b.id == "nocturne" {
                    return self.themes_msg(Msg::Reset);
                }
                if b.css.is_empty() {
                    self.note(format!("{} is not available yet.", b.name), false);
                    return Task::none();
                }
                self.end_preview();
                self.th.confirm_delete = false;
                self.th.sel = None;
                self.th.url.clear();
                self.th.name = b.name.to_string();
                self.th.want_preview = true;
                return self.set_text(b.css);
            }
            Msg::Name(s) => self.th.name = s.chars().take(48).collect(),
            Msg::Edit(a) => {
                let edit = a.is_edit();
                // A paste is a theme someone wants to try: show it everywhere until Keep or Revert.
                if matches!(a, text_editor::Action::Edit(text_editor::Edit::Paste(_))) {
                    self.th.want_preview = true;
                }
                self.th.content.perform(a);
                if edit {
                    return self.reparse();
                }
            }
            Msg::Parsed(e, r) if e == self.th.epoch => match r {
                Ok((p, res)) => {
                    self.th.fetching = false;
                    self.th.parsed = Some(p);
                    if let Some(r) = &res {
                        // Share it: the engine and the active theme find it by the same text.
                        self.themes
                            .set_resolved(&self.th.content.text(), (**r).clone());
                    }
                    self.th.resolved = res;
                    self.th.note = None;
                    if self.th.want_preview || self.th.preview {
                        self.th.want_preview = false;
                        return self.start_preview();
                    }
                }
                Err(e) => {
                    self.th.fetching = false;
                    self.th.parsed = None;
                    self.th.resolved = None;
                    self.th.want_preview = false;
                    self.note(e, true);
                }
            },
            Msg::Resolved(css, r) => {
                self.th.resolving.remove(&css);
                let r = Arc::try_unwrap(r).unwrap_or_else(|a| (*a).clone());
                let fonts = r.fonts.clone();
                self.themes.set_resolved(&css, r);
                self.th.applied = None;
                return self.load_fonts(&fonts);
            }
            Msg::Refresh => {
                self.themes.clear_resolved();
                let mut tasks = vec![self.reparse_with(true)];
                let (src, ui) = (self.theme_source(), self.theme_ui());
                if let Some(css) = self.themes.active_css(src, ui).map(str::to_string)
                    && import::has_remote(&css)
                    && self.themes.remote()
                    && self.th.resolving.insert(css.clone())
                {
                    tasks.push(self.resolve_task(css, self.base_of(src, ui), true));
                }
                // Plugin stylesheets: forget the cached copies and read the servers' start pages again.
                #[cfg(feature = "theme-engine")]
                {
                    nocturne_core::webcss::clear_plugin_cache();
                    self.server_ui.clear();
                    self.ui_gen += 1;
                }
                self.th.applied = None;
                return Task::batch(tasks);
            }
            Msg::Safe => {
                self.end_preview();
                self.th.safe = true;
                self.th.applied = None;
                self.toast("Safe look: the built-in theme. Settings > Themes to choose one again.");
            }
            Msg::Report(path) => {
                let (src, ui) = (self.theme_source(), self.theme_ui());
                let json = self.report_json(src, ui);
                let _ = std::fs::write(path, json);
            }
            Msg::LeaveSafe => {
                self.th.safe = false;
                self.th.applied = None;
            }
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
                return run(read_css(p), |r| Message::Themes(Msg::Loaded(r)));
            }
            Msg::Loaded(Ok(s)) => {
                let file = PathBuf::from(self.th.path.trim())
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.detach_import(&file);
                self.th.want_preview = true;
                self.th.url.clear();
                self.note(
                    "Imported. Check the result below, then Preview or Save.",
                    false,
                );
                return self.set_text(&s);
            }
            Msg::Loaded(Err(e)) => self.note(e, true),
            Msg::Url(s) => self.th.url = s,
            Msg::Download if !self.themes.remote() => self.note(OFF_NOTE, true),
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
                        self.detach_import(&file_name_of(&self.th.url));
                        self.th.want_preview = true;
                        self.note(
                            "Downloaded once. Nothing updates on its own; use Refresh to fetch again.",
                            false,
                        );
                        return self.set_text(&s);
                    }
                    Err(e) => self.note(e, true),
                }
            }
            Msg::Preview => return self.start_preview(),
            Msg::Revert => self.end_preview(),
            Msg::Save => {
                self.save_theme();
                self.end_preview();
                self.note("Saved.", false);
            }
            Msg::Apply => {
                let id = self.save_theme();
                self.themes.set_global(Some(id));
                self.themes.save();
                self.th.safe = false;
                self.end_preview();
                self.note("Applied to the whole app.", false);
            }
            Msg::Reset => {
                self.themes.set_global(None);
                self.themes.save();
                self.th.safe = false;
                self.end_preview();
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
            Msg::How => self.th.how ^= true,
            Msg::ReportTab(b) => self.th.brand_tab = b,
            Msg::Gallery(g) => return self.gallery_msg(g),
            Msg::CopyReport => return iced::clipboard::write(self.report_text()),
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
            "Colours, corner radii, fonts and layout from a CSS file.",
            "Esc to go back".into(),
        );
        let how = container(
            column![
                action_button(
                    if self.th.how {
                        "Hide how themes work"
                    } else {
                        "How themes work"
                    },
                    false,
                    false,
                    false,
                    false,
                    Message::Themes(Msg::How)
                ),
                if self.th.how {
                    container(
                        text(HOW)
                            .size(t::BODY)
                            .line_height(1.55)
                            .color(t::text_secondary()),
                    )
                    .max_width(760)
                } else {
                    container(space())
                },
            ]
            .spacing(t::S2),
        )
        .padding([0.0, t::page_pad()]);
        // Narrow windows stack the lists above the editor; a short one scrolls the page and gives the editor and the report fixed heights.
        let narrow = self.win.width < 700.0;
        let short = narrow || self.win.height < 760.0;
        let body: El<'_> = if narrow {
            column![self.theme_list(None, 170.0.into()), self.theme_editor(true)]
                .spacing(t::S4)
                .into()
        } else if short {
            row![
                self.theme_list(Some(260.0), 560.0.into()),
                self.theme_editor(true)
            ]
            .spacing(t::S6)
            .into()
        } else {
            row![
                self.theme_list(Some(300.0), Length::Fill),
                self.theme_editor(false)
            ]
            .spacing(t::S6)
            .height(Length::Fill)
            .into()
        };
        let gallery = self.th.gal.view == super::themes_gallery::View::Gallery;
        let switch = container(self.themes_switch()).padding([0.0, t::page_pad()]);
        if gallery {
            return column![
                container(column![head, how, switch].spacing(t::S3)).padding(Padding {
                    top: t::header_h() + t::S6,
                    ..Padding::ZERO
                }),
                container(self.gallery_page())
                    .padding(Padding {
                        top: t::S4,
                        left: t::page_pad(),
                        right: t::page_pad(),
                        ..Padding::ZERO
                    })
                    .height(Length::Fill),
            ]
            .height(Length::Fill)
            .into();
        }
        let page = column![
            container(column![head, how, switch].spacing(t::S3)).padding(Padding {
                top: t::header_h() + t::S6,
                ..Padding::ZERO
            }),
            container(body)
                .padding(Padding {
                    top: t::S4,
                    bottom: t::S6,
                    left: t::page_pad(),
                    right: t::page_pad(),
                })
                .height(if short { Length::Shrink } else { Length::Fill }),
        ];
        if short {
            scroll(page)
                .style(style::scroll)
                .height(Length::Fill)
                .into()
        } else {
            page.height(Length::Fill).into()
        }
    }

    /// The theme lists (built-in, examples, saved, per server); `size` is the column's width, or its height when the page is stacked.
    fn theme_list(&self, width: Option<f32>, height: Length) -> El<'_> {
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
                if global == Some(s.id) {
                    "ACTIVE"
                } else if s.gallery.is_some() {
                    "GALLERY"
                } else {
                    "NOT FROM GALLERY"
                },
                Msg::Pick(Some(s.id)),
            ));
        }
        let mut builtin = column![].spacing(2);
        for (i, b) in import::BUILTIN.iter().enumerate() {
            let on = b.id == "nocturne" && global.is_none();
            builtin = builtin.push(entry(
                b.name.to_string(),
                on,
                if on {
                    "ACTIVE"
                } else if b.css.is_empty() && b.id != "nocturne" {
                    "SOON"
                } else {
                    "BUILT-IN"
                },
                Msg::Builtin(i),
            ));
        }
        let mut presets = column![].spacing(2);
        #[cfg(feature = "theme-engine")]
        let all = cz::PRESETS.iter().chain(nocturne_style::STARTERS);
        #[cfg(not(feature = "theme-engine"))]
        let all = cz::PRESETS.iter();
        for (i, (name, _)) in all.enumerate() {
            presets = presets.push(entry(
                format!("{name}  \u{b7}  use as starting point"),
                false,
                "EXAMPLE",
                Msg::Preset(i),
            ));
        }
        let mut col = column![
            text(tracked("Built-in"))
                .shaping(text::Shaping::Advanced)
                .size(t::CONTROL_TEXT)
                .font(t::bold())
                .color(t::text_secondary()),
            builtin,
            text(tracked("Examples"))
                .shaping(text::Shaping::Advanced)
                .size(t::CONTROL_TEXT)
                .font(t::bold())
                .color(t::text_secondary()),
            presets,
            text(tracked("Saved themes"))
                .shaping(text::Shaping::Advanced)
                .size(t::CONTROL_TEXT)
                .font(t::bold())
                .color(t::text_secondary()),
            list,
        ]
        .spacing(t::S3);
        if !self.themes.themes.is_empty() {
            col = col.push(
                text("GALLERY: installed from the gallery. NOT FROM GALLERY: added by hand (pasted, imported or downloaded).")
                    .size(t::CAPTION)
                    .color(t::text_tertiary()),
            );
        }
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
            .width(width.map_or(Length::Fill, Length::Fixed))
            .height(height)
            .into()
    }

    fn theme_editor(&self, compact: bool) -> El<'_> {
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
            .height(if compact {
                Length::Fixed(240.0)
            } else {
                Length::FillPortion(5)
            })
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
                    th.busy || th.url.trim().is_empty() || !self.themes.remote(),
                    msg(Msg::Download)
                ),
                action_button(
                    "Refresh imports",
                    false,
                    false,
                    false,
                    !self.themes.remote(),
                    msg(Msg::Refresh)
                ),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        ]
        .spacing(t::S2);
        let import = if self.themes.remote() {
            import
        } else {
            import.push(text(OFF_NOTE).size(t::CAPTION).color(t::text_secondary()))
        };

        let acts = {
            let previewing = th.preview;
            let can = th.parsed.is_some();
            let mut r = row![].spacing(t::S2).align_y(Alignment::Center);
            if previewing {
                r = r.push(
                    text("Previewing on the whole app")
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
                // The Theme Studio edits what is in the editor, over the live app.
                #[cfg(feature = "theme-engine")]
                {
                    r = r.push(action_button(
                        "Open Studio",
                        true,
                        false,
                        false,
                        false,
                        Message::Studio(super::studio::Msg::Open),
                    ));
                }
                if th.safe {
                    r = r.push(action_button(
                        "Leave safe look",
                        false,
                        false,
                        false,
                        false,
                        msg(Msg::LeaveSafe),
                    ));
                }
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
        col = col.push(self.report_panel(compact)).push(acts);
        col.width(Length::Fill).height(Length::Fill).into()
    }

    /// What the engine made of one server's start-page stylesheets and branding (imports inlined, no Jellyfin translation: the classes are native in layout mode), grouped for the Server branding tab.
    #[cfg(feature = "theme-engine")]
    fn brand_report(&self, id: SourceId) -> Option<BrandReport> {
        if !self.eng.enabled {
            return None;
        }
        let read = |css: &str| {
            let text = self
                .themes
                .resolved_for(css)
                .map_or(css, |r| r.css.as_str());
            nocturne_style::Sheet::parse(text)
        };
        let mut rep = nocturne_style::Report::default();
        let mut lines = vec![];
        let mut groups = vec![];
        if let Some(p) = self.server_ui.get(&id).and_then(|u| u.plugins.clone()) {
            if p.loaded > 0 || !p.skipped.is_empty() {
                let sheet = read(&p.css);
                lines.push(format!(
                    "{} stylesheets from the server's web client (plugins), {} rules read by the engine, {} skipped",
                    p.loaded,
                    sheet.rules.len(),
                    p.skipped.len()
                ));
                rep.merge(&sheet.report);
            }
            if p.scripts > 0 {
                lines.push(format!("{} scripts not run (desktop app)", p.scripts));
            }
            if !p.skipped.is_empty() {
                groups.push(("stylesheets skipped".to_string(), p.skipped.clone()));
            }
        }
        if let Some(css) = self.themes.branding_of(id) {
            let sheet = read(css);
            lines.push(format!(
                "Branding CSS: {} rules read by the engine, {} skipped",
                sheet.rules.len(),
                sheet.report.failures()
            ));
            rep.merge(&sheet.report);
        }
        for (k, g) in &rep.groups {
            let mut items = g.examples.clone();
            if g.count > items.len() {
                items.push(format!("and {} more", g.count - items.len()));
            }
            groups.push((format!("{} {}", g.count, k.label()), items));
        }
        (!lines.is_empty() || !groups.is_empty()).then_some(BrandReport { lines, groups })
    }

    #[cfg(not(feature = "theme-engine"))]
    fn brand_report(&self, id: SourceId) -> Option<BrandReport> {
        let css = self.themes.branding_of(id)?;
        let p = self.themes.parse_text(css).ok()?;
        Some(BrandReport {
            lines: vec![format!(
                "{} applied, {} not understood",
                p.theme.count() + p.layout.count(),
                p.warnings.len() + p.dropped_warnings
            )],
            groups: vec![(
                "not understood".into(),
                p.warnings.iter().take(5).map(|w| w.to_string()).collect(),
            )],
        })
    }

    /// The report area: tabs "This theme" and "Server branding" over one scrollable panel.
    fn report_panel(&self, compact: bool) -> El<'_> {
        let on = self.th.brand_tab;
        let tab = |label: &str, brand: bool| {
            action_button(
                label,
                on == brand,
                false,
                false,
                false,
                Message::Themes(Msg::ReportTab(brand)),
            )
        };
        let body: El<'_> = if on {
            self.branding_tab()
        } else {
            self.theme_report()
        };
        column![
            row![
                tab("This theme", false),
                tab("Server branding", true),
                space::horizontal(),
                action_button(
                    "Copy",
                    false,
                    false,
                    false,
                    false,
                    Message::Themes(Msg::CopyReport)
                ),
            ]
            .spacing(t::S2),
            container(
                scroll(container(body).padding(Padding {
                    right: t::S3,
                    ..Padding::ZERO
                }))
                .style(style::scroll)
                .width(Length::Fill)
                .height(Length::Fill)
            )
            .style(style::panel)
            .padding(t::S3)
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(t::S2)
        .height(if compact {
            Length::Fixed(260.0)
        } else {
            Length::FillPortion(4)
        })
        .into()
    }

    /// Per server: what its start page and administrator added to its look, grouped by kind with one item per line.
    fn branding_tab(&self) -> El<'_> {
        let mut col = column![
            text("The look a server's own web client and administrator set, used while that server's UI is chosen in Settings > Sources. A theme you pick for the server wins over it.")
                .size(t::CAPTION)
                .color(t::text_tertiary())
        ]
        .spacing(t::S2);
        let mut any = false;
        for s in &self.catalog.slots {
            let Some(r) = self.brand_report(s.id) else {
                continue;
            };
            any = true;
            col = col.push(rule()).push(
                text(s.profile.label.clone())
                    .size(t::CONTROL_TEXT)
                    .font(t::semibold()),
            );
            for l in r.lines {
                col = col.push(text(l).size(t::CAPTION).color(t::text()));
            }
            for (h, items) in r.groups {
                col = col.push(
                    text(tracked(&h))
                        .shaping(text::Shaping::Advanced)
                        .size(t::CAPTION)
                        .font(t::bold())
                        .color(t::text_secondary()),
                );
                for i in items {
                    col = col.push(
                        row![
                            text("\u{b7}").size(t::CAPTION).color(t::text_tertiary()),
                            text(i)
                                .font(t::MONO)
                                .size(t::CAPTION)
                                .color(t::text_secondary())
                                .width(Length::Fill),
                        ]
                        .spacing(t::S2),
                    );
                }
            }
        }
        if !any {
            col = col.push(
                text("No server has branding or plugin stylesheets to report.")
                    .size(t::CAPTION)
                    .color(t::text_secondary()),
            );
        }
        col.width(Length::Fill).into()
    }

    /// What the engine skipped in the text being edited, when the engine reads it (the marker or its vocabulary) and is running.
    #[cfg(feature = "theme-engine")]
    fn engine_report(&self) -> Option<(usize, Vec<String>)> {
        let raw = self.th.content.text();
        let css = self
            .themes
            .resolved_for(&raw)
            .map_or(raw, |r| r.css.clone());
        if !self.eng.enabled || !nocturne_style::is_engine_sheet(&css) {
            return None;
        }
        let (text, _) = nocturne_style::compat::translate(&css);
        let rep = nocturne_style::Sheet::parse(&text).report;
        Some((rep.failures(), rep.lines()))
    }

    #[cfg(not(feature = "theme-engine"))]
    fn engine_report(&self) -> Option<(usize, Vec<String>)> {
        None
    }

    /// "N settings applied", the counts per kind, the contrast notes and the list with positions.
    /// The "This theme" report as plain lines: drawn by [`Self::theme_report`] and copied by the Copy button.
    fn theme_report_lines(&self) -> Vec<RLine> {
        let fetching = RLine::new("Fetching imports and fonts...", Tone::Secondary);
        let Some(p) = &self.th.parsed else {
            return vec![if self.th.fetching {
                fetching
            } else {
                RLine::new(
                    "Paste, type or import a theme to see what the app understood.",
                    Tone::Tertiary,
                )
            }];
        };
        let mut lines = vec![];
        if self.th.fetching {
            lines.push(fetching);
        }
        // An engine sheet is read by the engine, not the stage-1 translator: report what the engine skipped.
        let engine = self.engine_report();
        let mut head = format!("{} settings applied.", p.theme.count() + p.layout.count());
        let mut kinds: Vec<(cz::WarningKind, usize)> = vec![];
        for w in p.warnings.iter().filter(|_| engine.is_none()) {
            match kinds.iter_mut().find(|(k, _)| *k == w.kind) {
                Some((_, n)) => *n += 1,
                None => kinds.push((w.kind, 1)),
            }
        }
        for (k, n) in &kinds {
            head.push_str(&format!(" {n} {}.", k.label()));
        }
        if p.dropped_warnings > 0 && engine.is_none() {
            head.push_str(&format!(" {} more not listed.", p.dropped_warnings));
        }
        if let Some((failures, _)) = &engine {
            head = "Read by the theme engine.".to_owned();
            if *failures > 0 {
                head.push_str(&format!(" {failures} skipped."));
            }
        }
        lines.push(RLine::new(head, Tone::Primary));
        for l in engine.iter().flat_map(|r| r.1.iter().cloned()) {
            lines.push(RLine::new(l, Tone::Secondary));
        }
        if engine.is_none() {
            for a in crate::theme::advisories(&crate::theme::Tokens::resolve(&p.theme)) {
                lines.push(RLine::new(a, Tone::Warn));
            }
        }
        if let Some(r) = &self.th.resolved {
            lines.push(RLine::new(fetch_summary(r), Tone::Secondary));
            for f in r.fonts.iter().take(6) {
                lines.push(RLine::new(
                    format!(
                        "Font {} {}, loaded{}",
                        f.family,
                        f.weight,
                        Options::new(true, false)
                            .fetched_at(&f.url)
                            .map_or(String::new(), |at| format!(", fetched {}", ago(at)))
                    ),
                    Tone::Secondary,
                ));
            }
        } else if !self.themes.remote() && import::has_remote(&self.th.content.text()) {
            lines.push(RLine::new(
                "Imports and fonts are not fetched: Settings > Themes > Fetch theme imports is off.",
                Tone::Secondary,
            ));
        }
        let loaded = |m: &str| {
            self.th.resolved.as_ref().is_some_and(|r| {
                r.fonts
                    .iter()
                    .any(|f| m.contains(&format!("`{}`", f.family)))
            })
        };
        for w in p
            .warnings
            .iter()
            .filter(|_| engine.is_none())
            .filter(|w| !(w.kind == cz::WarningKind::FontNotDownloaded && loaded(&w.message)))
            .take(SHOWN)
        {
            let tone = if w.kind.informational() {
                Tone::Tertiary
            } else {
                Tone::Secondary
            };
            lines.push(RLine {
                pos: Some(format!("{}:{}", w.line, w.col)),
                text: w.message.clone(),
                tone,
            });
        }
        lines
    }

    fn theme_report(&self) -> El<'_> {
        let lines: Vec<El<'_>> = self
            .theme_report_lines()
            .into_iter()
            .map(|l| {
                let c = match l.tone {
                    Tone::Primary => t::text(),
                    Tone::Secondary => t::text_secondary(),
                    Tone::Tertiary => t::text_tertiary(),
                    Tone::Warn => t::warn(),
                };
                let body = text(l.text).size(t::CAPTION).color(c);
                match l.pos {
                    Some(pos) => row![mono(pos).width(56), body].spacing(t::S2).into(),
                    None => body.into(),
                }
            })
            .collect();
        column(lines).spacing(2).into()
    }

    /// The open report tab as plain text, for the Copy button.
    fn report_text(&self) -> String {
        let mut out = String::new();
        if self.th.brand_tab {
            out.push_str("Server branding\n");
            let mut any = false;
            for s in &self.catalog.slots {
                let Some(r) = self.brand_report(s.id) else {
                    continue;
                };
                any = true;
                out.push_str(&format!("\n{}\n", s.profile.label));
                for l in r.lines {
                    out.push_str(&format!("{l}\n"));
                }
                for (h, items) in r.groups {
                    out.push_str(&format!("{h}\n"));
                    for i in items {
                        out.push_str(&format!("  - {i}\n"));
                    }
                }
            }
            if !any {
                out.push_str("\nNo server has branding or plugin stylesheets to report.\n");
            }
        } else {
            out.push_str("This theme\n\n");
            for l in self.theme_report_lines() {
                match l.pos {
                    Some(p) => out.push_str(&format!("{p}  {}\n", l.text)),
                    None => out.push_str(&format!("{}\n", l.text)),
                }
            }
        }
        out
    }
}

/// One line of the "This theme" report.
struct RLine {
    /// `line:col` of a warning in the edited text.
    pos: Option<String>,
    text: String,
    tone: Tone,
}

impl RLine {
    fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            pos: None,
            text: text.into(),
            tone,
        }
    }
}

#[derive(Clone, Copy)]
enum Tone {
    Primary,
    Secondary,
    Tertiary,
    Warn,
}

/// One server's branding and plugin-stylesheet report: summary lines, then groups of (heading, items).
struct BrandReport {
    lines: Vec<String>,
    groups: Vec<(String, Vec<String>)>,
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

/// "3 files downloaded, 2 from the cache" for the report line.
fn fetch_summary(r: &Resolved) -> String {
    format!(
        "Fetched {} {}, {} from the cache; {} {} loaded.",
        r.downloaded,
        if r.downloaded == 1 { "file" } else { "files" },
        r.from_cache,
        r.fonts.len(),
        if r.fonts.len() == 1 { "font" } else { "fonts" },
    )
}

/// "5 min ago" for a Unix time.
pub(super) fn ago(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let d = now.saturating_sub(at);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        _ => format!("{} days ago", d / 86400),
    }
}

/// The theme engine's numbers for the report (null in a build without the engine).
#[cfg(feature = "theme-engine")]
fn engine_report(css: &str) -> serde_json::Value {
    super::engine_ui::theme_report(css)
}

#[cfg(not(feature = "theme-engine"))]
fn engine_report(_: &str) -> serde_json::Value {
    serde_json::Value::Null
}
