//! The themes gallery on the Themes page: tabs Official / Community / My themes, and cards with a preview, the name, the author and licence, the description and Install / Installed / Update. Downloads, the checksum and the caching live in `nocturne_core::customize::import::gallery`; an installed theme is a normal saved theme tagged with its gallery id (`Saved::gallery`), so Update replaces it and themes added by hand stay untouched.
use super::App;
use super::message::Message;
use super::themes::{Msg, ago};
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, mono};
use iced::widget::image::Handle;
use iced::widget::{column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::customize::import;
use nocturne_core::customize::import::gallery::{self, Entry, Loaded};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const SUBMIT_URL: &str = "https://github.com/ifsvivek/nocturne/blob/main/themes/README.md";
/// Narrowest a gallery card gets: 4 per row at 1920 px, 2 at 1280, 1 at 900.
const CARD_MIN: f32 = 430.0;

type El<'a> = Element<'a, Message>;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Official,
    Community,
    Mine,
}

/// The page's two views; the choice lasts for the session.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Gallery,
    Editor,
}

#[derive(Clone)]
pub enum GMsg {
    Tab(Tab),
    View(View),
    Search(String),
    /// Open the editor on a saved theme.
    Edit(u32),
    /// Make a saved theme the global theme.
    ApplySaved(u32),
    /// Apply a built-in theme by its index in `import::BUILTIN`.
    ApplyBuiltin(usize),
    /// Open the "how to submit a theme" page.
    Submit,
    /// Read the index again (and the previews).
    Refresh,
    /// The index, and whether it was a manual refresh.
    Index(bool, Result<Arc<Loaded>, String>),
    Preview(String, Option<Handle>),
    Install(String),
    Installed(String, Result<String, String>),
    /// Make an installed gallery theme the global theme.
    Apply(String),
}

#[derive(Default)]
pub struct Gallery {
    pub tab: Tab,
    pub view: View,
    query: String,
    pub(super) index: Option<Arc<Loaded>>,
    loading: bool,
    err: Option<String>,
    previews: HashMap<String, Handle>,
    /// Previews already asked for, so each is requested once.
    asked: HashSet<String>,
    busy: HashSet<String>,
    note: Option<(String, bool)>,
}

impl App {
    pub(super) fn gallery_msg(&mut self, m: GMsg) -> Task<Message> {
        let wrap = |m: GMsg| Message::Themes(Msg::Gallery(m));
        match m {
            GMsg::Tab(tab) => {
                self.th.gal.tab = tab;
                if tab != Tab::Mine && self.th.gal.index.is_none() && !self.th.gal.loading {
                    return self.gallery_msg(GMsg::Refresh);
                }
            }
            GMsg::View(v) => self.th.gal.view = v,
            GMsg::Search(q) => self.th.gal.query = q,
            GMsg::Edit(id) => {
                self.th.gal.view = View::Editor;
                return self.themes_msg(Msg::Pick(Some(id)));
            }
            GMsg::ApplySaved(id) => {
                self.themes.set_global(Some(id));
                self.themes.save();
                self.th.safe = false;
                self.th.applied = None;
                self.th.gal.note = Some(("Applied to the whole app.".into(), false));
            }
            GMsg::ApplyBuiltin(i) => {
                let Some(b) = import::BUILTIN.get(i) else {
                    return Task::none();
                };
                if b.css.is_empty() {
                    self.themes.set_global(None);
                } else {
                    let id = match self.themes.themes.iter().find(|s| s.css == b.css) {
                        Some(s) => s.id,
                        None => self.themes.add(b.name, b.css, None),
                    };
                    self.themes.set_global(Some(id));
                }
                self.themes.save();
                self.th.safe = false;
                self.th.applied = None;
                self.th.gal.note = Some((format!("{} is applied.", b.name), false));
            }
            GMsg::Submit => {
                let _ = std::process::Command::new("xdg-open")
                    .arg(SUBMIT_URL)
                    .spawn();
            }
            GMsg::Refresh => {
                let refresh = self.th.gal.index.is_some();
                let remote = self.themes.remote();
                if !remote {
                    return Task::none();
                }
                self.th.gal.loading = true;
                self.th.gal.err = None;
                let (url, dev) = gallery::index_url();
                return run(
                    async move {
                        gallery::load_index(&url, refresh, remote, dev, true)
                            .await
                            .map(Arc::new)
                    },
                    move |r| wrap(GMsg::Index(refresh, r)),
                );
            }
            GMsg::Index(refresh, r) => {
                self.th.gal.loading = false;
                match r {
                    Ok(l) => {
                        let (url, dev) = gallery::index_url();
                        let remote = self.themes.remote();
                        let mut tasks = vec![];
                        for e in &l.entries {
                            if !self.th.gal.asked.insert(e.id.clone()) && !refresh {
                                continue;
                            }
                            let (url, e) = (url.clone(), e.clone());
                            let id = e.id.clone();
                            tasks.push(run(
                                async move {
                                    gallery::fetch_preview(&url, &e, refresh, remote, dev, true)
                                        .await
                                        .ok()
                                        .map(Handle::from_bytes)
                                },
                                move |h| wrap(GMsg::Preview(id, h)),
                            ));
                        }
                        self.th.gal.index = Some(l);
                        return Task::batch(tasks);
                    }
                    Err(e) => self.th.gal.err = Some(e),
                }
            }
            GMsg::Preview(id, h) => {
                if let Some(h) = h {
                    self.th.gal.previews.insert(id, h);
                }
            }
            GMsg::Install(id) => {
                let Some(e) = self.gallery_entry(&id) else {
                    return Task::none();
                };
                if !self.themes.remote() {
                    self.th.gal.note = Some((gallery::OFF.into(), true));
                    return Task::none();
                }
                self.th.gal.busy.insert(id.clone());
                self.th.gal.note = None;
                let (url, dev) = gallery::index_url();
                return run(
                    async move { gallery::fetch_css(&url, &e, true, dev).await },
                    move |r| wrap(GMsg::Installed(id, r)),
                );
            }
            GMsg::Installed(id, r) => {
                self.th.gal.busy.remove(&id);
                match (r, self.gallery_entry(&id)) {
                    (Ok(css), Some(e)) => {
                        self.themes.install_gallery(&e.id, &e.sha256, &e.name, &css);
                        self.themes.save();
                        self.th.applied = None;
                        self.th.gal.note = Some((format!("{} is installed.", e.name), false));
                    }
                    (Err(e), _) => self.th.gal.note = Some((e, true)),
                    _ => {}
                }
            }
            GMsg::Apply(id) => {
                if let Some(sid) = self.themes.from_gallery(&id).map(|s| s.id) {
                    self.themes.set_global(Some(sid));
                    self.themes.save();
                    self.th.safe = false;
                    self.th.applied = None;
                    self.th.gal.note = Some(("Applied to the whole app.".into(), false));
                }
            }
        }
        Task::none()
    }

    fn gallery_entry(&self, id: &str) -> Option<Entry> {
        let l = self.th.gal.index.as_ref()?;
        l.entries.iter().find(|e| e.id == id).cloned()
    }

    /// The tab buttons and the search field above the cards.
    fn gallery_bar(&self) -> El<'_> {
        let g = &self.th.gal;
        let on = g.tab;
        let status = match (&g.index, g.loading) {
            _ if on == Tab::Mine || !self.themes.remote() => String::new(),
            (_, true) => "Loading the gallery...".to_string(),
            (Some(l), _) if l.stale => format!("Offline: the copy from {}", ago(l.fetched)),
            (Some(l), _) => format!("Updated {}", ago(l.fetched)),
            _ => String::new(),
        };
        let mut bar = row![].spacing(t::S2).align_y(Alignment::Center);
        let tab = |label: &str, tab: Tab| {
            action_button(
                label,
                on == tab,
                false,
                false,
                false,
                Message::Themes(Msg::Gallery(GMsg::Tab(tab))),
            )
        };
        bar = bar
            .push(tab("Official", Tab::Official))
            .push(tab("Community", Tab::Community))
            .push(tab("My themes", Tab::Mine))
            .push(space().width(Length::Fill))
            .push(text(status).size(t::CAPTION).color(t::text_tertiary()));
        if on != Tab::Mine && self.themes.remote() {
            bar = bar.push(action_button(
                "Refresh",
                false,
                false,
                false,
                g.loading,
                Message::Themes(Msg::Gallery(GMsg::Refresh)),
            ));
        }
        bar.push(
            text_input("Search themes", &g.query)
                .on_input(|s| Message::Themes(Msg::Gallery(GMsg::Search(s))))
                .size(t::CONTROL_TEXT)
                .padding([t::S2, t::S3])
                .style(style::input)
                .width(Length::Fixed(280.0)),
        )
        .into()
    }

    /// The segmented "Gallery | Editor" switch under the page header.
    pub(super) fn themes_switch(&self) -> El<'_> {
        let on = self.th.gal.view;
        let b = |label: &str, v: View| {
            action_button(
                label,
                on == v,
                false,
                false,
                false,
                Message::Themes(Msg::Gallery(GMsg::View(v))),
            )
        };
        row![b("Gallery", View::Gallery), b("Editor", View::Editor)]
            .spacing(2)
            .into()
    }

    /// The whole Gallery view: tabs, search, status line and the card grid.
    pub(super) fn gallery_page(&self) -> El<'_> {
        let g = &self.th.gal;
        let small = |s: String, c: iced::Color| text(s).size(t::CAPTION).color(c);
        let msg = |m: GMsg| Message::Themes(Msg::Gallery(m));
        let mut col = column![self.gallery_bar()].spacing(t::S3);
        let mine = g.tab == Tab::Mine;
        if !mine && !self.themes.remote() {
            return col
                .push(small(gallery::OFF.into(), t::text_secondary()))
                .into();
        }
        if !mine && let Some(e) = &g.err {
            col = col.push(small(e.clone(), t::danger()));
        }
        if let Some((n, err)) = &g.note {
            let c = if *err {
                t::danger()
            } else {
                t::text_secondary()
            };
            col = col.push(small(n.clone(), c));
        }
        let q = g.query.trim().to_lowercase();
        let hit =
            |parts: &[&str]| q.is_empty() || parts.iter().any(|p| p.to_lowercase().contains(&q));
        let gap = t::S4;
        let usable = self.win.width - 2.0 * t::page_pad() - t::S3;
        let cols = (((usable + gap) / (CARD_MIN + gap)) as usize).max(1);
        let cw = ((usable - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let mut cards: Vec<El<'_>> = vec![];
        let entries = || g.index.iter().flat_map(|l| l.entries.iter());
        match g.tab {
            Tab::Official | Tab::Community => {
                let official = g.tab == Tab::Official;
                if official {
                    for (i, b) in import::BUILTIN.iter().enumerate() {
                        if entries().any(|e| e.id == b.id) || !hit(&[b.name]) {
                            continue;
                        }
                        let on = match self.themes.global {
                            None => b.css.is_empty(),
                            Some(id) => {
                                !b.css.is_empty()
                                    && self.themes.get(id).is_some_and(|s| s.css == b.css)
                            }
                        };
                        let act = if on {
                            mono("APPLIED").into()
                        } else {
                            action_button(
                                "Apply",
                                true,
                                false,
                                false,
                                false,
                                msg(GMsg::ApplyBuiltin(i)),
                            )
                        };
                        cards.push(
                            self.card(
                                cw,
                                None,
                                b.name,
                                "NOCTURNE \u{b7} built in".into(),
                                if b.css.is_empty() {
                                    "The default look."
                                } else {
                                    "Ships with NOCTURNE."
                                }
                                .into(),
                                Some(if on { "APPLIED" } else { "BUILT-IN" }),
                                vec![act],
                            ),
                        );
                    }
                }
                for e in entries().filter(|e| e.official() == official) {
                    if hit(&[&e.name, &e.description, &e.author]) {
                        cards.push(self.gallery_card(e, cw));
                    }
                }
                if cards.is_empty() && g.index.is_some() {
                    col = col.push(small(
                        if !q.is_empty() {
                            "No themes match the search."
                        } else if official {
                            "No official themes in the gallery."
                        } else {
                            "No community themes yet. Anyone can submit one by pull request: see themes/README.md in the repository."
                        }
                        .into(),
                        t::text_secondary(),
                    ));
                    if !official && q.is_empty() {
                        col = col.push(action_button(
                            "Open themes/README.md",
                            false,
                            false,
                            false,
                            false,
                            msg(GMsg::Submit),
                        ));
                    }
                }
            }
            Tab::Mine => {
                for s in &self.themes.themes {
                    let entry = s
                        .gallery
                        .as_deref()
                        .and_then(|gid| entries().find(|e| e.id == gid));
                    if let Some(e) = entry {
                        if hit(&[&e.name, &e.description, &e.author, &s.name]) {
                            cards.push(self.gallery_card(e, cw));
                        }
                    } else if hit(&[&s.name]) {
                        let applied = self.themes.global == Some(s.id);
                        let apply: El<'_> = if applied {
                            mono("APPLIED").into()
                        } else {
                            action_button(
                                "Apply",
                                true,
                                false,
                                false,
                                false,
                                msg(GMsg::ApplySaved(s.id)),
                            )
                        };
                        let edit = action_button(
                            "Edit",
                            false,
                            false,
                            false,
                            false,
                            msg(GMsg::Edit(s.id)),
                        );
                        let from_gallery = s.gallery.is_some();
                        cards.push(
                            self.card(
                                cw,
                                None,
                                &s.name,
                                "Your theme".into(),
                                if from_gallery {
                                    "Installed from the gallery."
                                } else {
                                    "Pasted, imported or downloaded."
                                }
                                .into(),
                                Some(if applied {
                                    "APPLIED"
                                } else if from_gallery {
                                    "INSTALLED"
                                } else {
                                    "NOT FROM THE GALLERY"
                                }),
                                vec![apply, edit],
                            ),
                        );
                    }
                }
                if cards.is_empty() {
                    col = col.push(small(
                        if q.is_empty() {
                            "No saved themes yet. Install one from Official or Community, or make one in the Editor."
                        } else {
                            "No themes match the search."
                        }
                        .into(),
                        t::text_secondary(),
                    ));
                }
            }
        }
        let mut it = cards.into_iter();
        loop {
            let mut r = row![].spacing(gap);
            let mut n = 0;
            for c in it.by_ref().take(cols) {
                r = r.push(c);
                n += 1;
            }
            if n == 0 {
                break;
            }
            for _ in n..cols {
                r = r.push(space().width(Length::Fixed(cw)));
            }
            col = col.push(r);
        }
        scrollable(col.spacing(t::S4).padding(iced::Padding {
            right: t::S3,
            bottom: t::S4,
            ..Default::default()
        }))
        .style(style::scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// One card: a 16:9 preview, name, byline, two lines of description, a status tag and the buttons.
    #[allow(clippy::too_many_arguments)]
    fn card<'a>(
        &'a self,
        cw: f32,
        pic: Option<&Handle>,
        name: &str,
        byline: String,
        desc: String,
        status: Option<&'static str>,
        acts: Vec<El<'a>>,
    ) -> El<'a> {
        let inner = cw - 2.0 * t::S3;
        let ph = (inner * 9.0 / 16.0).floor();
        let pic: El<'a> = match pic {
            Some(h) => iced::widget::image(h.clone())
                .width(Length::Fixed(inner))
                .height(Length::Fixed(ph))
                .content_fit(iced::ContentFit::Cover)
                .border_radius(t::r_xs())
                .into(),
            None => container(space())
                .width(Length::Fixed(inner))
                .height(Length::Fixed(ph))
                .style(|_| container::Style {
                    background: Some(t::raised().into()),
                    border: style::border(t::NONE, 0.0, t::r_xs()),
                    ..Default::default()
                })
                .into(),
        };
        let mut title = row![
            text(name.to_string())
                .size(t::CONTROL_TEXT)
                .font(t::semibold())
                .width(Length::Fill),
        ]
        .align_y(Alignment::Center);
        if let Some(s) = status {
            title = title.push(mono(s).size(t::CAPTION).color(t::text_tertiary()));
        }
        let mut btns = row![].spacing(t::S2).height(32).align_y(Alignment::Center);
        for a in acts {
            btns = btns.push(a);
        }
        container(
            column![
                pic,
                title,
                text(byline).size(t::CAPTION).color(t::text_tertiary()),
                container(text(desc).size(t::CAPTION).color(t::text_secondary()))
                    .height(Length::Fixed((t::CAPTION * 1.3 * 2.0).ceil() + 1.0))
                    .clip(true),
                btns,
            ]
            .spacing(t::S2),
        )
        .padding(t::S3)
        .width(Length::Fixed(cw))
        .style(style::panel)
        .into()
    }

    fn gallery_card(&self, e: &Entry, cw: f32) -> El<'_> {
        let g = &self.th.gal;
        let saved = self.themes.from_gallery(&e.id);
        let installed = saved.is_some();
        let current = saved.is_some_and(|s| s.gallery_sha.as_deref() == Some(e.sha256.as_str()));
        let applied = saved.is_some_and(|s| self.themes.global == Some(s.id));
        let busy = g.busy.contains(&e.id);
        let msg = |m: GMsg| Message::Themes(Msg::Gallery(m));
        let mut acts = vec![];
        if !e.compatible() {
            acts.push(small_note(format!("Needs NOCTURNE {}", e.min_version)));
        } else if !installed || !current {
            acts.push(action_button(
                if busy {
                    "Downloading..."
                } else if installed {
                    "Update"
                } else {
                    "Install"
                },
                true,
                false,
                false,
                busy,
                msg(GMsg::Install(e.id.clone())),
            ));
        }
        if let Some(s) = saved {
            acts.push(if applied {
                mono("APPLIED").into()
            } else {
                action_button(
                    "Apply",
                    false,
                    false,
                    false,
                    false,
                    msg(GMsg::Apply(e.id.clone())),
                )
            });
            acts.push(action_button(
                "Edit",
                false,
                false,
                false,
                false,
                msg(GMsg::Edit(s.id)),
            ));
        }
        let status = if applied {
            Some("APPLIED")
        } else if installed && !current {
            Some("UPDATE")
        } else if installed {
            Some("INSTALLED")
        } else {
            None
        };
        self.card(
            cw,
            g.previews.get(&e.id),
            &e.name,
            format!("{} \u{b7} {}", e.author, e.licence),
            e.description.clone(),
            status,
            acts,
        )
    }
}

fn small_note<'a>(s: String) -> El<'a> {
    text(s).size(t::CAPTION).color(t::text_tertiary()).into()
}
