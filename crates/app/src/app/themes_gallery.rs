//! The themes gallery on the Themes page: tabs Official / Community / My themes, and cards with a preview, the name, the author and licence, the description and Install / Installed / Update. Downloads, the checksum and the caching live in `nocturne_core::customize::import::gallery`; an installed theme is a normal saved theme tagged with its gallery id (`Saved::gallery`), so Update replaces it and themes added by hand stay untouched.
use super::App;
use super::message::Message;
use super::themes::{Msg, ago};
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, mono};
use iced::widget::image::Handle;
use iced::widget::{column, container, row, space, text};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::customize::import::gallery::{self, Entry, Loaded};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type El<'a> = Element<'a, Message>;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    Official,
    Community,
    #[default]
    Mine,
}

#[derive(Clone)]
pub enum GMsg {
    Tab(Tab),
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
    index: Option<Arc<Loaded>>,
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

    /// The three tab buttons above the theme lists.
    pub(super) fn gallery_tabs(&self) -> El<'_> {
        let on = self.th.gal.tab;
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
        row![
            tab("Official", Tab::Official),
            tab("Community", Tab::Community),
            tab("My themes", Tab::Mine),
        ]
        .spacing(t::S2)
        .into()
    }

    /// The cards of the Official or Community tab.
    pub(super) fn gallery_cards(&self) -> El<'_> {
        let g = &self.th.gal;
        let official = g.tab == Tab::Official;
        let small = |s: String, c: iced::Color| text(s).size(t::CAPTION).color(c);
        let mut col = column![].spacing(t::S3);
        if !self.themes.remote() {
            return col
                .push(small(gallery::OFF.into(), t::text_secondary()))
                .into();
        }
        let mut head = row![].spacing(t::S2).align_y(Alignment::Center);
        let status = match (&g.index, g.loading) {
            (_, true) => "Loading the gallery...".to_string(),
            (Some(l), _) if l.stale => {
                format!("Offline: the copy from {}", ago(l.fetched))
            }
            (Some(l), _) => format!("Updated {}", ago(l.fetched)),
            _ => String::new(),
        };
        head = head
            .push(small(status, t::text_tertiary()).width(Length::Fill))
            .push(action_button(
                "Refresh",
                false,
                false,
                false,
                g.loading,
                Message::Themes(Msg::Gallery(GMsg::Refresh)),
            ));
        col = col.push(head);
        if let Some(e) = &g.err {
            col = col.push(small(e.clone(), t::danger()));
        }
        if let Some((n, err)) = &g.note {
            col = col.push(small(
                n.clone(),
                if *err {
                    t::danger()
                } else {
                    t::text_secondary()
                },
            ));
        }
        let mut shown = 0;
        for e in g
            .index
            .iter()
            .flat_map(|l| l.entries.iter())
            .filter(|e| e.official() == official)
        {
            shown += 1;
            col = col.push(self.gallery_card(e));
        }
        if shown == 0 && g.index.is_some() {
            col = col.push(small(
                if official {
                    "No official themes in the gallery."
                } else {
                    "No community themes yet. Anyone can submit one by pull request: see themes/README.md in the repository."
                }
                .into(),
                t::text_secondary(),
            ));
        }
        col.into()
    }

    fn gallery_card(&self, e: &Entry) -> El<'_> {
        let g = &self.th.gal;
        let saved = self.themes.from_gallery(&e.id);
        let installed = saved.is_some();
        let current = saved.is_some_and(|s| s.gallery_sha.as_deref() == Some(e.sha256.as_str()));
        let applied = saved.is_some_and(|s| self.themes.global == Some(s.id));
        let busy = g.busy.contains(&e.id);
        let msg = |m: GMsg| Message::Themes(Msg::Gallery(m));
        let pic: El<'_> = match g.previews.get(&e.id) {
            Some(h) => iced::widget::image(h.clone())
                .width(Length::Fill)
                .border_radius(t::r_xs())
                .into(),
            None => container(space())
                .width(Length::Fill)
                .height(Length::Fixed(150.0))
                .style(|_| container::Style {
                    background: Some(t::raised().into()),
                    border: style::border(t::NONE, 0.0, t::r_xs()),
                    ..Default::default()
                })
                .into(),
        };
        let mut acts = row![].spacing(t::S2).align_y(Alignment::Center);
        if !e.compatible() {
            acts = acts.push(small_note(format!("Needs NOCTURNE {}", e.min_version)));
        } else if !installed || !current {
            acts = acts.push(action_button(
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
        } else {
            acts = acts.push(mono("INSTALLED"));
        }
        if installed {
            acts = acts.push(if applied {
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
        }
        container(
            column![
                pic,
                text(e.name.clone())
                    .size(t::CONTROL_TEXT)
                    .font(t::semibold()),
                text(format!("{} \u{b7} {}", e.author, e.licence))
                    .size(t::CAPTION)
                    .color(t::text_tertiary()),
                text(e.description.clone())
                    .size(t::CAPTION)
                    .color(t::text_secondary()),
                acts,
            ]
            .spacing(t::S2),
        )
        .padding(t::S3)
        .width(Length::Fill)
        .style(style::panel)
        .into()
    }
}

fn small_note<'a>(s: String) -> El<'a> {
    text(s).size(t::CAPTION).color(t::text_tertiary()).into()
}
