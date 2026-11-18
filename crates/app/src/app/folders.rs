//! The local folders screen: the folders of the local library with availability, add (the system
//! file chooser through the desktop portal, or a typed path), remove, rescan with live progress,
//! counts and the list of unmatched files with manual matching. Nothing here writes to, renames or
//! deletes a media file: edits go to the index data under the app's own data directory.
use super::message::Message;
use super::{App, Phase};
use crate::style::{self, border};
use crate::task::run;
use crate::theme as t;
use crate::widget::{
    action_button, dot, icon, mono, pill_button, progress_bar, rule, section, tab,
};
use iced::widget::{
    Id, column, container, operation, row, scrollable as scroll, space, text, text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::catalog::{Env, Slot};
use nocturne_core::folders::{MatchEdit, Overview, UNMATCHED_SHOWN};
use nocturne_core::profile::ProfileKind;
use nocturne_core::source::SourceId;
use nocturne_core::sources::status_text;
use nocturne_local::Kind;
use std::path::{Path, PathBuf};
use std::time::Instant;

type El<'a> = Element<'a, Message>;

const PATH_ID: &str = "folders-path";
const EDIT_ID: &str = "folders-edit-title";

/// The form for one unmatched file.
pub struct Edit {
    pub id: u64,
    pub file: String,
    pub kind: Kind,
    pub title: String,
    pub year: String,
    pub season: String,
    pub episode: String,
    pub err: Option<&'static str>,
}

pub struct Folders {
    pub id: SourceId,
    /// First-run: a Continue / Back footer instead of the usual way out.
    pub setup: bool,
    pub overview: Overview,
    pub path: String,
    pub path_err: Option<&'static str>,
    pub edit: Option<Edit>,
}

impl Folders {
    pub fn new(id: SourceId, setup: bool) -> Self {
        Self {
            id,
            setup,
            overview: Overview::default(),
            path: String::new(),
            path_err: None,
            edit: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Path(String),
    AddTyped,
    Choose,
    Picked(Option<PathBuf>),
    Validated(PathBuf, Result<(), &'static str>),
    Remove(PathBuf),
    Rescan,
    Stop,
    Overview(SourceId, Overview),
    Match(u64),
    EditKind(Kind),
    Title(String),
    Year(String),
    Season(String),
    Episode(String),
    Save,
    Saved(bool),
    CloseEdit,
    Continue,
    Back,
    Close,
}

/// `~/x` and `~` to the home directory; anything else as typed (it must then be absolute).
fn expand(raw: &str) -> PathBuf {
    let raw = raw.trim();
    match (
        raw.strip_prefix("~/"),
        raw == "~",
        nocturne_session::paths::home(),
    ) {
        (Some(rest), _, Some(h)) => h.join(rest),
        (_, true, Some(h)) => h,
        _ => PathBuf::from(raw),
    }
}

/// Can this be added as a media folder? (blocks on the file system: run it off the UI thread.)
fn check_folder(p: &Path) -> Result<(), &'static str> {
    if !p.is_absolute() {
        return Err("Type the full path, starting with / or ~/.");
    }
    if !p.is_dir() {
        return Err("That is not a folder.");
    }
    if std::fs::read_dir(p).is_err() {
        return Err("That folder can't be read. Check its permissions.");
    }
    Ok(())
}

impl App {
    /// The local library's profile and slot, created when it does not exist yet.
    pub(super) fn ensure_local_slot(&mut self) -> SourceId {
        let id = self.sources.ensure_local();
        if self.catalog.slot(id).is_none() {
            let none = |_: &str, _: &str| None;
            let slot = Slot::build(
                self.sources.get(id).expect("just ensured"),
                &Env {
                    token: &none,
                    data_dir: &self.data,
                    disk: None,
                },
            );
            self.catalog.upsert(slot);
            self.catalog.reorder(&self.sources.ids());
            self.save_sources();
        }
        id
    }

    pub(super) fn open_folders(&mut self, id: Option<SourceId>) -> Task<Message> {
        let id = id.unwrap_or_else(|| self.ensure_local_slot());
        self.folders = Some(Folders::new(id, false));
        Task::batch([
            operation::focus(Id::from(PATH_ID)),
            self.refresh_folders(),
            self.load_secrets(),
        ])
    }

    /// First run, local choice: the folders screen without the app around it.
    pub(super) fn open_folders_setup(&mut self) -> Task<Message> {
        self.account_back = None;
        let id = self.ensure_local_slot();
        self.folders = Some(Folders::new(id, true));
        self.phase = Phase::Ready;
        Task::batch([operation::focus(Id::from(PATH_ID)), self.refresh_folders()])
    }

    /// The slot keeps its own copy of the profile; bring it up to date after the folder list changed.
    pub(super) fn sync_profile(&mut self, id: SourceId) {
        if let (Some(p), Some(s)) = (self.sources.get(id).cloned(), self.catalog.slot_mut(id)) {
            s.profile = p;
        }
    }

    pub(super) fn folders_key(&mut self, key: &str) -> Task<Message> {
        match key {
            "escape" => {
                if let Some(f) = self.folders.as_mut() {
                    if f.edit.is_some() {
                        f.edit = None;
                    } else if !f.setup {
                        self.folders = None;
                    }
                }
            }
            "tab" => return operation::focus_next(),
            _ => {}
        }
        Task::none()
    }

    pub(super) fn folder_msg(&mut self, m: Msg) -> Task<Message> {
        let Some(f) = self.folders.as_mut() else {
            return Task::none();
        };
        let id = f.id;
        match m {
            Msg::Path(p) => {
                f.path = p;
                f.path_err = None;
            }
            Msg::AddTyped => {
                let p = expand(&f.path);
                if f.path.trim().is_empty() {
                    return Task::none();
                }
                return validate(p);
            }
            Msg::Choose => {
                // The desktop's own folder chooser (xdg-desktop-portal); nothing if there is none.
                return run(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose a media folder")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |p| Message::Folder(Msg::Picked(p)),
                );
            }
            Msg::Picked(Some(p)) => return validate(p),
            Msg::Picked(None) => {}
            Msg::Validated(p, Err(e)) => {
                f.path_err = Some(e);
                f.path = p.display().to_string();
            }
            Msg::Validated(p, Ok(())) => {
                f.path.clear();
                f.path_err = None;
                if !self.sources.add_root(p) {
                    if let Some(f) = self.folders.as_mut() {
                        f.path_err = Some("That folder is already added.");
                    }
                    return Task::none();
                }
                self.save_sources();
                self.sync_profile(id);
                return self.rescan_now(id);
            }
            Msg::Remove(p) => {
                if self.sources.remove_root(&p) {
                    self.save_sources();
                    self.sync_profile(id);
                    if let Some(f) = self.folders.as_mut() {
                        f.overview.folders.retain(|x| x.path != p);
                    }
                    return self.rescan_now(id);
                }
            }
            Msg::Rescan => return self.rescan_now(id),
            Msg::Stop => self.cancel_scan(id),
            Msg::Overview(o_id, o) => {
                if o_id == id {
                    f.overview = o;
                }
            }
            Msg::Match(file) => {
                let file_name = f
                    .overview
                    .unmatched
                    .iter()
                    .find(|u| u.id == file)
                    .map(|u| u.file.clone())
                    .unwrap_or_default();
                f.edit = Some(Edit {
                    id: file,
                    file: file_name,
                    kind: Kind::Movie,
                    title: String::new(),
                    year: String::new(),
                    season: String::new(),
                    episode: String::new(),
                    err: None,
                });
                return operation::focus(Id::from(EDIT_ID));
            }
            Msg::EditKind(k) => {
                if let Some(e) = f.edit.as_mut() {
                    e.kind = k;
                    e.err = None;
                }
            }
            Msg::Title(v) => self.edit_field(|e| e.title = v),
            Msg::Year(v) => self.edit_field(|e| e.year = v),
            Msg::Season(v) => self.edit_field(|e| e.season = v),
            Msg::Episode(v) => self.edit_field(|e| e.episode = v),
            Msg::Save => {
                let Some(e) = f.edit.as_mut() else {
                    return Task::none();
                };
                let parsed = MatchEdit::parse(e.kind, &e.title, &e.year, &e.season, &e.episode);
                let edit = match parsed {
                    Ok(x) => x,
                    Err(why) => {
                        e.err = Some(why);
                        return Task::none();
                    }
                };
                let file = e.id;
                let Some(l) = self.catalog.backend(id).and_then(|b| b.local()).cloned() else {
                    return Task::none();
                };
                return run(
                    async move {
                        tokio::task::spawn_blocking(move || l.set_match(file, &edit).is_ok())
                            .await
                            .unwrap_or(false)
                    },
                    |ok| Message::Folder(Msg::Saved(ok)),
                );
            }
            Msg::Saved(ok) => {
                if ok {
                    f.edit = None;
                    return Task::batch([self.refresh_folders(), self.retry_page()]);
                } else if let Some(e) = f.edit.as_mut() {
                    e.err = Some("Couldn't save that.");
                }
            }
            Msg::CloseEdit => f.edit = None,
            Msg::Continue => {
                self.folders = None;
                self.setup = None;
                return self.ready();
            }
            // First run, local choice: back to the welcome screen with nothing left behind.
            Msg::Back => return self.remove_source(id),
            Msg::Close => self.folders = None,
        }
        Task::none()
    }

    fn edit_field(&mut self, set: impl FnOnce(&mut Edit)) {
        if let Some(e) = self.folders.as_mut().and_then(|f| f.edit.as_mut()) {
            set(e);
            e.err = None;
        }
    }

    // ---- view ----

    pub(super) fn folders_page(&self) -> El<'_> {
        let Some(f) = &self.folders else {
            return space().into();
        };
        let (title, body) = if f.setup {
            (
                "Add your folders",
                "Choose the folders your movies, shows and music are in. NOCTURNE reads them and never renames, moves or changes a file.",
            )
        } else {
            (
                "Local folders",
                "NOCTURNE indexes these folders and keeps its own notes about them. Your files are never renamed, moved or changed.",
            )
        };
        let mut page = column![section(
            title,
            body,
            if f.setup {
                String::new()
            } else {
                "Esc to go back".into()
            }
        )];

        // add
        let field = text_input("/path/to/your/folder", &f.path)
            .id(Id::from(PATH_ID))
            .on_input(|v| Message::Folder(Msg::Path(v)))
            .on_submit(Message::Folder(Msg::AddTyped))
            .size(t::BODY)
            .padding([t::S2, t::S3])
            .style(style::input);
        let mut add = column![
            row![
                field,
                action_button(
                    "Choose\u{2026}",
                    false,
                    false,
                    false,
                    false,
                    Message::Folder(Msg::Choose)
                ),
                action_button(
                    "Add",
                    true,
                    false,
                    false,
                    f.path.trim().is_empty(),
                    Message::Folder(Msg::AddTyped)
                ),
            ]
            .spacing(t::S2)
            .align_y(Alignment::Center),
        ]
        .spacing(t::S2);
        if let Some(e) = f.path_err {
            add = add.push(text(e).size(t::CONTROL_TEXT).color(t::DANGER));
        }
        page = page.push(container(add).padding(Padding {
            top: t::S6,
            bottom: t::S4,
            left: t::PAGE_PAD,
            right: t::PAGE_PAD,
        }));
        page = page.push(container(rule()).padding([0.0, t::PAGE_PAD]));

        // folders
        let roots: Vec<PathBuf> = match self.sources.get(f.id).map(|p| &p.kind) {
            Some(ProfileKind::Local { roots }) => roots.clone(),
            _ => vec![],
        };
        if roots.is_empty() {
            page = page.push(
                container(
                    text("No folders yet. Choose one above or type its path.")
                        .size(t::BODY)
                        .color(t::TEXT_SECONDARY),
                )
                .padding([t::S6, t::PAGE_PAD]),
            );
        }
        for r in &roots {
            let info = f.overview.folders.iter().find(|x| &x.path == r);
            let (ok, items) = info.map_or((true, 0), |i| (i.available, i.items));
            let state = if ok {
                row![
                    dot(t::ACCENT_TEXT),
                    text(format!("{items} files"))
                        .size(t::CONTROL_TEXT)
                        .color(t::TEXT_SECONDARY)
                ]
            } else {
                row![
                    dot(t::WARN),
                    text("Not available. Is the drive mounted?")
                        .size(t::CONTROL_TEXT)
                        .color(t::WARN)
                ]
            }
            .spacing(t::S2)
            .align_y(Alignment::Center);
            let line = row![
                container(icon("folder", 18.0, t::TEXT_SECONDARY))
                    .center(38)
                    .style(|_| container::Style {
                        background: Some(t::GLASS.into()),
                        border: border(t::HAIRLINE_STRONG, 1.0, 19.0),
                        ..Default::default()
                    }),
                text(r.display().to_string())
                    .size(t::BODY)
                    .width(Length::Fill),
                state,
                action_button(
                    "Remove",
                    false,
                    true,
                    false,
                    false,
                    Message::Folder(Msg::Remove(r.clone()))
                ),
            ]
            .spacing(t::S4)
            .align_y(Alignment::Center)
            .padding([t::S3, t::S6]);
            page = page.push(container(column![line, rule()]).padding([0.0, t::PAGE_PAD]));
        }

        // indexing + counts
        let scanning = self.scans.get(&f.id);
        let status = self
            .catalog
            .slot(f.id)
            .map_or_else(String::new, |s| status_text(&s.status, Instant::now()));
        let mut idx = column![].spacing(t::S3);
        match scanning {
            Some(sc) => {
                let frac = if sc.progress.total > 0 {
                    sc.progress.done as f32 / sc.progress.total as f32
                } else {
                    0.0
                };
                idx = idx.push(
                    row![
                        text(status.clone()).size(t::BODY),
                        space().width(Length::Fill),
                        action_button(
                            "Stop",
                            false,
                            false,
                            false,
                            false,
                            Message::Folder(Msg::Stop)
                        ),
                    ]
                    .align_y(Alignment::Center),
                );
                idx = idx.push(container(progress_bar(frac, 4.0)).max_width(520));
            }
            None => {
                idx = idx.push(
                    row![
                        text(if roots.is_empty() {
                            String::new()
                        } else {
                            "Up to date.".into()
                        })
                        .size(t::BODY)
                        .color(t::TEXT_SECONDARY),
                        space().width(Length::Fill),
                        action_button(
                            "Rescan now",
                            false,
                            false,
                            false,
                            roots.is_empty(),
                            Message::Folder(Msg::Rescan)
                        ),
                    ]
                    .align_y(Alignment::Center),
                );
            }
        }
        let c = &f.overview.counts;
        let stat = |n: usize, l: &'static str| -> El<'static> {
            column![text(n.to_string()).size(t::HEADING).font(t::BOLD), mono(l)]
                .spacing(2)
                .width(110)
                .into()
        };
        idx = idx.push(
            row![
                stat(c.movies, "MOVIES"),
                stat(c.shows, "SHOWS"),
                stat(c.episodes, "EPISODES"),
                stat(c.music, "MUSIC"),
                stat(c.unmatched, "UNMATCHED"),
            ]
            .spacing(t::S4),
        );
        page = page.push(container(idx).padding([t::S6, t::PAGE_PAD]));
        if !f.setup {
            page = page.push(self.online_section(f.id, &f.overview.pending));
        }

        // unmatched
        if c.unmatched > 0 {
            page = page.push(container(rule()).padding([0.0, t::PAGE_PAD]));
            page = page.push(
                container(
                    column![
                        text("Unmatched").size(t::BODY_LG).font(t::BOLD),
                        text("Files NOCTURNE could not place from their names. Tell it what each one is; the file itself is never changed.")
                            .size(t::CONTROL_TEXT)
                            .color(t::TEXT_SECONDARY),
                    ]
                    .spacing(2),
                )
                .padding([t::S6, t::PAGE_PAD]),
            );
            for u in &f.overview.unmatched {
                let open = f.edit.as_ref().is_some_and(|e| e.id == u.id);
                let line = row![
                    column![
                        text(u.file.clone()).size(t::BODY),
                        mono(if u.dir.is_empty() {
                            "(top of the folder)".to_string()
                        } else {
                            u.dir.clone()
                        }),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    action_button(
                        if open { "Cancel" } else { "Match\u{2026}" },
                        false,
                        false,
                        false,
                        false,
                        Message::Folder(if open {
                            Msg::CloseEdit
                        } else {
                            Msg::Match(u.id)
                        }),
                    ),
                ]
                .align_y(Alignment::Center)
                .padding([t::S2, t::S6]);
                let mut cell = column![line].spacing(t::S2);
                if open && let Some(e) = &f.edit {
                    cell = cell.push(edit_form(e));
                }
                page = page.push(container(column![cell, rule()]).padding([0.0, t::PAGE_PAD]));
            }
            if c.unmatched > UNMATCHED_SHOWN {
                page = page.push(
                    container(
                        text(format!(
                            "{} more appear as these are sorted out.",
                            c.unmatched - UNMATCHED_SHOWN
                        ))
                        .size(t::CONTROL_TEXT)
                        .color(t::TEXT_TERTIARY),
                    )
                    .padding([t::S4, t::PAGE_PAD]),
                );
            }
        }

        // first-run footer
        if f.setup {
            let both = matches!(self.setup, Some(nocturne_core::sources::FirstRun::Both));
            let mut foot = row![].spacing(t::S3);
            if !both {
                foot = foot.push(
                    container(pill_button(
                        "Back",
                        false,
                        false,
                        Message::Folder(Msg::Back),
                    ))
                    .width(120),
                );
            }
            foot = foot.push(
                container(pill_button(
                    if roots.is_empty() && both {
                        "Skip for now"
                    } else {
                        "Continue"
                    },
                    roots.is_empty() && both || !roots.is_empty(),
                    false,
                    if roots.is_empty() && !both {
                        Message::Noop
                    } else {
                        Message::Folder(Msg::Continue)
                    },
                ))
                .width(180),
            );
            page = page.push(container(foot).padding([t::S6, t::PAGE_PAD]));
        } else {
            page = page.push(
                container(action_button(
                    "Done",
                    true,
                    false,
                    false,
                    false,
                    Message::Folder(Msg::Close),
                ))
                .padding([t::S4, t::PAGE_PAD]),
            );
        }
        scroll(page.padding(Padding {
            top: if f.setup { t::S8 } else { t::HEADER_H + t::S8 },
            bottom: t::S8,
            ..Padding::ZERO
        }))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

/// Check the folder off the UI thread, then `Validated`.
fn validate(p: PathBuf) -> Task<Message> {
    run(
        async move {
            let q = p.clone();
            let r = tokio::task::spawn_blocking(move || check_folder(&q))
                .await
                .unwrap_or(Err("That folder can't be read."));
            (p, r)
        },
        |(p, r)| Message::Folder(Msg::Validated(p, r)),
    )
}

fn edit_form(e: &Edit) -> El<'_> {
    let input = |ph: &str, v: &str, w: f32, f: fn(String) -> Msg| {
        text_input(ph, v)
            .on_input(move |s| Message::Folder(f(s)))
            .on_submit(Message::Folder(Msg::Save))
            .size(t::BODY)
            .padding([t::S2, t::S3])
            .width(w)
            .style(style::input)
    };
    let kinds = row![
        tab(
            "Movie",
            e.kind == Kind::Movie,
            Message::Folder(Msg::EditKind(Kind::Movie))
        ),
        tab(
            "Episode",
            e.kind == Kind::Episode,
            Message::Folder(Msg::EditKind(Kind::Episode))
        ),
        tab(
            "Music",
            e.kind == Kind::Track,
            Message::Folder(Msg::EditKind(Kind::Track))
        ),
    ]
    .spacing(t::S4);
    let title = input(
        if e.kind == Kind::Episode {
            "Show name"
        } else {
            "Title"
        },
        &e.title,
        320.0,
        Msg::Title,
    )
    .id(Id::from(EDIT_ID));
    let mut fields = row![title].spacing(t::S2).align_y(Alignment::Center);
    if e.kind != Kind::Track {
        fields = fields.push(input("Year", &e.year, 90.0, Msg::Year));
    }
    if e.kind == Kind::Episode {
        fields = fields.push(input("Season", &e.season, 90.0, Msg::Season));
        fields = fields.push(input("Episode", &e.episode, 90.0, Msg::Episode));
    }
    let mut col = column![
        kinds,
        fields,
        row![
            action_button(
                "Save match",
                true,
                false,
                false,
                false,
                Message::Folder(Msg::Save)
            ),
            text(e.err.unwrap_or(""))
                .size(t::CONTROL_TEXT)
                .color(t::DANGER),
        ]
        .spacing(t::S3)
        .align_y(Alignment::Center),
    ]
    .spacing(t::S3);
    col = col.padding(Padding {
        top: 0.0,
        bottom: t::S3,
        left: t::S6,
        right: t::S6,
    });
    col.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_relative_paths_expand_and_others_must_be_absolute() {
        let home = nocturne_session::paths::home().unwrap();
        assert_eq!(expand(" ~/Movies "), home.join("Movies"));
        assert_eq!(expand("~"), home);
        assert_eq!(expand("/mnt/x"), PathBuf::from("/mnt/x"));
        assert!(check_folder(Path::new("movies")).is_err());
        assert!(check_folder(Path::new("/definitely/not/here")).is_err());
        let d = tempfile::tempdir().unwrap();
        assert!(check_folder(d.path()).is_ok());
        let f = d.path().join("a.mkv");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(check_folder(&f), Err("That is not a folder."));
    }
}
