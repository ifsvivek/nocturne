//! Debug-only preview of the player's loading overlay (`NOCTURNE_PREVIEW=buffering`), so it can
//! be looked at and screenshotted before the player page exists. `from_env` returns `None`
//! unless `debug_assertions` is on.
//! `NOCTURNE_PREVIEW_AT=0.42` pins the fill (else it runs 0 to 100 % over 10 s);
//! `NOCTURNE_PREVIEW_BACKDROP` / `NOCTURNE_PREVIEW_LOGO` take image files;
//! `NOCTURNE_PREVIEW_LIGHT=1` shows the mid-playback variant.
use super::Message;
use crate::widget::buffering::{self, Progress, Splash};
use iced::widget::image::Handle;
use iced::{Element, Size};
use std::time::{Duration, Instant};

const RUN: Duration = Duration::from_secs(10);

pub struct Preview {
    start: Instant,
    pinned: Option<f32>,
    prog: Progress,
    backdrop: Option<Handle>,
    logo: Option<Handle>,
    light: bool,
}

impl Preview {
    pub fn from_env() -> Option<Self> {
        if !cfg!(debug_assertions) || std::env::var("NOCTURNE_PREVIEW").ok()? != "buffering" {
            return None;
        }
        let now = Instant::now();
        let env = |k: &str| std::env::var(k).ok();
        let pinned: Option<f32> = env("NOCTURNE_PREVIEW_AT").and_then(|v| v.parse().ok());
        let mut prog = Progress::new(now);
        if let Some(p) = pinned {
            prog.set(p, now);
        }
        Some(Self {
            start: now,
            pinned,
            prog,
            backdrop: env("NOCTURNE_PREVIEW_BACKDROP").map(Handle::from_path),
            logo: env("NOCTURNE_PREVIEW_LOGO").map(Handle::from_path),
            light: env("NOCTURNE_PREVIEW_LIGHT").is_some(),
        })
    }

    fn fill(&self, now: Instant) -> f32 {
        self.pinned
            .unwrap_or_else(|| now.duration_since(self.start).as_secs_f32() / RUN.as_secs_f32())
    }

    pub fn tick(&mut self, now: Instant) {
        self.prog.set(self.fill(now), now);
    }

    /// Breathing and zoom keep the overlay moving, so it asks for frames unless motion is reduced.
    pub fn animating(&self, reduced: bool) -> bool {
        !reduced
    }

    pub fn view(&self, now: Instant, reduced: bool, win: Size) -> Element<'_, Message> {
        let fill = self.fill(now);
        let status = if fill < 0.02 {
            "Opening".to_string()
        } else {
            buffering::percent_label(fill)
        };
        let since = now.duration_since(self.start);
        buffering::view(Splash {
            title: "The Long Way Home",
            backdrop: self.backdrop.as_ref(),
            logo: self.logo.as_ref(),
            status,
            reveal: self.prog.value(now, reduced),
            breath: buffering::breath(since.as_secs_f32(), self.prog.stalled(now), reduced),
            zoom: buffering::zoom(since, reduced),
            fade: 1.0,
            reduced,
            light: self.light,
            win,
        })
    }
}
