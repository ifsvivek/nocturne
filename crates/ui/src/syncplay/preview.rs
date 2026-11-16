//! Debug-only launcher for Watch together (`NOCTURNE_PREVIEW=syncplay`): one signed-in client of a
//! throwaway server, with a player page that follows the group. Start two instances (two users,
//! different `NOCTURNE_DEVICE_ID`) to try a real group. Launch through
//! `scripts/run-isolated.sh target/debug/nocturne-iced` (never from inside the repo).
//!
//! - `NOCTURNE_PREVIEW_SERVER=http://127.0.0.1:8100 NOCTURNE_PREVIEW_TOKEN_FILE=<test.env>
//!   NOCTURNE_PREVIEW_USER=alice`: signs that user in (password from `NOCTURNE_JF_TEST_<USER>_PW` in
//!   the file; loopback only; the password is never printed).
//! - `NOCTURNE_PREVIEW_WRITES=1`: server changes on (create / join / transport need it).
//! - `NOCTURNE_PREVIEW_ITEM=<item id>`: key `p` starts it for the group.
//! - `NOCTURNE_PREVIEW_SERVER2` / `NOCTURNE_PREVIEW_TOKEN_FILE2` add a second server (picker).
//! - Keys in the window: `g` opens the screen (while no player is up), the screen's own keys, and
//!   the player's keys otherwise. `NOCTURNE_PREVIEW_KEYS="g@500,tab@900,f@1000"` injects keys,
//!   `NOCTURNE_SNAP=/tmp/x.png` (+ `NOCTURNE_SNAP_AFTER_MS`) writes window screenshots and quits.
//!   Group traffic is logged as `SP ...` lines (no tokens).
use super::{Msg, Outcome, SyncPlay, Target};
use crate::player::{self, Player, PlayerCtx};
use crate::{keys, theme};
use iced::{Color, Element, Size, Subscription, Task, window};
use nocturne_core::backend::Backend;
use nocturne_core::source::{Media, Source, SourceId};
use nocturne_core::video::PlayerPrefs;
use nocturne_jellyfin::Config;
use nocturne_jellyfin_user::{Session, UserClient};
use nocturne_player::Buffering;
use std::time::Duration;

#[derive(Clone)]
enum M {
    S(Msg),
    P(player::Msg),
    Win(window::Id),
    Snap(usize),
    Shot(window::Screenshot, usize),
    Inject(String),
    /// The context for the player of the group's item (or nothing: it could not be built).
    Ctx(Option<Box<PlayerCtx>>, f64),
    Close,
}

struct Setup {
    targets: Vec<Target>,
    item: Option<Media>,
    writes: bool,
}

static SETUP: std::sync::OnceLock<Option<Setup>> = std::sync::OnceLock::new();

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn read_env(file: &str, key: &str) -> Option<String> {
    std::fs::read_to_string(file)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
}

async fn target(url: String, file: String, origin: u32, label: &str) -> Option<Target> {
    if !url.starts_with("http://127.0.0.1:") {
        eprintln!("PREVIEW: only a loopback server is allowed");
        return None;
    }
    let user = env("NOCTURNE_PREVIEW_USER").unwrap_or_else(|| "alice".into());
    let pw = read_env(
        &file,
        &format!("NOCTURNE_JF_TEST_{}_PW", user.to_uppercase()),
    )?;
    let s: Session = UserClient::authenticate_by_name(&url, &user, &pw)
        .await
        .ok()?;
    let cfg = Config {
        base_url: url.clone(),
        token: s.access_token.clone(),
    };
    Some(Target {
        origin: SourceId(origin),
        label: label.into(),
        client: UserClient::from_session(&url, &s).ok()?,
        user_name: s.user_name.clone(),
        source: Source::with_user(cfg, &s.user_id)
            .ok()?
            .with_origin(SourceId(origin)),
    })
}

fn setup() -> Option<Setup> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(async {
        let mut targets = vec![
            target(
                env("NOCTURNE_PREVIEW_SERVER")?,
                env("NOCTURNE_PREVIEW_TOKEN_FILE")?,
                1,
                "Home",
            )
            .await?,
        ];
        if let (Some(u), Some(f)) = (
            env("NOCTURNE_PREVIEW_SERVER2"),
            env("NOCTURNE_PREVIEW_TOKEN_FILE2"),
        ) && let Some(t) = target(u, f, 2, "Archive").await
        {
            targets.push(t);
        }
        let item = match env("NOCTURNE_PREVIEW_ITEM") {
            Some(id) => Backend::Jellyfin(targets[0].source.clone())
                .full(&id)
                .await
                .ok()
                .map(|(m, _)| m),
            None => None,
        };
        Some(Setup {
            targets,
            item,
            writes: env("NOCTURNE_PREVIEW_WRITES").is_some(),
        })
    })
}

struct Preview {
    sp: SyncPlay,
    player: Option<Player>,
    item: Option<Media>,
    backends: Vec<(SourceId, Backend)>,
    win: Option<window::Id>,
    size: Size,
}

fn boot() -> (Preview, Task<M>) {
    let s = SETUP
        .get()
        .and_then(|s| s.as_ref())
        .expect("NOCTURNE_PREVIEW_SERVER + TOKEN_FILE + USER (loopback), see syncplay/preview.rs");
    let backends = s
        .targets
        .iter()
        .map(|t| (t.origin, Backend::Jellyfin(t.source.clone())))
        .collect();
    let mut sp = SyncPlay::new(s.targets.clone(), false);
    let _ = sp.set_writes(s.writes, None);
    let mut tasks = vec![window::oldest().and_then(Task::done).map(M::Win)];
    if env("NOCTURNE_PREVIEW_OPEN").as_deref() != Some("0") {
        tasks.push(sp.update(Msg::OpenScreen, None).map(M::S));
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
    (
        Preview {
            sp,
            player: None,
            item: s.item.clone(),
            backends,
            win: None,
            size: Size::new(1280.0, 800.0),
        },
        Task::batch(tasks),
    )
}

impl Preview {
    fn ctx(&self, origin: SourceId, list: Vec<Media>, idx: usize) -> Task<Option<PlayerCtx>> {
        let b = self
            .backends
            .iter()
            .find(|(o, _)| *o == origin)
            .map(|(_, b)| b.clone());
        let backends = self.backends.clone();
        let m = list[idx].clone();
        crate::task::run(
            async move {
                let b = b.or_else(|| {
                    eprintln!("SP no backend for origin {}", m.origin.0);
                    None
                })?;
                let target = match b.play_target(&m).await {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("SP play_target failed: {e:?} kind={:?} id={}", m.kind, m.id);
                        return None;
                    }
                };
                Some(PlayerCtx {
                    queue: list,
                    backends,
                    media: m,
                    target,
                    source_id: None,
                    resume: None,
                    hooks: None,
                    prefs: PlayerPrefs::default(),
                    buffering: Buffering::default(),
                })
            },
            |c| c,
        )
    }

    /// What the screen asked for.
    fn drain(&mut self) -> Task<M> {
        let mut tasks = vec![];
        for o in std::mem::take(&mut self.sp.outcomes) {
            match o {
                Outcome::Toast(s) => eprintln!("SP toast: {s}"),
                Outcome::OpenSettings => eprintln!("SP open-settings"),
                Outcome::OpenPlayer {
                    origin,
                    list,
                    idx,
                    start,
                } => {
                    eprintln!("SP open-player idx={idx} start={start:.1}");
                    tasks.push(
                        self.ctx(origin, list, idx)
                            .map(move |c| M::Ctx(c.map(Box::new), start)),
                    );
                }
                Outcome::Player(t) => tasks.push(t.map(M::P)),
                Outcome::ClosePlayer => {
                    eprintln!("SP group stop");
                    if let Some(p) = &mut self.player {
                        tasks.push(p.close(false).map(M::P));
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    fn after_player(&mut self, t: Task<player::Msg>) -> Task<M> {
        if let Some(p) = &mut self.player
            && let Some(player::Outcome::Closed { failed }) = p.take_outcome()
        {
            eprintln!("SP player closed failed={failed}");
            self.player = None;
            self.sp.player_closed();
        }
        Task::batch([t.map(M::P), self.drain()])
    }

    fn update(&mut self, m: M) -> Task<M> {
        match m {
            M::S(msg) => {
                let t = self.sp.update(msg, self.player.as_mut());
                Task::batch([t.map(M::S), self.drain()])
            }
            M::P(msg) => match &mut self.player {
                Some(p) => {
                    let t = p.update(msg);
                    self.after_player(t)
                }
                None => Task::none(),
            },
            M::Win(id) => {
                self.win = Some(id);
                Task::none()
            }
            M::Ctx(c, start) => match c {
                Some(c) => {
                    let (p, open) = Player::open_group(*c, start);
                    self.player = Some(p);
                    open.map(M::P)
                }
                None => {
                    eprintln!("SP could not open the group's item");
                    Task::none()
                }
            },
            M::Inject(k) => {
                let key = keys::Key {
                    text: (k.chars().count() == 1).then(|| k.clone()),
                    name: k,
                    ..Default::default()
                };
                self.key(key)
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
            M::Close => self.quit(),
        }
    }

    fn key(&mut self, k: keys::Key) -> Task<M> {
        if self.sp.is_open() {
            let t = self.sp.key(&k, self.player.as_mut());
            return Task::batch([t.map(M::S), self.drain()]);
        }
        match (k.name.as_str(), self.player.as_mut()) {
            ("g", _) => {
                let t = self.sp.update(Msg::OpenScreen, self.player.as_mut());
                t.map(M::S)
            }
            ("p", _) if self.item.is_some() => {
                let m = self.item.clone().expect("checked");
                let t = if self.sp.in_group() {
                    self.sp.play(m, vec![])
                } else {
                    eprintln!("SP not in a group: p does nothing");
                    Task::none()
                };
                Task::batch([t.map(M::S), self.drain()])
            }
            (_, Some(p)) => {
                let t = p.update(player::Msg::Key(k));
                self.after_player(t)
            }
            _ => Task::none(),
        }
    }

    fn quit(&mut self) -> Task<M> {
        self.sp.shutdown(self.player.as_mut());
        if let Some(p) = &mut self.player {
            p.shutdown();
        }
        match self.win {
            Some(id) => window::close(id).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    fn subscription(&self) -> Subscription<M> {
        Subscription::batch([
            self.sp.subscription().map(M::S),
            self.player
                .as_ref()
                .map_or_else(Subscription::none, |p| p.subscription().map(M::P)),
            iced::keyboard::listen()
                .filter_map(|e| keys::from_event(&e).map(|k| M::Inject(k.name))),
            window::close_requests().map(|_| M::Close),
        ])
    }

    fn view(&self) -> Element<'_, M> {
        let base: Element<M> = match &self.player {
            Some(p) => p.view().map(M::P),
            None => iced::widget::container(
                iced::widget::text("Watch together preview")
                    .font(theme::MONO)
                    .size(theme::MICRO)
                    .color(theme::TEXT_TERTIARY),
            )
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .padding(theme::S6)
            .style(crate::style::fill(theme::FRAME))
            .into(),
        };
        let mut layers = vec![base];
        if let Some(pill) = self.sp.pill() {
            layers.push(
                iced::widget::container(pill.map(M::S))
                    .padding(theme::S4)
                    .align_x(iced::Alignment::End)
                    .width(iced::Length::Fill)
                    .into(),
            );
        }
        if let Some(o) = self.sp.view(self.size) {
            layers.push(o.map(M::S));
        }
        iced::widget::stack(layers).into()
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
        .title("NOCTURNE watch together preview")
        .subscription(Preview::subscription)
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme::TEXT,
        })
        .theme(|_: &Preview| iced::Theme::Dark)
        .default_font(theme::FONT)
        .exit_on_close_request(false)
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(800.0, 560.0)),
            transparent: true,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "nocturne".into(),
                ..Default::default()
            },
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
