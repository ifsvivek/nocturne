//! Debug-only launcher for the walkthrough, so every step can be looked at and captured without a
//! catalog. Launch through `scripts/run-isolated.sh target/debug/nocturne` (never from inside
//! the repo).
//!
//! `NOCTURNE_PREVIEW=onboarding` plus any of:
//! - `NOCTURNE_PREVIEW_MODE=upgrade`: the two-step "What's new in 0.3" path.
//! - `NOCTURNE_PREVIEW_STEP=where|setup|behave|ready|news2` and `NOCTURNE_PREVIEW_CHOICE=local|server|both`:
//!   start on that step.
//! - `NOCTURNE_PREVIEW_WORLD=demo`: three folders (indexing), one connected server.
//! - `NOCTURNE_PREVIEW_EXTERNAL_VIDEO=1`: show the external-mpv playback line.
//! - `NOCTURNE_PREVIEW_KEYS="right@600,enter@900"` injects named keys; `NOCTURNE_PREVIEW_SIZE=1280x800`.
//!
//! Outcomes are answered the way the root will: AddFolders / AddServer add a stand-in folder / server
//! and call `returned`; SetWrites / SetSync flip the world; Finish prints it and exits.
//! `NOCTURNE_SNAP=/tmp/x.png` + `NOCTURNE_SNAP_AFTER_MS=1500,3000` write window screenshots
//! (`x.png`, `x-2.png`, ...), through Iced's own `window::screenshot`.
use super::{Mode, Msg, Onboarding, Outcome, Return, Server, Step, World};
use crate::{keys, theme};
use iced::widget::{container, image, space, stack};
use iced::{Element, Length, Size, Subscription, Task, window};
use nocturne_core::sources::FirstRun;
use std::time::{Duration, Instant};

#[derive(Clone)]
enum M {
    Ob(Msg),
    Win(window::Id),
    Snap(usize),
    Shot(window::Screenshot, usize),
    Inject(String),
    Frame(Instant),
    Tick,
    Close,
    Resize(Size),
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn size() -> Size {
    env("NOCTURNE_PREVIEW_SIZE")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(Size::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(Size::new(1280.0, 800.0))
}

struct Preview {
    o: Onboarding,
    win: Option<window::Id>,
    now: Instant,
}

fn boot() -> (Preview, Task<M>) {
    let mut world = World {
        embedded_video: env("NOCTURNE_PREVIEW_EXTERNAL_VIDEO").is_none(),
        ..World::default()
    };
    if env("NOCTURNE_PREVIEW_WORLD").as_deref() == Some("demo") {
        world.folders = vec!["Films".into(), "Series".into(), "Music".into()];
        world.indexing = Some(1240);
        world.servers = vec![Server {
            name: "Living room".into(),
            user: "ana".into(),
        }];
    }
    let mode = if env("NOCTURNE_PREVIEW_MODE").as_deref() == Some("upgrade") {
        Mode::Upgrade
    } else {
        Mode::Fresh
    };
    let mut o = Onboarding::new(mode, world);
    o.set_win(size());
    o.choice = match env("NOCTURNE_PREVIEW_CHOICE").as_deref() {
        Some("local") => Some(FirstRun::Local),
        Some("both") => Some(FirstRun::Both),
        Some("server") => Some(FirstRun::Server),
        _ => None,
    };
    let step = match env("NOCTURNE_PREVIEW_STEP").as_deref() {
        Some("where") => Some(Step::Where),
        Some("setup") => Some(Step::SetUp),
        Some("behave") => Some(Step::Behave),
        Some("ready") => Some(Step::Ready),
        Some("news2") => Some(Step::News2),
        _ => None,
    };
    if let Some(s) = step {
        o.choice.get_or_insert(FirstRun::Both);
        o.step = s;
        o.focus = o.default_focus();
    }
    let mut tasks = vec![window::oldest().and_then(|id| Task::done(M::Win(id)))];
    if env("NOCTURNE_SNAP").is_some() {
        let ms: Vec<u64> = env("NOCTURNE_SNAP_AFTER_MS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        for (n, ms) in if ms.is_empty() { vec![1500] } else { ms }
            .into_iter()
            .enumerate()
        {
            tasks.push(crate::task::after(Duration::from_millis(ms), M::Snap(n)));
        }
    }
    for k in env("NOCTURNE_PREVIEW_KEYS").unwrap_or_default().split(',') {
        if let Some((key, ms)) = k
            .split_once('@')
            .and_then(|(a, b)| Some((a, b.parse().ok()?)))
        {
            tasks.push(crate::task::after(
                Duration::from_millis(ms),
                M::Inject(key.into()),
            ));
        }
    }
    (
        Preview {
            o,
            win: None,
            now: Instant::now(),
        },
        Task::batch(tasks),
    )
}

fn snap_count() -> usize {
    env("NOCTURNE_SNAP_AFTER_MS").map_or(1, |s| s.split(',').count().max(1))
}

impl Preview {
    /// Answer outcomes like the root: fake folder / server, then `returned`.
    fn drain(&mut self) -> Task<M> {
        for out in std::mem::take(&mut self.o.outcomes) {
            eprintln!("OUTCOME {out:?}");
            let mut w = self.o.world.clone();
            match out {
                Outcome::AddFolders => {
                    w.folders.push(format!("Folder {}", w.folders.len() + 1));
                    w.indexing = Some(0);
                    self.o.set_world(w);
                    self.o.returned(Return::Folders);
                }
                Outcome::AddServer => {
                    w.servers.push(Server {
                        name: format!("Server {}", w.servers.len() + 1),
                        user: "ana".into(),
                    });
                    self.o.set_world(w);
                    self.o.returned(Return::Server);
                }
                Outcome::SetWrites(_) | Outcome::SetSync(_) => {}
                Outcome::Finish { .. } => return self.quit(),
            }
        }
        Task::none()
    }

    fn update(&mut self, m: M) -> Task<M> {
        match m {
            M::Ob(msg) => {
                self.o.update(msg);
                return self.drain();
            }
            M::Win(id) => self.win = Some(id),
            M::Inject(k) => {
                let key = keys::Key {
                    name: k,
                    ..Default::default()
                };
                self.o.key(&key);
                return self.drain();
            }
            M::Frame(n) => self.now = n,
            M::Tick => {
                self.now = Instant::now();
                // A stand-in index run: files climb while the setup step is showing.
                let mut w = self.o.world.clone();
                if let Some(n) = w.indexing {
                    w.indexing = Some(n + 37);
                    self.o.set_world(w);
                }
            }
            M::Snap(n) => {
                if let Some(id) = self.win {
                    return window::screenshot(id).map(move |s| M::Shot(s, n));
                }
            }
            M::Shot(s, n) => {
                crate::snap::save(&s, n);
                if n + 1 >= snap_count() {
                    return self.quit();
                }
            }
            M::Resize(s) => self.o.set_win(s),
            M::Close => return self.quit(),
        }
        Task::none()
    }

    fn quit(&self) -> Task<M> {
        match self.win {
            Some(id) => window::close(id).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    fn subscription(&self) -> Subscription<M> {
        let mut subs = vec![
            iced::keyboard::listen().filter_map(|e| {
                keys::from_event(&e).map(|k| {
                    // The root forwards keys to `Onboarding::key`; so does the preview.
                    M::Inject(k.name)
                })
            }),
            window::close_requests().map(|_| M::Close),
            window::resize_events().map(|(_, s)| M::Resize(s)),
            iced::time::every(Duration::from_millis(250)).map(|_| M::Tick),
        ];
        if self.o.animating(self.now) {
            subs.push(window::frames().map(M::Frame));
        }
        Subscription::batch(subs)
    }

    fn view(&self) -> Element<'_, M> {
        // Stand-in for the root's backdrop.
        stack![
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(crate::style::fill(theme::frame())),
            image(crate::widget::tex::dots())
                .width(Length::Fill)
                .height(Length::Fill)
                .opacity(0.16_f32)
                .content_fit(iced::ContentFit::Cover),
            image(crate::widget::tex::glow())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Cover),
            self.o.view(self.now).map(M::Ob),
        ]
        .into()
    }
}

pub fn run() -> iced::Result {
    iced::application(boot, Preview::update, Preview::view)
        .title("NOCTURNE onboarding preview")
        .subscription(Preview::subscription)
        .style(|_, _| iced::theme::Style {
            background_color: iced::Color::TRANSPARENT,
            text_color: theme::text(),
        })
        .theme(|_: &Preview| iced::Theme::Dark)
        .default_font(theme::base())
        .exit_on_close_request(false)
        .window(window::Settings {
            size: size(),
            min_size: Some(Size::new(800.0, 560.0)),
            ..Default::default()
        })
        .run()
}
