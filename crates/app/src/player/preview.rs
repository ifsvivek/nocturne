//! Debug-only launcher for the player page, so it can be run and screenshotted without the
//! catalog: `NOCTURNE_PREVIEW=player NOCTURNE_PREVIEW_FILE=/tmp/clip.mp4` plays a local file
//! (`PlayTarget::File`); `NOCTURNE_PREVIEW_URL=<stream url>` plays a stream from a throwaway
//! server (a `NOCTURNE_PREVIEW_HEADER="Authorization: ..."` line is sent with it).
//! `NOCTURNE_PREVIEW_SECOND` adds a second queue entry (a file) for the queue panel.
//! `NOCTURNE_PREVIEW_PROMPT=1` offers a resume prompt, `NOCTURNE_PREVIEW_QUIT_AFTER_MS` closes
//! the page after a delay (the exit path, for open/close cycling).
//! `NOCTURNE_SNAP=/tmp/x.png` (+ `NOCTURNE_SNAP_AFTER_MS`, see snap.rs) writes screenshots of the app
//! window (the video subsurface is not part of them) and quits after the last.
//! `NOCTURNE_PREVIEW_KEYS="key@ms,key@ms"` injects named keys (`space@1000,i@2000`; `shift+c@500`).
//! `NOCTURNE_PREVIEW_HDR=ask|window|popout` sets the HDR video setting; `NOCTURNE_HDR_DISPLAY=yes|no`
//! stands in for the display's HDR capability (see `Player::hdr_capable`).
use super::{Msg, Outcome, Player, PlayerCtx};
use crate::{keys, theme};
use iced::{Color, Element, Size, Subscription, Task, window};
use nocturne_core::backend::PlayTarget;
use nocturne_core::hdr::HdrMode;
use nocturne_core::source::Media;
use nocturne_core::video::PlayerPrefs;
use nocturne_player::Buffering;
use std::time::Duration;

#[derive(Clone)]
enum M {
    P(Msg),
    Snap(usize),
    Shot(window::Screenshot, usize),
    Inject(String),
    Probe,
    Quit,
    Close,
}

struct Preview {
    player: Player,
    win: Option<window::Id>,
    cycles: u32,
}

fn media(name: &str) -> Media {
    Media::from_item(&nocturne_jellyfin::Item {
        id: name.into(),
        name: name.into(),
        kind: "Movie".into(),
        ..Default::default()
    })
}

/// `NOCTURNE_PREVIEW_SERVER=http://127.0.0.1:8097 NOCTURNE_PREVIEW_ITEM=<id>
/// NOCTURNE_PREVIEW_TOKEN_FILE=<test.env>`: play an item of the throwaway demo server through
/// the real backend (read-only GETs; the developer-token source never writes). Only loopback
/// addresses are accepted. Built once in `run`, before the event loop exists.
static SERVER: std::sync::OnceLock<Option<PlayerCtx>> = std::sync::OnceLock::new();

fn server_ctx() -> Option<PlayerCtx> {
    use nocturne_core::backend::Backend;
    let env = |k: &str| std::env::var(k).ok();
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
        let src = nocturne_core::source::Source::connect(Some(nocturne_jellyfin::Config {
            base_url: url,
            token,
        }))
        .await
        .ok()?;
        let b = Backend::Jellyfin(src);
        let (m, _) = b.full(&item).await.ok()?;
        let target = b.play_target(&m).await.ok()?;
        Some(PlayerCtx {
            queue: vec![m.clone()],
            backends: vec![(m.origin, b)],
            media: m,
            target,
            source_id: None,
            resume: None,
            hooks: None,
            prefs: PlayerPrefs::default(),
            buffering: Buffering::default(),
        })
    })
}

fn ctx() -> Option<PlayerCtx> {
    if let Some(c) = SERVER.get().and_then(|c| c.clone()) {
        return Some(c);
    }
    let env = |k: &str| std::env::var(k).ok();
    let (target, title) = match (env("NOCTURNE_PREVIEW_FILE"), env("NOCTURNE_PREVIEW_URL")) {
        (Some(f), _) => (PlayTarget::File(f.clone().into()), f),
        (None, Some(u)) => (
            PlayTarget::Url {
                url: u,
                headers: env("NOCTURNE_PREVIEW_HEADER").into_iter().collect(),
            },
            "Stream".into(),
        ),
        _ => return None,
    };
    let title = std::path::Path::new(&title)
        .file_stem()
        .map_or(title.clone(), |s| s.to_string_lossy().into_owned());
    let mut m = media(&title);
    if env("NOCTURNE_PREVIEW_PROMPT").is_some() {
        m.resume_s = Some(30.0);
        m.duration_s = Some(600.0);
    }
    let mut queue = vec![m.clone()];
    if let Some(s) = env("NOCTURNE_PREVIEW_SECOND") {
        queue.push(media(&s));
    }
    Some(PlayerCtx {
        media: m,
        queue,
        target,
        source_id: None,
        resume: None,
        backends: vec![],
        hooks: None,
        prefs: PlayerPrefs {
            hdr: match env("NOCTURNE_PREVIEW_HDR").as_deref() {
                Some("window") => HdrMode::Window,
                Some("popout") => HdrMode::Popout,
                _ => HdrMode::Ask,
            },
            ..Default::default()
        },
        buffering: Buffering::default(),
    })
}

fn boot() -> (Preview, Task<M>) {
    let (player, open) =
        Player::open(ctx().expect("NOCTURNE_PREVIEW_FILE or NOCTURNE_PREVIEW_URL"));
    let env = |k: &str| std::env::var(k).ok();
    let mut tasks = vec![open.map(M::P)];
    // The one snapshot hook (snap.rs): same variables and file naming as the app.
    if env("NOCTURNE_SNAP").is_some() {
        for (n, ms) in crate::snap::delays().into_iter().enumerate() {
            tasks.push(crate::task::after(Duration::from_millis(ms), M::Snap(n)));
        }
    }
    if let Some(ms) = env("NOCTURNE_PREVIEW_QUIT_AFTER_MS").and_then(|v| v.parse().ok()) {
        tasks.push(crate::task::after(Duration::from_millis(ms), M::Quit));
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
    let cycles = env("NOCTURNE_PREVIEW_CYCLES")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    (
        Preview {
            player,
            win: None,
            cycles,
        },
        Task::batch(tasks),
    )
}

/// Resident set size of this process in kB (QA: leak check across open/close cycles).
fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("VmRSS:")?
                    .trim()
                    .strip_suffix("kB")?
                    .trim()
                    .parse()
                    .ok()
            })
        })
        .unwrap_or(0)
}

impl Preview {
    fn update(&mut self, m: M) -> Task<M> {
        match m {
            M::P(msg) => {
                if let Msg::Window(w) = &msg {
                    self.win = *w;
                }
                let t = self.player.update(msg);
                self.after(t)
            }
            M::Inject(k) => {
                use super::{Act, Pop};
                let msg = match k.as_str() {
                    "pop:subs" => Msg::Act(Act::Pop(Pop::Subs)),
                    "pop:audio" => Msg::Act(Act::Pop(Pop::Audio)),
                    "pop:settings" => Msg::Act(Act::Pop(Pop::Settings)),
                    "ctx" => Msg::Act(Act::VideoRight),
                    "hover" => Msg::Seek(crate::widget::track::Ev::Hover(Some(0.4))),
                    _ => Msg::Key(match k.strip_prefix("shift+") {
                        Some(n) => keys::Key {
                            name: n.into(),
                            shift: true,
                            ..Default::default()
                        },
                        None => keys::Key {
                            name: k,
                            ..Default::default()
                        },
                    }),
                };
                let t = self.player.update(msg);
                self.after(t)
            }
            M::Snap(n) => match self.win {
                Some(id) => window::screenshot(id).map(move |s| M::Shot(s, n)),
                None => Task::none(),
            },
            M::Shot(s, n) => {
                crate::snap::save(&s, n);
                if n + 1 == crate::snap::delays().len() {
                    return self.quit();
                }
                Task::none()
            }
            M::Quit => {
                let t = self.player.stop();
                self.after(t)
            }
            M::Close => self.quit(),
            M::Probe => {
                self.probe();
                Task::none()
            }
        }
    }

    /// QA: what mpv reports once a second (`NOCTURNE_PREVIEW_PROBE=1`). Video pixels are not in
    /// the window screenshot, so playback is judged from these numbers.
    fn probe(&self) {
        let Some(p) = &self.player.playback else {
            return;
        };
        let g = |n: &str| p.prop(n).map_or("-".into(), |v| v.to_string());
        let st = p.state();
        eprintln!(
            "PROBE t={:.1} paused={} vol={:.0} sid={} sid2={} aid={} hwdec={} fps={} vo-drops={} dec-drops={} params={} cache={:.1} speed={} fs={} popout={} hdrprompt={} gamma={} target={}",
            st.position.unwrap_or(-1.0),
            st.paused,
            st.volume,
            g("sid"),
            g("secondary-sid"),
            g("aid"),
            g("hwdec-current"),
            g("estimated-vf-fps"),
            g("frame-drop-count"),
            g("decoder-frame-drop-count"),
            g("video-params/pixelformat"),
            st.buffered,
            g("speed"),
            self.player.fullscreen,
            self.player.popout,
            self.player.hdr_prompt.is_some(),
            g("video-params/gamma"),
            g("video-target-params/gamma"),
        );
    }

    /// The player's task, or the exit when it reports the page is over.
    fn after(&mut self, t: Task<Msg>) -> Task<M> {
        if let Some(m) = self.player.take_hdr_choice() {
            eprintln!("PREVIEW remember hdr={m:?}");
        }
        match self.player.take_outcome() {
            Some(Outcome::Closed { failed, .. }) => {
                eprintln!("PREVIEW closed failed={failed} rss_kb={}", rss_kb());
                if self.cycles > 0 {
                    // `NOCTURNE_PREVIEW_CYCLES=n`: open the page again n more times (leak check).
                    self.cycles -= 1;
                    let (p, open) = Player::open(ctx().expect("clip"));
                    self.player = p;
                    let ms = std::env::var("NOCTURNE_PREVIEW_QUIT_AFTER_MS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(3000);
                    return Task::batch([
                        t.map(M::P),
                        open.map(M::P),
                        crate::task::after(Duration::from_millis(ms), M::Quit),
                    ]);
                }
                Task::batch([t.map(M::P), self.quit()])
            }
            None => t.map(M::P),
        }
    }

    fn quit(&mut self) -> Task<M> {
        // Drop the player (stops mpv / destroys the surface) before the window goes.
        self.player.shutdown();
        eprintln!("PLAYER_DROPPED");
        match self.win {
            Some(id) => window::close(id).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    fn subscription(&self) -> Subscription<M> {
        Subscription::batch([
            self.player.subscription().map(M::P),
            iced::keyboard::listen()
                .filter_map(|e| keys::from_event(&e).map(|k| M::P(Msg::Key(k)))),
            window::close_requests().map(|_| M::Close),
            // SIGTERM, like the app's own tick (QA of the exit path).
            Subscription::run(crate::redraw::wakes).map(|_| M::Close),
            if std::env::var_os("NOCTURNE_PREVIEW_PROBE").is_some() {
                iced::time::every(Duration::from_secs(1)).map(|_| M::Probe)
            } else {
                Subscription::none()
            },
        ])
    }

    fn view(&self) -> Element<'_, M> {
        self.player.view().map(M::P)
    }
}

pub fn run() -> iced::Result {
    let _ = SERVER.set(server_ctx());
    iced::application(boot, Preview::update, Preview::view)
        .title("NOCTURNE player preview")
        .subscription(Preview::subscription)
        .style(|_, _| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme::text(),
        })
        .theme(|_: &Preview| iced::Theme::Dark)
        .default_font(theme::base())
        .exit_on_close_request(false)
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(800.0, 560.0)),
            transparent: true,
            platform_specific: crate::platform::window_settings(),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
