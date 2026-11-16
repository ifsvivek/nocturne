//! Debug-only launcher for the detail page, so it can be run and screenshotted without the catalog.
//! Launch through `scripts/run-isolated.sh target/debug/nocturne` (never from inside the repo:
//! debug builds read the repo's `.env`).
//!
//! `NOCTURNE_PREVIEW=detail` plus one of:
//! - `NOCTURNE_PREVIEW_SERVER=http://127.0.0.1:8099 NOCTURNE_PREVIEW_ITEM=<id>
//!   NOCTURNE_PREVIEW_TOKEN_FILE=<test.env>`: an item of a throwaway server (loopback only, read-only
//!   GETs; the writes switch stays off unless `NOCTURNE_PREVIEW_WRITES=1`, and writes are never carried
//!   out here anyway). `NOCTURNE_PREVIEW_ALT_ITEM=<id>` adds that item as a second copy from a second
//!   "source" (the same server answering as source 2), to exercise the versions picker.
//! - `NOCTURNE_PREVIEW_LOCAL=fixture` (or `missing`, whose file is gone): a generated local library.
//!
//! `NOCTURNE_SNAP=/tmp/x.png` + `NOCTURNE_SNAP_AFTER_MS=1500,3000` write screenshots of the window,
//! `NOCTURNE_PREVIEW_KEYS="down@800,enter@1200"` injects named keys, `NOCTURNE_PREVIEW_SIZE=1920x1080`
//! sets the window.
use super::{Detail, Mark, Msg, Outcome};
use crate::{keys, theme};
use iced::{Color, Element, Size, Subscription, Task, window};
use nocturne_core::backend::{Backend, LocalBackend};
use nocturne_core::source::{Kind, Media, SourceId};
use std::time::Duration;

#[derive(Clone)]
enum M {
    D(Box<Msg>),
    Win(window::Id),
    Snap(usize),
    Shot(window::Screenshot, usize),
    Inject(String),
    Close,
    Resize(Size),
}

struct Setup {
    media: Media,
    alts: Vec<Media>,
    backends: Vec<(SourceId, Backend)>,
    marks: Vec<Mark>,
}

static SETUP: std::sync::OnceLock<Option<Setup>> = std::sync::OnceLock::new();

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

fn mark(o: u32, label: &str, color: Color) -> Mark {
    Mark {
        origin: SourceId(o),
        label: label.into(),
        color,
    }
}

fn gradient_png(path: &std::path::Path, w: u32, h: u32, hue: u8) {
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let f = y as f32 / h as f32;
            buf.extend_from_slice(&[
                (20.0 + 60.0 * f) as u8 + hue / 4,
                (30.0 + 50.0 * (x as f32 / w as f32)) as u8,
                (90.0 + 120.0 * f) as u8,
            ]);
        }
    }
    let _ = image::save_buffer(path, &buf, w, h, image::ColorType::Rgb8);
}

fn local_setup(mode: &str) -> Option<Setup> {
    let dir = std::env::temp_dir().join(format!("nocturne-detail-preview-{}", std::process::id()));
    let film = dir.join("media/Films/Local Demo (2021)");
    std::fs::create_dir_all(&film).ok()?;
    let file = film.join("Local Demo (2021).mkv");
    std::fs::write(&file, b"not really a video").ok()?;
    std::fs::write(
        film.join("Local Demo (2021).nfo"),
        "<movie><title>Local Demo</title><year>2021</year><runtime>96</runtime>\
         <plot>A film from a folder on this computer. Its description comes from the .nfo file next to it.</plot>\
         <genre>Drama</genre><genre>Mystery</genre></movie>",
    )
    .ok()?;
    gradient_png(&film.join("poster.png"), 400, 600, 40);
    gradient_png(&film.join("fanart.png"), 1600, 900, 120);
    let b = LocalBackend::open(SourceId(9), &dir.join("data")).ok()?;
    b.with_mut(|l| {
        l.scan(
            &[dir.join("media/Films")],
            &std::sync::atomic::AtomicBool::new(false),
            |_| {},
        )
    })
    .ok()?;
    if mode == "missing" {
        std::fs::remove_file(&file).ok()?;
    }
    let b = Backend::Local(b);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let m = rt.block_on(async {
        let q = nocturne_jellyfin::BrowseQuery {
            include_types: vec!["Movie".into()],
            recursive: true,
            ..Default::default()
        };
        b.browse(&q).await.ok()?.0.into_iter().next()
    })?;
    Some(Setup {
        media: m,
        alts: vec![],
        backends: vec![(SourceId(9), b)],
        marks: vec![mark(9, "LOCAL", theme::ACCENT_TEXT)],
    })
}

fn server_setup() -> Option<Setup> {
    let (url, item) = (
        env("NOCTURNE_PREVIEW_SERVER")?,
        env("NOCTURNE_PREVIEW_ITEM")?,
    );
    if !url.starts_with("http://127.0.0.1:") {
        eprintln!("PREVIEW: only a loopback server is allowed");
        return None;
    }
    let token = std::fs::read_to_string(env("NOCTURNE_PREVIEW_TOKEN_FILE")?)
        .ok()?
        .lines()
        .find_map(|l| {
            l.strip_prefix("NOCTURNE_JF_TEST_TOKEN=")
                .map(str::to_string)
        })?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(async {
        let connect = |o: u32| {
            let cfg = nocturne_jellyfin::Config {
                base_url: url.clone(),
                token: token.clone(),
            };
            async move {
                nocturne_core::source::Source::connect(Some(cfg))
                    .await
                    .ok()
                    .map(|s| Backend::Jellyfin(s.with_origin(SourceId(o))))
            }
        };
        let (b1, b2) = (connect(1).await?, connect(2).await?);
        let (m, _) = b1.full(&item).await.ok()?;
        let mut alts = vec![];
        let mut backends = vec![(SourceId(1), b1)];
        let mut marks = vec![mark(1, "HOME", theme::ACCENT_TEXT)];
        if let Some(alt) = env("NOCTURNE_PREVIEW_ALT_ITEM") {
            let (a, _) = b2.full(&alt).await.ok()?;
            alts.push(a);
            backends.push((SourceId(2), b2));
            marks.push(mark(2, "ARCHIVE", theme::WARN));
        }
        Some(Setup {
            media: m,
            alts,
            backends,
            marks,
        })
    })
}

fn boot() -> (Preview, Task<M>) {
    let s = SETUP
        .get()
        .and_then(|s| s.as_ref())
        .expect("NOCTURNE_PREVIEW_SERVER+ITEM+TOKEN_FILE, or NOCTURNE_PREVIEW_LOCAL");
    let writes = env("NOCTURNE_PREVIEW_WRITES").is_some();
    let (mut d, load) = Detail::new(
        s.media.clone(),
        s.alts.clone(),
        s.backends.clone(),
        size(),
        writes,
        s.marks.clone(),
    );
    let mut tasks = vec![
        load.map(|m| M::D(Box::new(m))),
        window::oldest().and_then(Task::done).map(M::Win),
    ];
    if s.media.kind == Kind::Photo {
        tasks.push(d.open_photo().map(|m| M::D(Box::new(m))));
    }
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
    (Preview { d, win: None }, Task::batch(tasks))
}

struct Preview {
    d: Detail,
    win: Option<window::Id>,
}

impl Preview {
    fn drain(&mut self) {
        for o in std::mem::take(&mut self.d.outcomes) {
            let what = match o {
                Outcome::Open(m) => format!("open {:?}", m.kind),
                Outcome::Play {
                    media, source_id, ..
                } => format!(
                    "play origin={} source_id={}",
                    media.origin.0,
                    source_id.as_deref().unwrap_or("-")
                ),
                Outcome::Browse { kind, .. } => format!("browse {kind}"),
                Outcome::Toast(s) => format!("toast {s}"),
                Outcome::Write(w) => format!("write {w:?}"),
                Outcome::Prefer(m) => format!("prefer origin={}", m.origin.0),
                Outcome::Queue { tracks, next } => format!("queue {} next={next}", tracks.len()),
                Outcome::OpenUrl(_) => "open-url".into(),
                Outcome::Back => "back".into(),
            };
            eprintln!("OUTCOME {what}");
        }
    }

    fn update(&mut self, m: M) -> Task<M> {
        let t = match m {
            M::D(msg) => self.d.update(*msg).map(|m| M::D(Box::new(m))),
            M::Win(id) => {
                self.win = Some(id);
                Task::none()
            }
            M::Inject(k) => {
                let key = keys::Key {
                    name: k.clone(),
                    ..Default::default()
                };
                self.d
                    .key(&key)
                    .map_or(Task::none(), |t| t.map(|m| M::D(Box::new(m))))
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
            M::Resize(s) => {
                self.d.set_win(s);
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
            iced::keyboard::listen().filter_map(|e| {
                keys::from_event(&e).map(|k| {
                    // The root forwards keys to `Detail::key`; the preview does the same.
                    M::Inject(k.name)
                })
            }),
            window::close_requests().map(|_| M::Close),
            window::resize_events().map(|(_, s)| M::Resize(s)),
        ])
    }

    fn view(&self) -> Element<'_, M> {
        iced::widget::stack![
            iced::widget::container(iced::widget::space())
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .style(crate::style::fill(theme::FRAME)),
            iced::widget::image(crate::widget::tex::glow())
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .content_fit(iced::ContentFit::Cover),
            self.d.view().map(|m| M::D(Box::new(m))),
            // Stand-in for the root's header and breadcrumb strip.
            iced::widget::container(
                iced::widget::text("HEADER  /  Home / Movies / ...")
                    .font(theme::MONO)
                    .size(theme::MICRO)
                    .color(theme::TEXT_TERTIARY)
            )
            .width(iced::Length::Fill)
            .height(theme::HEADER_H + 29.0)
            .padding([20.0, theme::PAGE_PAD])
            .style(crate::style::fill(theme::HEADER_BG)),
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
    let _ = SETUP.set(match env("NOCTURNE_PREVIEW_LOCAL") {
        Some(mode) => local_setup(&mode),
        None => server_setup(),
    });
    iced::application(boot, Preview::update, Preview::view)
        .title("NOCTURNE detail preview")
        .subscription(Preview::subscription)
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme::TEXT,
        })
        .theme(|_: &Preview| iced::Theme::Dark)
        .default_font(theme::FONT)
        .exit_on_close_request(false)
        .window(window::Settings {
            size: size(),
            min_size: Some(Size::new(800.0, 560.0)),
            ..Default::default()
        })
        .run()
}
