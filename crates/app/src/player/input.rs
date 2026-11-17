//! Player-page keyboard handling: which layer owns a key (prompt, context menu, queue, open menu,
//! then the page's own map from core's `video::key`).
use super::popovers::ctx_rows;
use super::{CtxAct, Msg, Player, logic};
use crate::keys::Key as Pressed;
use iced::Task;
use nocturne_core::video::{self, Key, MenuAct, Pop, Repeat};

impl Player {
    /// Player-page keys. `None` when the key is not the page's (the caller may use it).
    pub fn key(&mut self, k: &Pressed) -> Option<Task<Msg>> {
        self.poke();
        let name = k.name.as_str();
        if self.problem.is_some() {
            return Some(match name {
                "escape" | "enter" => self.close(false),
                _ => Task::none(),
            });
        }
        if let Some(p) = self.hdr_prompt.as_mut() {
            // Esc answers "Play here in SDR" without remembering anything.
            let t = match name {
                "enter" => {
                    let popout = p.sel == 1;
                    self.hdr_choose(popout)
                }
                "escape" | "s" => {
                    p.remember = false;
                    self.hdr_choose(false)
                }
                _ => {
                    p.key(name);
                    Task::none()
                }
            };
            return Some(t);
        }
        if self.resume_prompt.is_some() {
            match name {
                "enter" => self.resume_choice(true),
                "escape" | "o" => self.resume_choice(false),
                _ => {}
            }
            return Some(Task::none());
        }
        if self.ctx_menu.is_some() {
            let rows = ctx_rows(self);
            return Some(match name {
                "up" | "down" => {
                    self.pop_sel = logic::list_nav(name, self.pop_sel, rows.len());
                    Task::none()
                }
                "enter" => {
                    self.ctx_menu = None;
                    match rows.get(self.pop_sel) {
                        Some((_, CtxAct::Key(k))) => self.do_key(*k),
                        Some((_, CtxAct::Pop(p))) => {
                            self.toggle_pop(*p);
                            Task::none()
                        }
                        None => Task::none(),
                    }
                }
                _ => {
                    self.ctx_menu = None;
                    Task::none()
                }
            });
        }
        if self.queue_open {
            let mut t = Task::none();
            match name {
                "up" | "down" => {
                    self.queue_sel = logic::list_nav(name, self.queue_sel, self.queue.len())
                }
                "enter" => t = self.jump(self.queue_sel),
                "[" => self.queue_shift(self.queue_sel, true),
                "]" => self.queue_shift(self.queue_sel, false),
                "x" | "delete" => self.queue_drop(self.queue_sel),
                "z" => self.toggle_shuffle(),
                "r" => self.cycle_repeat(),
                "escape" | "u" => self.queue_open = false,
                _ => {}
            }
            return Some(t);
        }
        if let Some(t) = self.pop_key(name) {
            return Some(t);
        }
        if name == "enter" {
            if let Some(s) = self.active_seg() {
                self.skip(s);
                return Some(Task::none());
            }
            if let Some(i) = self.next_card().and(self.next_index()) {
                return Some(self.jump(i));
            }
        }
        let key = video::key(name, k.shift, k.ctrl)?;
        Some(self.do_key(key))
    }

    fn pop_key(&mut self, name: &str) -> Option<Task<Msg>> {
        let pop = self.pop?;
        let items = video::items(pop, &self.tracks, &self.set);
        let mut t = Task::none();
        match name {
            "up" | "down" => {
                self.pop_sel = logic::list_nav(name, self.pop_sel, items.len());
                // Headings are not choices: step over them.
                if items.get(self.pop_sel).is_some_and(|i| i.head) {
                    self.pop_sel = logic::list_nav(name, self.pop_sel, items.len());
                }
            }
            "left" => {
                if let Some(a) = items.get(self.pop_sel).and_then(|i| i.dec.clone()) {
                    t = self.pop_act(a);
                }
            }
            "right" | "enter" => {
                if let Some(i) = items.get(self.pop_sel) {
                    t = self.pop_act(i.act.clone());
                }
            }
            "escape" => {
                self.pop = pop.is_sub().then_some(Pop::Settings);
                self.pop_sel = if pop.is_sub() {
                    video::settings_index(&self.tracks, &self.set, pop)
                } else {
                    0
                };
            }
            _ => {}
        }
        Some(t)
    }

    pub fn do_key(&mut self, key: Key) -> Task<Msg> {
        let pos = self.pos();
        match key {
            Key::Toggle => self.toggle(),
            Key::Seek(s) => self.seek(s),
            Key::Volume(d) => {
                self.muted_from = None;
                let v = video::bump_volume(self.volume, d);
                if let Some(p) = &self.playback {
                    let _ = p.set_volume(v);
                }
                self.osd(format!("Volume {}%", v.round()));
            }
            Key::Mute => self.mute(),
            Key::Stop => return self.stop(),
            Key::Fullscreen => return self.toggle_fullscreen(),
            Key::Back if self.help || self.stats => (self.help, self.stats) = (false, false),
            Key::Back if self.next_since.is_some() && !self.next_dismissed => {
                self.next_dismissed = true;
                self.next_since = None;
                self.osd("Autoplay cancelled");
            }
            Key::Back if self.mini => return self.toggle_mini(),
            Key::Back if self.fullscreen => return self.toggle_fullscreen(),
            Key::Back => return self.close(false),
            Key::Next => return self.step(1),
            Key::Prev => return self.step(-1),
            Key::Queue => {
                self.queue_open = !self.queue_open;
                self.queue_sel = self.qi;
                self.pop = None;
            }
            Key::Speed(d) => self.set_speed(video::step_speed(self.set.speed, d)),
            Key::Frame(d) => {
                if let Some(p) = &self.playback {
                    let _ = p.command(&[if d > 0 {
                        "frame-step"
                    } else {
                        "frame-back-step"
                    }]);
                }
            }
            Key::Chapter(d) => {
                if let Some(t) = video::chapter_target(&self.chapters, pos, d) {
                    self.seek_to(t);
                    let title = video::chapter_at(&self.chapters, t)
                        .map(|i| self.chapters[i].1.clone())
                        .filter(|s| !s.is_empty());
                    self.osd(title.unwrap_or_else(|| "Chapter".into()));
                }
            }
            Key::SubDelay(d) => self.add_delay("sub-delay", "Subtitle", d),
            Key::AudioDelay(d) => self.add_delay("audio-delay", "Audio", d),
            Key::CycleSubs => {
                self.refresh_meta();
                let cur = self
                    .tracks
                    .iter()
                    .find(|t| t.kind == video::TrackKind::Sub && t.selected)
                    .map_or(0, |t| t.id);
                let next = video::next_sub(&self.tracks, cur);
                let l = if next == 0 {
                    "Subtitles off"
                } else {
                    "Subtitles on"
                };
                self.set_track("sid", next, l);
            }
            Key::CycleSecondary => {
                self.refresh_meta();
                let cur = self.shown_sub(true);
                let next = video::next_sub_except(&self.tracks, cur, self.shown_sub(false));
                let l = if next == 0 {
                    "Secondary subtitles off"
                } else {
                    "Secondary subtitles on"
                };
                self.set_track("secondary-sid", next, l);
            }
            Key::Stats => self.stats = !self.stats,
            Key::Help => self.help = !self.help,
            Key::Screenshot => self.screenshot(),
            Key::Loop => {
                let r = if self.set.repeat == Repeat::One {
                    Repeat::Off
                } else {
                    Repeat::One
                };
                self.set_repeat(r);
            }
            Key::AbLoop => return self.pop_act(MenuAct::AbLoop),
            Key::Mini => return self.toggle_mini(),
            Key::Favorite => self.toggle_favorite(),
        }
        Task::none()
    }
}
