//! Debug-only launcher for the admin screen (`NOCTURNE_PREVIEW=admin`). Launch through
//! `scripts/run-isolated.sh target/debug/nocturne` (never from inside the repo).
//!
//! - `NOCTURNE_PREVIEW_SERVER=http://127.0.0.1:8100 NOCTURNE_PREVIEW_TOKEN_FILE=<test.env>`: the
//!   admin API key of a throwaway server (loopback only). `NOCTURNE_PREVIEW_SERVER2` /
//!   `NOCTURNE_PREVIEW_TOKEN_FILE2` add a second server, so the source picker shows.
//! - The screen is view only: this preview never turns server changes on for a real client.
//! - `NOCTURNE_PREVIEW_FAKE=1`: no server at all; the client points at a closed loopback port and
//!   server changes are ON, to look at the confirmation dialog (a confirmed write fails at once,
//!   nothing is ever sent anywhere).
//! - `NOCTURNE_SNAP=/tmp/x.png` (+ `NOCTURNE_SNAP_AFTER_MS=1500,3000`) writes screenshots of the
//!   window and quits, `NOCTURNE_PREVIEW_KEYS="down@800,right@1200,enter@1600"` injects named keys
//!   (a single character types it), `NOCTURNE_PREVIEW_SIZE=1920x1080` sets the window.
use super::{Admin, Msg, Outcome, Target};
use crate::{keys, theme};
use iced::{Color, Element, Size, Subscription, Task, window};
use nocturne_core::source::SourceId;
use nocturne_jellyfin::Config;
use std::time::Duration;

#[derive(Clone)]
enum M {
    A(Msg),
    Win(window::Id),
    Snap(usize),
    Shot(window::Screenshot, usize),
    Inject(String),
    Close,
}

static SETUP: std::sync::OnceLock<Option<(Vec<Target>, bool)>> = std::sync::OnceLock::new();

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

fn target(url_var: &str, file_var: &str, origin: u32, label: &str) -> Option<Target> {
    let url = env(url_var)?;
    if !url.starts_with("http://127.0.0.1:") {
        eprintln!("PREVIEW: only a loopback server is allowed");
        return None;
    }
    let token = std::fs::read_to_string(env(file_var)?)
        .ok()?
        .lines()
        .find_map(|l| {
            l.strip_prefix("NOCTURNE_JF_TEST_TOKEN=")
                .map(str::to_string)
        })?;
    Some(Target {
        origin: SourceId(origin),
        label: label.into(),
        cfg: Config {
            base_url: url,
            token,
        },
        pics: None,
    })
}

fn setup() -> Option<(Vec<Target>, bool)> {
    if env("NOCTURNE_PREVIEW_FAKE").is_some() {
        let t = Target {
            origin: SourceId(1),
            label: "Fake".into(),
            cfg: Config {
                base_url: "http://127.0.0.1:1".into(),
                token: "fake".into(),
            },
            pics: None,
        };
        return Some((vec![t], true));
    }
    let a = target(
        "NOCTURNE_PREVIEW_SERVER",
        "NOCTURNE_PREVIEW_TOKEN_FILE",
        1,
        "Home",
    )?;
    let mut v = vec![a];
    v.extend(target(
        "NOCTURNE_PREVIEW_SERVER2",
        "NOCTURNE_PREVIEW_TOKEN_FILE2",
        2,
        "Archive",
    ));
    Some((v, false))
}

struct Preview {
    a: Admin,
    win: Option<window::Id>,
}

fn boot() -> (Preview, Task<M>) {
    let (targets, writes) = SETUP
        .get()
        .and_then(|s| s.clone())
        .expect("NOCTURNE_PREVIEW_SERVER + NOCTURNE_PREVIEW_TOKEN_FILE, or NOCTURNE_PREVIEW_FAKE");
    let fake = env("NOCTURNE_PREVIEW_FAKE").is_some();
    let (a, load) = if fake {
        let t = targets[0].clone();
        let c = nocturne_jellyfin_admin::AdminClient::new(t.cfg.clone()).expect("client");
        Admin::with_client(t, c, writes)
    } else {
        Admin::open(targets, writes)
    };
    let mut tasks = vec![
        load.map(M::A),
        window::oldest().and_then(Task::done).map(M::Win),
    ];
    if env("NOCTURNE_SNAP").is_some() {
        let ms: Vec<u64> = env("NOCTURNE_SNAP_AFTER_MS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        for (n, ms) in if ms.is_empty() { vec![2500] } else { ms }
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
    (Preview { a, win: None }, Task::batch(tasks))
}

impl Preview {
    fn drain(&mut self) {
        for o in std::mem::take(&mut self.a.outcomes) {
            eprintln!(
                "OUTCOME {}",
                match o {
                    Outcome::Close => "close",
                    Outcome::OpenSettings => "open-settings",
                }
            );
        }
    }

    fn update(&mut self, m: M) -> Task<M> {
        let t = match m {
            M::A(msg) => self.a.update(msg).map(M::A),
            M::Win(id) => {
                self.win = Some(id);
                Task::none()
            }
            M::Inject(k) => {
                let key = keys::Key {
                    text: (k.chars().count() == 1).then(|| k.clone()),
                    name: k,
                    ..Default::default()
                };
                self.a.key(&key).map(M::A)
            }
            M::Snap(n) => match self.win {
                Some(id) => window::screenshot(id).map(move |s| M::Shot(s, n)),
                None => Task::none(),
            },
            M::Shot(s, n) => {
                save_png(&s, n);
                if n + 1 >= snap_count() {
                    return self.quit();
                }
                Task::none()
            }
            M::Close => return self.quit(),
        };
        self.drain();
        t
    }

    fn quit(&self) -> Task<M> {
        match self.win {
            Some(id) => window::close(id).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    fn subscription(&self) -> Subscription<M> {
        Subscription::batch([
            iced::keyboard::listen()
                .filter_map(|e| keys::from_event(&e).map(|k| M::Inject(k.name))),
            window::close_requests().map(|_| M::Close),
        ])
    }

    fn view(&self) -> Element<'_, M> {
        iced::widget::stack![
            iced::widget::container(iced::widget::space())
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .style(crate::style::fill(theme::frame())),
            iced::widget::image(crate::widget::tex::glow())
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .content_fit(iced::ContentFit::Cover),
            self.a.view().map(M::A),
            // Stand-in for the root's header and breadcrumb strip.
            iced::widget::container(
                iced::widget::text("HEADER  /  Home / Admin")
                    .font(theme::MONO)
                    .size(theme::MICRO)
                    .color(theme::text_tertiary())
            )
            .width(iced::Length::Fill)
            .height(theme::HEADER_H + 29.0)
            .padding([20.0, theme::PAGE_PAD])
            .style(crate::style::fill(theme::header_bg())),
        ]
        .into()
    }
}

fn snap_count() -> usize {
    env("NOCTURNE_SNAP_AFTER_MS").map_or(1, |s| s.split(',').count().max(1))
}

fn save_png(s: &window::Screenshot, n: usize) {
    let Some(base) = env("NOCTURNE_SNAP") else {
        return;
    };
    let p = std::path::Path::new(&base);
    let path = if n == 0 {
        p.to_path_buf()
    } else {
        let stem = p
            .file_stem()
            .map_or_else(String::new, |x| x.to_string_lossy().into());
        p.with_file_name(format!("{stem}-{}.png", n + 1))
    };
    let _ = image::save_buffer(
        &path,
        &s.rgba,
        s.size.width,
        s.size.height,
        image::ColorType::Rgba8,
    );
    eprintln!("SNAP written {}", path.display());
}

pub fn run() -> iced::Result {
    let _ = SETUP.set(setup());
    iced::application(boot, Preview::update, Preview::view)
        .title("NOCTURNE admin preview")
        .subscription(Preview::subscription)
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
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
