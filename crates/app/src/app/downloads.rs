//! Offline copies in the UI: the "Download to this computer" question, the queue panel (account menu >
//! Downloads) and the one running transfer. The logic (queue, naming, resume) is in
//! `nocturne_core::download`; this file is glue. A server item is fetched with GET only and written as a
//! new file into the chosen local folder, which is rescanned when the copy lands.
use super::message::Message;
use super::{App, Phase};
use crate::style;
use crate::task::run;
use crate::theme as t;
use crate::widget::{action_button, menu_row, mono, progress_bar};
use iced::widget::{column, container, opaque, row, scrollable as scroll, space, text};
use iced::{Alignment, Element, Length, Task};
use nocturne_core::download::fetch::{self, CANCEL, End, PAUSE, Shared};
use nocturne_core::download::{self, Job, Meta, Queue, State as JobState, Why};
use nocturne_core::profile::ProfileKind;
use nocturne_core::source::{Kind, Media};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

type El<'a> = Element<'a, Message>;

#[derive(Clone, Debug)]
pub enum Msg {
    /// "Download to this computer" on a detail page.
    Ask(Box<Media>),
    Planned(u64, Box<Plan>),
    Pick(usize),
    /// Arrow keys in the destination list.
    Step(i32),
    Confirm,
    CancelAsk,
    Open,
    Close,
    Pause(u64),
    Resume(u64),
    Remove(u64),
    ClearDone,
    Finished(u64, Result<End, Why>),
}

/// What a request comes to: the files, and their size when the server tells.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    items: Vec<(String, Meta)>,
    total: u64,
    unknown: usize,
}

struct Ask {
    media: Media,
    epoch: u64,
    plan: Option<Plan>,
    roots: Vec<PathBuf>,
    sel: usize,
}

struct Running {
    id: u64,
    sh: Arc<Shared>,
    mark: (Instant, u64),
    speed: f64,
}

pub struct State {
    queue: Queue,
    path: PathBuf,
    run: Option<Running>,
    pub open: bool,
    ask: Option<Ask>,
    epoch: u64,
    saved: Instant,
}

impl State {
    /// A transfer runs or one waits to start: the root tick must keep polling.
    pub fn busy(&self) -> bool {
        self.run.is_some() || self.queue.next().is_some()
    }

    pub fn new(data: &std::path::Path) -> Self {
        let path = Queue::path(data);
        Self {
            queue: Queue::load(&path),
            path,
            run: None,
            open: false,
            ask: None,
            epoch: 0,
            saved: Instant::now(),
        }
    }

    pub fn visible(&self) -> bool {
        self.open || self.ask.is_some()
    }

    /// Jobs not finished (for the menu label).
    pub fn pending(&self) -> usize {
        self.queue
            .jobs
            .iter()
            .filter(|j| matches!(j.state, JobState::Queued | JobState::Active))
            .count()
    }

    fn save(&mut self) {
        // ponytail: a small atomic write on the UI thread, throttled in `tick`; move it off if it ever shows in a trace.
        let _ = self.queue.save(&self.path);
        self.saved = Instant::now();
    }

    /// Escape: the question first, then the panel.
    pub fn escape(&mut self) {
        if self.ask.take().is_none() {
            self.open = false;
        }
    }
}

impl App {
    pub(super) fn dl_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Ask(media) => return self.dl_ask(*media),
            Msg::Planned(epoch, plan) => {
                if let Some(a) = self.dl.ask.as_mut().filter(|a| a.epoch == epoch) {
                    a.plan = Some(*plan);
                }
            }
            Msg::Pick(i) => {
                if let Some(a) = self.dl.ask.as_mut() {
                    a.sel = i.min(a.roots.len().saturating_sub(1));
                }
            }
            Msg::Step(d) => {
                if let Some(a) = self.dl.ask.as_mut() {
                    a.sel = (a.sel as i32 + d).clamp(0, a.roots.len() as i32 - 1) as usize;
                }
            }
            Msg::CancelAsk => self.dl.ask = None,
            Msg::Confirm => return self.dl_confirm(),
            Msg::Open => {
                self.menu = false;
                self.dl.open = true;
            }
            Msg::Close => self.dl.open = false,
            Msg::Pause(id) => {
                if let Some(r) = self.dl.run.as_ref().filter(|r| r.id == id) {
                    r.sh.ctl.store(PAUSE, Ordering::Relaxed);
                } else if let Some(j) = self.dl.queue.get_mut(id)
                    && j.state == JobState::Queued
                {
                    j.state = JobState::Paused;
                    self.dl.save();
                }
            }
            Msg::Resume(id) => {
                if let Some(j) = self.dl.queue.get_mut(id)
                    && matches!(j.state, JobState::Paused | JobState::Failed(_))
                {
                    j.state = JobState::Queued;
                    self.dl.save();
                    return self.dl_pump();
                }
            }
            Msg::Remove(id) => {
                if let Some(r) = self.dl.run.as_ref().filter(|r| r.id == id) {
                    // Finished(Cancelled) deletes the part and the entry.
                    r.sh.ctl.store(CANCEL, Ordering::Relaxed);
                } else if let Some(j) = self.dl.queue.remove(id) {
                    if j.state != JobState::Done {
                        fetch::discard_part(&j);
                    }
                    self.dl.save();
                }
            }
            Msg::ClearDone => {
                for j in &self.dl.queue.jobs {
                    if matches!(j.state, JobState::Failed(_)) {
                        fetch::discard_part(j);
                    }
                }
                self.dl.queue.clear_finished();
                self.dl.save();
            }
            Msg::Finished(id, r) => return self.dl_finished(id, r),
        }
        Task::none()
    }

    /// Where a copy can go: the local library's folders, and NOCTURNE's own downloads folder.
    fn dl_roots(&self) -> Vec<PathBuf> {
        let mut roots = match self.sources.local().map(|p| &p.kind) {
            Some(ProfileKind::Local { roots }) => roots.clone(),
            _ => vec![],
        };
        let own = self.data.join("downloads");
        if !roots.contains(&own) {
            roots.push(own);
        }
        roots
    }

    fn dl_ask(&mut self, media: Media) -> Task<Message> {
        let Some(b) = self.catalog.backend(media.origin).cloned() else {
            return Task::none();
        };
        let Some((c, user)) = b.jellyfin().map(|s| s.client()) else {
            return Task::none();
        };
        self.dl.epoch += 1;
        let epoch = self.dl.epoch;
        self.dl.ask = Some(Ask {
            media: media.clone(),
            epoch,
            plan: None,
            roots: self.dl_roots(),
            sel: 0,
        });
        run(
            async move {
                let items = download::expand(&b, &media).await;
                // Provider ids: a movie's own, and the show's for episodes (what the local copy matches by).
                let mut need: Vec<String> = vec![];
                for i in &items {
                    let id = match i.kind {
                        Kind::Movie if i.ids.is_empty() => Some(i.id.clone()),
                        Kind::Episode => i.series_id.clone(),
                        _ => None,
                    };
                    if let Some(id) = id.filter(|id| !need.contains(id)) {
                        need.push(id);
                    }
                }
                let ids = if need.is_empty() {
                    Default::default()
                } else {
                    b.provider_ids(&need).await.unwrap_or_default()
                };
                let metas: Vec<(String, Meta)> = items
                    .iter()
                    .filter_map(|i| {
                        let show = i
                            .series_id
                            .as_ref()
                            .and_then(|s| ids.get(s))
                            .cloned()
                            .unwrap_or_default();
                        let mut i = i.clone();
                        if i.kind == Kind::Movie && i.ids.is_empty() {
                            i.ids = ids.get(&i.id).cloned().unwrap_or_default();
                        }
                        Some((i.id.clone(), Meta::of(&i, show)?))
                    })
                    .collect();
                let sizes = fetch::sizes(
                    &c,
                    &user,
                    &metas.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
                )
                .await;
                Plan {
                    total: sizes.iter().flatten().sum(),
                    unknown: sizes.iter().filter(|s| s.is_none()).count(),
                    items: metas,
                }
            },
            move |p| Message::Downloads(Msg::Planned(epoch, Box::new(p))),
        )
    }

    fn dl_confirm(&mut self) -> Task<Message> {
        let Some(a) = self.dl.ask.take_if(|a| a.plan.is_some()) else {
            return Task::none();
        };
        let (plan, Some(root)) = (a.plan.unwrap_or_default(), a.roots.get(a.sel).cloned()) else {
            return Task::none();
        };
        if let Err(e) = std::fs::create_dir_all(&root) {
            self.toast(format!("Can't use that folder ({}).", e.kind()));
            return Task::none();
        }
        let into = self.ensure_local_slot();
        // The chosen folder becomes part of the local library (the downloads folder the first time).
        if self.sources.add_root(root.clone()) {
            self.save_sources();
            self.sync_profile(into);
        }
        let origin = a.media.origin;
        let mut added = 0;
        for (item, meta) in plan.items {
            if self.dl.queue.has(origin, &item) {
                continue;
            }
            self.dl.queue.add(Job {
                id: 0,
                origin,
                item,
                meta,
                root: root.clone(),
                into,
                dest: None,
                done: 0,
                total: 0,
                state: JobState::Queued,
            });
            added += 1;
        }
        self.dl.save();
        self.dl.open = true;
        self.toast(match added {
            0 => "Already in your downloads.".to_string(),
            1 => "Added to Downloads.".to_string(),
            n => format!("Added {n} files to Downloads."),
        });
        self.dl_pump()
    }

    /// Start the next queued job when nothing runs.
    pub(super) fn dl_pump(&mut self) -> Task<Message> {
        if self.dl.run.is_some() || !matches!(self.phase, Phase::Ready) {
            return Task::none();
        }
        let Some(id) = self.dl.queue.next() else {
            return Task::none();
        };
        let Some(job) = self.dl.queue.get(id).cloned() else {
            return Task::none();
        };
        let Some((c, user)) = self
            .catalog
            .backend(job.origin)
            .and_then(|b| b.jellyfin())
            .map(|s| s.client())
        else {
            // The server is gone from this app: it can not be asked.
            if let Some(j) = self.dl.queue.get_mut(id) {
                j.state = JobState::Failed(Why::Auth);
            }
            self.dl.save();
            return self.dl_pump();
        };
        let sh = Arc::new(Shared::default());
        sh.done.store(job.done, Ordering::Relaxed);
        if let Some(j) = self.dl.queue.get_mut(id) {
            j.state = JobState::Active;
        }
        self.dl.run = Some(Running {
            id,
            sh: sh.clone(),
            mark: (Instant::now(), job.done),
            speed: 0.0,
        });
        self.dl.save();
        run(
            async move { fetch::run(&c, &user, &job, &sh).await },
            move |r| Message::Downloads(Msg::Finished(id, r)),
        )
    }

    /// Every root tick: read the running transfer's progress, and start one if the queue waits.
    pub(super) fn dl_tick(&mut self) -> Task<Message> {
        let Some(r) = self.dl.run.as_mut() else {
            return if self.dl.queue.next().is_some() {
                self.dl_pump()
            } else {
                Task::none()
            };
        };
        let (done, total) = (
            r.sh.done.load(Ordering::Relaxed),
            r.sh.total.load(Ordering::Relaxed),
        );
        let dest = r.sh.dest.lock().ok().and_then(|d| d.clone());
        let dt = r.mark.0.elapsed().as_secs_f64();
        if dt >= 1.0 {
            let inst = done.saturating_sub(r.mark.1) as f64 / dt;
            r.speed = if r.speed == 0.0 {
                inst
            } else {
                r.speed * 0.6 + inst * 0.4
            };
            r.mark = (Instant::now(), done);
        }
        let id = r.id;
        let mut dirty = false;
        if let Some(j) = self.dl.queue.get_mut(id) {
            (j.done, j.total) = (done, total);
            // Remembered as soon as it is known, so a restart finds the `.part`.
            if j.dest.is_none() && dest.is_some() {
                j.dest = dest;
                dirty = true;
            }
        }
        if dirty || self.dl.saved.elapsed() > Duration::from_secs(5) {
            self.dl.save();
        }
        Task::none()
    }

    fn dl_finished(&mut self, id: u64, r: Result<End, Why>) -> Task<Message> {
        let run = self.dl.run.take_if(|x| x.id == id);
        let dest = run
            .as_ref()
            .and_then(|x| x.sh.dest.lock().ok().and_then(|d| d.clone()));
        let totals = run.map(|x| {
            (
                x.sh.done.load(Ordering::Relaxed),
                x.sh.total.load(Ordering::Relaxed),
            )
        });
        let mut rescan = None;
        let mut note = None;
        match (r, self.dl.queue.get_mut(id)) {
            (Ok(End::Cancelled), _) => {
                self.dl.queue.remove(id);
            }
            (Ok(End::Done), Some(j)) => {
                j.dest = dest.or(j.dest.take());
                (j.done, j.total) = totals.unwrap_or((j.done, j.total));
                j.state = JobState::Done;
                note = Some(format!("Downloaded {}.", j.meta.label()));
                rescan = Some(j.into);
            }
            (Ok(End::Paused), Some(j)) => j.state = JobState::Paused,
            (Err(why), Some(j)) => {
                j.dest = dest.or(j.dest.take());
                j.state = JobState::Failed(why);
            }
            (_, None) => {}
        }
        self.dl.save();
        if let Some(n) = note {
            self.toast(n);
        }
        let mut tasks = vec![];
        // One scan after the last file for that folder, not one per episode.
        if let Some(into) =
            rescan.filter(|into| {
                !self.dl.queue.jobs.iter().any(|j| {
                    j.into == *into && matches!(j.state, JobState::Queued | JobState::Active)
                })
            })
        {
            tasks.push(self.rescan_now(into));
        }
        tasks.push(self.dl_pump());
        Task::batch(tasks)
    }

    // ---- view ----

    pub(super) fn downloads_dialog(&self) -> El<'_> {
        let mut card = column![mono("DOWNLOADS")].spacing(t::S3);
        if let Some(a) = &self.dl.ask {
            card = card.push(self.dl_ask_view(a));
        }
        if self.dl.open || self.dl.ask.is_none() {
            card = card.push(self.dl_list());
        }
        if self.dl.ask.is_some() {
            return Self::downloads_card(card);
        }
        let any_done = self
            .dl
            .queue
            .jobs
            .iter()
            .any(|j| matches!(j.state, JobState::Done | JobState::Failed(_)));
        let mut foot = row![space().width(Length::Fill)]
            .spacing(t::S2)
            .align_y(Alignment::Center);
        if any_done {
            foot = foot.push(action_button(
                "Clear finished",
                false,
                false,
                false,
                false,
                Message::Downloads(Msg::ClearDone),
            ));
        }
        foot = foot.push(action_button(
            "Close",
            true,
            false,
            true,
            false,
            Message::Downloads(Msg::Close),
        ));
        card = card.push(foot);
        Self::downloads_card(card)
    }

    fn downloads_card(card: iced::widget::Column<'_, Message>) -> El<'_> {
        opaque(
            container(
                container(card)
                    .width(620)
                    .max_height(640)
                    .padding(t::S6)
                    .style(style::panel),
            )
            .center(Length::Fill)
            .style(style::scrim),
        )
    }

    fn dl_ask_view<'a>(&'a self, a: &'a Ask) -> El<'a> {
        let summary = match &a.plan {
            None => "Checking what this includes\u{2026}".to_string(),
            Some(p) if p.items.is_empty() => "There is nothing to download here.".to_string(),
            Some(p) => {
                let n = p.items.len();
                let size = match (p.total, p.unknown) {
                    (0, _) => "size not reported".to_string(),
                    (b, 0) => download::bytes(b),
                    (b, _) => format!("at least {}", download::bytes(b)),
                };
                format!(
                    "{n} {} \u{b7} {size}",
                    if n == 1 { "file" } else { "files" }
                )
            }
        };
        let mut c = column![
            text(a.media.title.clone()).size(t::HEADING).font(t::bold()),
            text("Saves the original file(s) from the server into a folder of your local library, so they play without the server. Nothing on the server changes.")
                .size(t::CONTROL_TEXT)
                .line_height(1.5)
                .color(t::text_secondary()),
            mono(summary),
            mono("SAVE TO"),
        ]
        .spacing(t::S2);
        for (i, r) in a.roots.iter().enumerate() {
            let label = row![
                text(if i == a.sel { "\u{25cf}" } else { "\u{25cb}" }).size(t::BODY),
                text(r.display().to_string())
                    .size(t::BODY)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(t::S2);
            c = c.push(menu_row(
                label,
                i == a.sel,
                false,
                Message::Downloads(Msg::Pick(i)),
            ));
        }
        let ready = a.plan.as_ref().is_some_and(|p| !p.items.is_empty());
        c.push(
            row![
                space().width(Length::Fill),
                action_button(
                    "Cancel",
                    false,
                    false,
                    false,
                    false,
                    Message::Downloads(Msg::CancelAsk)
                ),
                action_button(
                    "Download",
                    true,
                    false,
                    ready,
                    !ready,
                    Message::Downloads(Msg::Confirm)
                ),
            ]
            .spacing(t::S2),
        )
        .into()
    }

    fn dl_list(&self) -> El<'_> {
        let jobs = &self.dl.queue.jobs;
        if jobs.is_empty() {
            return text("Nothing downloading. Use \u{201c}Download to this computer\u{2026}\u{201d} in a title's \u{2026} menu.")
                .size(t::BODY)
                .color(t::text_secondary())
                .into();
        }
        let mut list = column![].spacing(t::S3);
        for j in jobs {
            let speed = self
                .dl
                .run
                .as_ref()
                .filter(|r| r.id == j.id)
                .map_or(0.0, |r| r.speed);
            let (line, frac) = job_line(j, speed);
            let (pause, resume) = (
                matches!(j.state, JobState::Active | JobState::Queued),
                matches!(j.state, JobState::Paused | JobState::Failed(_)),
            );
            let mut btns = row![].spacing(t::S2);
            if pause {
                btns = btns.push(action_button(
                    "Pause",
                    false,
                    false,
                    false,
                    false,
                    Message::Downloads(Msg::Pause(j.id)),
                ));
            }
            if resume {
                btns = btns.push(action_button(
                    "Resume",
                    false,
                    false,
                    false,
                    false,
                    Message::Downloads(Msg::Resume(j.id)),
                ));
            }
            btns = btns.push(action_button(
                if j.state == JobState::Done {
                    "Remove"
                } else {
                    "Cancel"
                },
                false,
                false,
                false,
                false,
                Message::Downloads(Msg::Remove(j.id)),
            ));
            let mut body = column![
                text(j.meta.label())
                    .size(t::BODY)
                    .wrapping(text::Wrapping::None),
                mono(line).wrapping(text::Wrapping::None),
            ]
            .spacing(2)
            .width(Length::Fill);
            if let Some(f) = frac {
                body = body.push(progress_bar(f, 4.0));
            }
            list = list.push(row![body, btns].spacing(t::S3).align_y(Alignment::Center));
        }
        scroll(list)
            .style(style::scroll)
            .height(Length::Shrink)
            .into()
    }
}

/// The status line of a job and its progress fraction (while there is something to show).
fn job_line(j: &Job, speed: f64) -> (String, Option<f32>) {
    let frac = (j.total > 0).then(|| j.done as f32 / j.total as f32);
    let of = |p: &str| match j.total {
        0 => p.to_string(),
        t => format!(
            "{p} \u{b7} {} of {}",
            download::bytes(j.done),
            download::bytes(t)
        ),
    };
    match &j.state {
        JobState::Queued => ("Waiting".into(), None),
        JobState::Active if speed > 0.0 => (
            of(&format!("{}/s", download::bytes(speed as u64))).replace(" \u{b7} ", "  \u{b7}  "),
            frac,
        ),
        JobState::Active => ("Starting\u{2026}".into(), frac),
        JobState::Paused => (of("Paused"), frac),
        JobState::Done => (
            "Saved. It is in your library as another source.".into(),
            None,
        ),
        JobState::Failed(w) => (w.text().into(), frac),
    }
}
