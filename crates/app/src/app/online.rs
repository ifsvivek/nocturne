//! Online details and subtitles for local media (opt-in, off by default): the switches and keys on the
//! folders page, the background lookup run, the "Choose the right match" list, the attribution text and the
//! subtitle finder dialog. Logic and network code are in `nocturne_core::online`; this file is only the glue.
//! Keys live in the keyring; the per-source switches live in the source profile.
use super::message::Message;
use super::{App, Phase};
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, mono, progress_bar, rule, switch};
use iced::widget::{column, container, opaque, row, scrollable as scroll, space, text, text_input};
use iced::{Alignment, Element, Length, Padding, Task};
use nocturne_core::backend::Backend;
use nocturne_core::online::http::Err;
use nocturne_core::online::subs::{self, Hit, Query};
use nocturne_core::online::{
    self, Endpoints, Engine, Flags, PRIVACY, Progress, Providers, Report, TMDB_NOTICE,
    TVMAZE_CREDIT,
};
use nocturne_core::source::{Media, SourceId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

type El<'a> = Element<'a, Message>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    Details,
    Tmdb,
    Tvmaze,
    Musicbrainz,
    Subtitles,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Flag(Flag, bool),
    TmdbKey(String),
    SaveTmdb,
    SubsKey(String),
    SaveSubsKey,
    User(String),
    Pass(String),
    SaveLogin,
    Saved(Secrets),
    SecretSaved(&'static str, Result<(), String>),
    Lookup,
    Stop,
    Progress(SourceId, u64, Progress),
    Done(SourceId, u64, Report),
    Choose(String, usize),
    Clear,
    Open(&'static str),
    // The subtitle finder.
    Find(Box<Media>),
    Found(u64, Result<Vec<Hit>, &'static str>),
    Download(usize),
    Downloaded(u64, Result<PathBuf, &'static str>),
    CloseFinder,
}

/// Which secrets exist in the keyring (never their values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Secrets {
    pub tmdb: bool,
    pub subs: bool,
    pub login: bool,
}

struct Running {
    cancel: Arc<AtomicBool>,
    run_id: u64,
    progress: Progress,
}

pub struct Finder {
    media: Media,
    epoch: u64,
    hits: Option<Result<Vec<Hit>, &'static str>>,
    busy: bool,
    note: Option<Result<String, &'static str>>,
}

#[derive(Default)]
pub struct State {
    secrets: Secrets,
    tmdb_key: String,
    subs_key: String,
    user: String,
    pass: String,
    note: HashMap<&'static str, Result<String, &'static str>>,
    runs: HashMap<SourceId, Running>,
    gen_id: u64,
    finder: Option<Finder>,
}

impl State {
    pub fn finder_open(&self) -> bool {
        self.finder.is_some()
    }
    pub fn running(&self, id: SourceId) -> bool {
        self.runs.contains_key(&id)
    }
}

const TMDB_URL: &str = "https://www.themoviedb.org";
const TVMAZE_URL: &str = "https://www.tvmaze.com";
const MB_URL: &str = "https://musicbrainz.org";

fn err_text(e: Err) -> &'static str {
    match e {
        Err::Auth => "The service didn't accept the key or login. Check them and try again.",
        Err::Quota => "Today's download limit for this account is used up.",
        Err::NotFound => "Nothing found.",
        _ => "Couldn't reach the service. Try again later.",
    }
}

impl App {
    fn local_flags(&self, id: SourceId) -> Flags {
        self.sources.get(id).map(|p| p.online).unwrap_or_default()
    }

    /// Which keys exist, read off the UI thread when the folders page opens.
    pub(super) fn load_secrets(&self) -> Task<Message> {
        run(
            async {
                tokio::task::spawn_blocking(|| Secrets {
                    tmdb: online::has_secret(online::KEY_TMDB),
                    subs: online::has_secret(online::KEY_SUBS),
                    login: online::has_secret(online::KEY_SUBS_LOGIN),
                })
                .await
                .unwrap_or_default()
            },
            |s| Message::Online(Msg::Saved(s)),
        )
    }

    /// Start the background lookup for a local source: after each scan, when a switch changes, on request.
    /// Does nothing unless the source's details switch is on.
    pub(super) fn start_online(&mut self, id: SourceId) -> Task<Message> {
        let flags = self.local_flags(id);
        let Some(Backend::Local(l)) = self.catalog.backend(id).cloned() else {
            return Task::none();
        };
        if !flags.details || self.online.runs.contains_key(&id) {
            return Task::none();
        }
        self.online.gen_id += 1;
        let run_id = self.online.gen_id;
        let cancel = Arc::new(AtomicBool::new(false));
        self.online.runs.insert(
            id,
            Running {
                cancel: cancel.clone(),
                run_id,
                progress: Progress::default(),
            },
        );
        let (tx, rx) = iced::futures::channel::mpsc::unbounded();
        nocturne_core::bridge::spawn(async move {
            let key = tokio::task::spawn_blocking(online::tmdb_key)
                .await
                .ok()
                .flatten();
            let providers = Providers::from_flags(&flags, key);
            let engine = Engine::new(Endpoints::default(), cancel);
            let tx2 = tx.clone();
            let rep = engine
                .run(&l, &providers, move |p| {
                    let _ = tx2.unbounded_send(Msg::Progress(id, run_id, p));
                })
                .await;
            let _ = tx.unbounded_send(Msg::Done(id, run_id, rep));
        });
        Task::run(rx, Message::Online)
    }

    fn stop_online(&mut self, id: SourceId) {
        if let Some(r) = self.online.runs.get(&id) {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn online_msg(&mut self, m: Msg) -> Task<Message> {
        let folder_id = self.folders.as_ref().map(|f| f.id);
        match m {
            Msg::Flag(flag, on) => {
                let Some(id) = folder_id else {
                    return Task::none();
                };
                self.sources.set_online(id, |f| match flag {
                    Flag::Details => f.details = on,
                    Flag::Tmdb => f.tmdb = on,
                    Flag::Tvmaze => f.tvmaze = on,
                    Flag::Musicbrainz => f.musicbrainz = on,
                    Flag::Subtitles => f.subtitles = on,
                });
                self.save_sources();
                self.sync_profile(id);
                if on && flag != Flag::Subtitles {
                    return self.start_online(id);
                }
            }
            Msg::TmdbKey(s) => {
                self.online.tmdb_key = nocturne_core::secret::clean(&s);
                self.online.note.remove("tmdb");
            }
            Msg::SaveTmdb => {
                let key = std::mem::take(&mut self.online.tmdb_key);
                if key.trim().is_empty() {
                    // Blank means remove.
                    return run(
                        async {
                            tokio::task::spawn_blocking(|| online::set_secret(online::KEY_TMDB, ""))
                                .await
                                .unwrap_or(Err("failed".into()))
                        },
                        |r| Message::Online(Msg::SecretSaved("tmdb", r)),
                    );
                }
                self.online
                    .note
                    .insert("tmdb", Ok("Checking the key\u{2026}".into()));
                // One request to TMDB to check it; stored only if accepted.
                return run(
                    async move {
                        let e = Engine::new(Endpoints::default(), Arc::default());
                        match e.validate_tmdb(key.trim()).await {
                            Ok(()) => {
                                let k = key.trim().to_string();
                                tokio::task::spawn_blocking(move || {
                                    online::set_secret(online::KEY_TMDB, &k)
                                })
                                .await
                                .unwrap_or(Err("failed".into()))
                            }
                            Err(Err::Auth) => Err("TMDB didn't accept that key.".into()),
                            Err(_) => {
                                Err("Couldn't reach TMDB to check the key. Try again.".into())
                            }
                        }
                    },
                    |r| Message::Online(Msg::SecretSaved("tmdb", r)),
                );
            }
            Msg::SubsKey(s) => {
                self.online.subs_key = nocturne_core::secret::clean(&s);
                self.online.note.remove("subs");
            }
            Msg::SaveSubsKey => {
                let key = std::mem::take(&mut self.online.subs_key);
                return run(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            online::set_secret(online::KEY_SUBS, &key)
                        })
                        .await
                        .unwrap_or(Err("failed".into()))
                    },
                    |r| Message::Online(Msg::SecretSaved("subs", r)),
                );
            }
            Msg::User(s) => self.online.user = s,
            Msg::Pass(s) => self.online.pass = s,
            Msg::SaveLogin => {
                let (u, p) = (
                    std::mem::take(&mut self.online.user),
                    std::mem::take(&mut self.online.pass),
                );
                return run(
                    async move {
                        tokio::task::spawn_blocking(move || subs::set_login(&u, &p))
                            .await
                            .unwrap_or(Err("failed".into()))
                    },
                    |r| Message::Online(Msg::SecretSaved("login", r)),
                );
            }
            Msg::Saved(s) => self.online.secrets = s,
            Msg::SecretSaved(which, r) => {
                self.online.note.insert(
                    which,
                    match r {
                        Ok(()) => Ok("Saved in your system keyring.".into()),
                        Err(_) => Err("Couldn't save it: no system keyring is available."),
                    },
                );
                let load = self.load_secrets();
                // A new key can unblock a waiting lookup.
                let go = folder_id.map_or_else(Task::none, |id| self.start_online(id));
                return Task::batch([load, go]);
            }
            Msg::Lookup => {
                if let Some(id) = folder_id {
                    return self.start_online(id);
                }
            }
            Msg::Stop => {
                if let Some(id) = folder_id {
                    self.stop_online(id);
                }
            }
            Msg::Progress(id, run_id, p) => {
                if let Some(r) = self.online.runs.get_mut(&id).filter(|r| r.run_id == run_id) {
                    r.progress = p;
                }
            }
            Msg::Done(id, run_id, rep) => {
                if self.online.runs.get(&id).is_none_or(|r| r.run_id != run_id) {
                    return Task::none();
                }
                self.online.runs.remove(&id);
                if rep.bad_key {
                    self.toast("TMDB didn't accept your key. Check it under Local folders.");
                }
                return Task::batch([self.retry_page(), self.refresh_folders()]);
            }
            Msg::Choose(key, idx) => {
                let Some(id) = folder_id else {
                    return Task::none();
                };
                let Some(l) = self.catalog.backend(id).and_then(|b| b.local()).cloned() else {
                    return Task::none();
                };
                // Stored first, then the lookup runs and fetches exactly that pick.
                return run(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            l.with_mut(|lib| lib.choose_online(&key, idx)).is_ok()
                        })
                        .await
                        .unwrap_or(false)
                    },
                    |_| Message::Online(Msg::Lookup),
                );
            }
            Msg::Clear => {
                let Some(l) = folder_id
                    .and_then(|id| self.catalog.backend(id))
                    .and_then(|b| b.local())
                    .cloned()
                else {
                    return Task::none();
                };
                self.toast("Downloaded details removed.");
                return run(
                    async move {
                        tokio::task::spawn_blocking(move || l.with_mut(|lib| lib.online_clear()))
                            .await
                            .ok();
                    },
                    |()| Message::Folder(super::folders::Msg::Saved(true)),
                );
            }
            Msg::Open(url) => {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            }
            Msg::Find(media) => return self.open_finder(*media),
            Msg::Found(epoch, r) => {
                if let Some(f) = self.online.finder.as_mut().filter(|f| f.epoch == epoch) {
                    f.hits = Some(r);
                }
            }
            Msg::Download(i) => return self.download_sub(i),
            Msg::Downloaded(epoch, r) => {
                if let Some(f) = self.online.finder.as_mut().filter(|f| f.epoch == epoch) {
                    f.busy = false;
                    if let (Ok(path), Some(p)) = (&r, self.player.as_mut())
                        && p.media.id == f.media.id
                        && p.media.origin == f.media.origin
                    {
                        p.add_sub_file(path);
                    }
                    f.note =
                        Some(r.map(|_| {
                            "Saved. It loads the next time you play this file.".to_string()
                        }));
                }
            }
            Msg::CloseFinder => self.online.finder = None,
        }
        Task::none()
    }

    // ---- the subtitle finder ----

    fn open_finder(&mut self, media: Media) -> Task<Message> {
        self.online.gen_id += 1;
        let epoch = self.online.gen_id;
        let lang = self.settings.sub_lang.clone();
        let q = Query::for_media(&media, lang.as_deref());
        self.online.finder = Some(Finder {
            media,
            epoch,
            hits: None,
            busy: false,
            note: None,
        });
        let Some(q) = q else {
            return Task::done(Message::Online(Msg::Found(
                epoch,
                Err("Only movies and episodes can be searched."),
            )));
        };
        run(
            async move {
                let Some((key, _)) = tokio::task::spawn_blocking(subs::credentials)
                    .await
                    .ok()
                    .flatten()
                else {
                    return Err(
                        "Add your OpenSubtitles API key under Local folders, Online details.",
                    );
                };
                let e = Engine::new(Endpoints::default(), Arc::default());
                e.find_subtitles(&key, &q).await.map_err(err_text)
            },
            move |r| Message::Online(Msg::Found(epoch, r)),
        )
    }

    fn download_sub(&mut self, i: usize) -> Task<Message> {
        let Some(f) = self.online.finder.as_mut().filter(|f| !f.busy) else {
            return Task::none();
        };
        let Some(Ok(hits)) = &f.hits else {
            return Task::none();
        };
        let Some(hit) = hits.get(i).cloned() else {
            return Task::none();
        };
        let Some(l) = self
            .catalog
            .backend(f.media.origin)
            .and_then(|b| b.local())
            .cloned()
        else {
            return Task::none();
        };
        f.busy = true;
        f.note = None;
        let (epoch, id) = (f.epoch, f.media.id.clone());
        run(
            async move {
                let Some((key, login)) = tokio::task::spawn_blocking(subs::credentials)
                    .await
                    .ok()
                    .flatten()
                else {
                    return Err(
                        "Add your OpenSubtitles API key under Local folders, Online details.",
                    );
                };
                let e = Engine::new(Endpoints::default(), Arc::default());
                let (name, bytes) = e
                    .download_subtitle(&key, login.as_ref(), &hit)
                    .await
                    .map_err(|er| match er {
                        // OpenSubtitles asks for a login before it hands out files.
                        Err::Auth => "OpenSubtitles needs your login to download. Add it under Local folders, Online details.",
                        other => err_text(other),
                    })?;
                tokio::task::spawn_blocking(move || l.save_subtitle(&id, &name, &bytes))
                    .await
                    .map_err(|_| "Couldn't save the subtitle.")?
                    .map_err(|_| "Couldn't save the subtitle.")
            },
            move |r| Message::Online(Msg::Downloaded(epoch, r)),
        )
    }

    pub(super) fn finder_dialog(&self) -> El<'_> {
        let f = self.online.finder.as_ref().expect("finder is open");
        let mut card = column![
            mono("SUBTITLES"),
            text(f.media.title.clone()).size(t::HEADING).font(t::BOLD),
            text("Searches OpenSubtitles with this title, its year or id and your subtitle language. Nothing about the file is sent. Downloads are kept in NOCTURNE's own folder, never next to your media.")
                .size(t::CONTROL_TEXT)
                .line_height(1.5)
                .color(t::TEXT_SECONDARY),
        ]
        .spacing(t::S3);
        match &f.hits {
            None => {
                card = card.push(
                    text("Searching\u{2026}")
                        .size(t::BODY)
                        .color(t::TEXT_SECONDARY),
                )
            }
            Some(Err(e)) => card = card.push(text(*e).size(t::BODY).color(t::DANGER)),
            Some(Ok(h)) if h.is_empty() => {
                card = card.push(
                    text("No subtitles found in your language.")
                        .size(t::BODY)
                        .color(t::TEXT_SECONDARY),
                );
            }
            Some(Ok(hits)) => {
                let mut list = column![].spacing(t::S2);
                for (i, h) in hits.iter().take(30).enumerate() {
                    let name = if h.release.is_empty() {
                        h.file_name.clone()
                    } else {
                        h.release.clone()
                    };
                    let mut meta = format!(
                        "{}  \u{b7}  rating {:.1}  \u{b7}  {} downloads",
                        h.language.to_uppercase(),
                        h.rating,
                        h.downloads
                    );
                    if h.hearing_impaired {
                        meta.push_str("  \u{b7}  hearing impaired");
                    }
                    list = list.push(
                        row![
                            column![text(name).size(t::BODY), mono(meta)]
                                .spacing(2)
                                .width(Length::Fill),
                            action_button(
                                "Download",
                                false,
                                false,
                                false,
                                f.busy,
                                Message::Online(Msg::Download(i))
                            ),
                        ]
                        .align_y(Alignment::Center)
                        .spacing(t::S3),
                    );
                }
                card = card.push(scroll(list).style(style::scroll).height(Length::Shrink));
            }
        }
        if f.busy {
            card = card.push(
                text("Downloading\u{2026}")
                    .size(t::CONTROL_TEXT)
                    .color(t::TEXT_SECONDARY),
            );
        }
        match &f.note {
            Some(Ok(s)) => {
                card = card.push(text(s.clone()).size(t::CONTROL_TEXT).color(t::ACCENT_TEXT))
            }
            Some(Err(s)) => card = card.push(text(*s).size(t::CONTROL_TEXT).color(t::DANGER)),
            None => {}
        }
        card = card.push(
            container(action_button(
                "Close",
                true,
                false,
                true,
                false,
                Message::Online(Msg::CloseFinder),
            ))
            .align_right(Length::Fill),
        );
        opaque(
            container(
                container(card)
                    .width(560)
                    .max_height(620)
                    .padding(t::S6)
                    .style(style::panel),
            )
            .center(Length::Fill)
            .style(style::scrim),
        )
    }

    // ---- the folders page section ----

    pub(super) fn online_section(
        &self,
        id: SourceId,
        pending: &[nocturne_core::folders::Pending],
    ) -> El<'_> {
        let flags = self.local_flags(id);
        let st = &self.online;
        let note = |k: &str| -> El<'_> {
            match st.note.get(k) {
                Some(Ok(s)) => text(s.clone())
                    .size(t::CONTROL_TEXT)
                    .color(t::ACCENT_TEXT)
                    .into(),
                Some(Err(s)) => text(*s).size(t::CONTROL_TEXT).color(t::DANGER).into(),
                None => space().into(),
            }
        };
        let toggle = |label: &str, hint: &str, on: bool, flag: Flag| -> El<'_> {
            row![
                column![
                    text(label.to_string()).size(t::BODY),
                    text(hint.to_string())
                        .size(t::CONTROL_TEXT)
                        .color(t::TEXT_SECONDARY),
                ]
                .spacing(2)
                .width(Length::Fill),
                switch(on, false, Message::Online(Msg::Flag(flag, !on))),
            ]
            .align_y(Alignment::Center)
            .spacing(t::S4)
            .into()
        };
        let field = |ph: &str, v: &str, secret: bool, on_input: fn(String) -> Msg, submit: Msg| {
            let i = text_input(ph, v)
                .on_input(move |s| Message::Online(on_input(s)))
                .on_submit(Message::Online(submit))
                .size(t::BODY)
                .padding([t::S2, t::S3])
                .style(style::input);
            if secret { i.secure(true) } else { i }
        };
        let saved = |b: bool| {
            if b {
                "A key is saved."
            } else {
                "No key saved yet."
            }
        };
        let head = |s: &str| text(s.to_string()).size(t::BODY_LG).font(t::BOLD);
        let mut c = column![
            head("Online details"),
            text("Optional and off by default. NOCTURNE works fully from your files without it.")
                .size(t::CONTROL_TEXT)
                .color(t::TEXT_SECONDARY),
            toggle(
                "Look up details online",
                "Posters, descriptions, genres and ratings for titles your files don't describe.",
                flags.details,
                Flag::Details
            ),
            container(
                text(PRIVACY)
                    .size(t::CONTROL_TEXT)
                    .line_height(1.5)
                    .color(t::TEXT_TERTIARY)
            )
            .max_width(640),
        ]
        .spacing(t::S3);

        if flags.details {
            c = c.push(toggle(
                "TMDB",
                "Movies and TV. Needs your own free TMDB key.",
                flags.tmdb,
                Flag::Tmdb,
            ));
            if flags.tmdb {
                c = c.push(
                    column![
                        row![
                            field(
                                "TMDB API key or read access token",
                                &st.tmdb_key,
                                true,
                                Msg::TmdbKey,
                                Msg::SaveTmdb
                            )
                            .width(360),
                            action_button(
                                "Check and save",
                                true,
                                false,
                                false,
                                false,
                                Message::Online(Msg::SaveTmdb)
                            ),
                            action_button(
                                "Get a key",
                                false,
                                false,
                                false,
                                false,
                                Message::Online(Msg::Open(
                                    "https://www.themoviedb.org/settings/api"
                                ))
                            ),
                        ]
                        .spacing(t::S2)
                        .align_y(Alignment::Center),
                        row![
                            text(saved(st.secrets.tmdb))
                                .size(t::CONTROL_TEXT)
                                .color(t::TEXT_SECONDARY),
                            note("tmdb"),
                        ]
                        .spacing(t::S3),
                    ]
                    .spacing(t::S2)
                    .padding(Padding {
                        left: t::S4,
                        ..Padding::ZERO
                    }),
                );
            }
            c = c.push(toggle(
                "TVmaze",
                "TV shows and episodes. No key needed.",
                flags.tvmaze,
                Flag::Tvmaze,
            ));
            c = c.push(toggle(
                "MusicBrainz and Cover Art Archive",
                "Albums: year, genres and cover. No key needed.",
                flags.musicbrainz,
                Flag::Musicbrainz,
            ));

            // Progress or the way to start.
            match st.runs.get(&id) {
                Some(r) => {
                    let frac = if r.progress.total > 0 {
                        r.progress.done as f32 / r.progress.total as f32
                    } else {
                        0.0
                    };
                    c = c.push(
                        row![
                            text(format!(
                                "Looking up details {} of {}",
                                r.progress.done, r.progress.total
                            ))
                            .size(t::BODY),
                            space().width(Length::Fill),
                            action_button(
                                "Stop",
                                false,
                                false,
                                false,
                                false,
                                Message::Online(Msg::Stop)
                            ),
                        ]
                        .align_y(Alignment::Center),
                    );
                    c = c.push(container(progress_bar(frac, 4.0)).max_width(520));
                }
                None => {
                    c = c.push(
                        row![
                            action_button(
                                "Look up now",
                                false,
                                false,
                                false,
                                false,
                                Message::Online(Msg::Lookup)
                            ),
                            action_button(
                                "Remove downloaded details",
                                false,
                                true,
                                false,
                                false,
                                Message::Online(Msg::Clear)
                            ),
                        ]
                        .spacing(t::S2),
                    );
                }
            }

            if !pending.is_empty() {
                c = c.push(head("Choose the right match"));
                c = c.push(
                    text("These titles fit more than one result. Pick the right one; nothing is guessed.")
                        .size(t::CONTROL_TEXT)
                        .color(t::TEXT_SECONDARY),
                );
                for p in pending.iter().take(20) {
                    let mut opts = column![].spacing(t::S1);
                    for (i, cand) in p.candidates.iter().enumerate() {
                        let y = cand.year.map_or(String::new(), |y| format!(" ({y})"));
                        let from = nocturne_local::online::provider_label(&cand.provider);
                        opts = opts.push(
                            row![
                                column![
                                    text(format!("{}{y}", cand.title)).size(t::BODY),
                                    text(cand.overview.chars().take(110).collect::<String>())
                                        .size(t::CAPTION)
                                        .color(t::TEXT_TERTIARY),
                                ]
                                .spacing(2)
                                .width(Length::Fill),
                                mono(from.to_uppercase()),
                                action_button(
                                    "This one",
                                    false,
                                    false,
                                    false,
                                    false,
                                    Message::Online(Msg::Choose(p.key.clone(), i))
                                ),
                            ]
                            .align_y(Alignment::Center)
                            .spacing(t::S3),
                        );
                    }
                    c = c.push(
                        column![text(p.label.clone()).size(t::BODY).font(t::SEMIBOLD), opts]
                            .spacing(t::S2),
                    );
                }
            }
        }

        // Subtitles.
        c = c.push(space().height(t::S3));
        c = c.push(head("Subtitles"));
        c = c.push(toggle("Find subtitles online", "Adds \u{201c}Find subtitles online\u{201d} to the \u{2026} menu of your files. Searches OpenSubtitles with the title, year or id and your subtitle language; saves in NOCTURNE's own folder, never next to your media.", flags.subtitles, Flag::Subtitles));
        if flags.subtitles {
            c = c.push(
                column![
                    row![
                        field("OpenSubtitles API key", &st.subs_key, true, Msg::SubsKey, Msg::SaveSubsKey).width(360),
                        action_button("Save key", true, false, false, false, Message::Online(Msg::SaveSubsKey)),
                        action_button("Get a key", false, false, false, false, Message::Online(Msg::Open("https://www.opensubtitles.com/consumers"))),
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                    row![
                        field("Your OpenSubtitles user name", &st.user, false, Msg::User, Msg::SaveLogin).width(240),
                        field("Password", &st.pass, true, Msg::Pass, Msg::SaveLogin).width(200),
                        action_button("Save login", false, false, false, false, Message::Online(Msg::SaveLogin)),
                    ]
                    .spacing(t::S2)
                    .align_y(Alignment::Center),
                    row![
                        text(format!("{} {}", saved(st.secrets.subs), if st.secrets.login { "A login is saved." } else { "No login saved (downloads usually need one)." }))
                            .size(t::CONTROL_TEXT)
                            .color(t::TEXT_SECONDARY),
                        note("subs"),
                        note("login"),
                    ]
                    .spacing(t::S3),
                    text("Your key and login are kept in the system keyring, never in a file. Leave a field empty and save to remove it.")
                        .size(t::CAPTION)
                        .color(t::TEXT_TERTIARY),
                ]
                .spacing(t::S2)
                .padding(Padding { left: t::S4, ..Padding::ZERO }),
            );
        }

        // Credits for every provider switched on.
        let mut credits = column![].spacing(t::S2);
        let link = |name: &'static str, url: &'static str| -> El<'static> {
            action_button(
                name,
                false,
                false,
                false,
                false,
                Message::Online(Msg::Open(url)),
            )
        };
        if flags.details && flags.tmdb {
            credits = credits.push(
                row![
                    link("themoviedb.org", TMDB_URL),
                    text(TMDB_NOTICE).size(t::CAPTION).color(t::TEXT_TERTIARY)
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            );
        }
        if flags.details && flags.tvmaze {
            credits = credits.push(
                row![
                    link("tvmaze.com", TVMAZE_URL),
                    text(TVMAZE_CREDIT).size(t::CAPTION).color(t::TEXT_TERTIARY)
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            );
        }
        if flags.details && flags.musicbrainz {
            credits = credits.push(
                row![
                    link("musicbrainz.org", MB_URL),
                    text("Album data from MusicBrainz; covers from the Cover Art Archive.")
                        .size(t::CAPTION)
                        .color(t::TEXT_TERTIARY)
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            );
        }
        if flags.subtitles {
            credits = credits.push(
                row![
                    link("opensubtitles.com", "https://www.opensubtitles.com"),
                    text("Subtitles from OpenSubtitles.")
                        .size(t::CAPTION)
                        .color(t::TEXT_TERTIARY)
                ]
                .spacing(t::S3)
                .align_y(Alignment::Center),
            );
        }
        container(column![rule(), container(c).padding([t::S6, 0.0]), credits].spacing(t::S2))
            .padding([0.0, t::PAGE_PAD])
            .into()
    }

    /// Whether the finder should be offered on an item's detail page.
    pub(super) fn subtitles_on(&self, origin: SourceId) -> bool {
        matches!(self.catalog.backend(origin), Some(Backend::Local(_)))
            && self.local_flags(origin).subtitles
            && matches!(self.phase, Phase::Ready)
    }
}
