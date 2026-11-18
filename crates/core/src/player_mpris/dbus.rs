//! The D-Bus half of [`super`]: MPRIS service and ScreenSaver inhibit on one worker thread.
use super::{Cmd, Out, Snap};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

impl Snap {
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m: HashMap<String, OwnedValue> = HashMap::new();
        let mut put = |k: &str, v: Value<'_>| {
            if let Ok(v) = OwnedValue::try_from(v) {
                m.insert(k.into(), v);
            }
        };
        put(
            "mpris:trackid",
            Value::from(ObjectPath::from_static_str_unchecked(
                "/org/nocturne/track/1",
            )),
        );
        put("mpris:length", Value::from(self.length_us));
        put("xesam:title", Value::from(self.title.as_str()));
        if !self.artist.is_empty() {
            put("xesam:artist", Value::from(vec![self.artist.as_str()]));
        }
        if !self.art_url.is_empty() {
            put("mpris:artUrl", Value::from(self.art_url.as_str()));
        }
        m
    }
}

struct Shared {
    snap: Mutex<Snap>,
    tx: Sender<Cmd>,
}

struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "NOCTURNE"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec![]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

struct Player(Arc<Shared>);

impl Player {
    fn send(&self, c: Cmd) {
        let _ = self.0.tx.send(c);
    }
    fn snap(&self) -> Snap {
        self.0.snap.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(Cmd::Next)
    }
    fn previous(&self) {
        self.send(Cmd::Prev)
    }
    fn pause(&self) {
        self.send(Cmd::Pause)
    }
    fn play_pause(&self) {
        self.send(Cmd::PlayPause)
    }
    fn stop(&self) {
        self.send(Cmd::Stop)
    }
    fn play(&self) {
        self.send(Cmd::Play)
    }
    fn seek(&self, offset_us: i64) {
        self.send(Cmd::SeekBy(crate::player_logic::from_micros(offset_us)))
    }
    fn set_position(&self, _track: ObjectPath<'_>, pos_us: i64) {
        self.send(Cmd::SeekTo(crate::player_logic::from_micros(pos_us)))
    }
    fn open_uri(&self, _uri: &str) {}
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.snap().status.to_string()
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        0.25
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        4.0
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.snap().metadata()
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.snap().volume
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        self.snap().pos_us
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.snap().can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.snap().can_prev
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

pub(super) fn worker(tx: Sender<Cmd>, rx: Receiver<Out>) {
    let shared = Arc::new(Shared {
        snap: Mutex::new(Snap::default()),
        tx,
    });
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| {
            b.name(format!(
                "org.mpris.MediaPlayer2.nocturne.instance{}",
                std::process::id()
            ))
        })
        .and_then(|b| b.serve_at(PATH, Root))
        .and_then(|b| b.serve_at(PATH, Player(shared.clone())))
        .and_then(|b| b.build())
        .ok();
    let mut cookie: Option<u32> = None;
    for msg in rx {
        match msg {
            Out::Snap(s) => {
                let Some(c) = &conn else { continue };
                let old = std::mem::replace(&mut *shared.snap.lock().unwrap(), s.clone());
                let mut ch: HashMap<&str, Value<'_>> = HashMap::new();
                if old.status != s.status {
                    ch.insert("PlaybackStatus", Value::from(s.status));
                }
                if (&old.title, &old.artist, old.length_us, &old.art_url)
                    != (&s.title, &s.artist, s.length_us, &s.art_url)
                {
                    let m: HashMap<String, Value<'_>> = s
                        .metadata()
                        .into_iter()
                        .map(|(k, v)| (k, Value::from(v)))
                        .collect();
                    ch.insert("Metadata", Value::from(zbus::zvariant::Dict::from(m)));
                }
                if old.can_next != s.can_next {
                    ch.insert("CanGoNext", Value::from(s.can_next));
                }
                if old.can_prev != s.can_prev {
                    ch.insert("CanGoPrevious", Value::from(s.can_prev));
                }
                if !ch.is_empty() {
                    let _ = c.emit_signal(
                        None::<&str>,
                        PATH,
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                        &(PLAYER, ch, Vec::<&str>::new()),
                    );
                }
            }
            Out::Inhibit(on) => {
                let Some(c) = &conn else { continue };
                if on && cookie.is_none() {
                    cookie = c
                        .call_method(
                            Some("org.freedesktop.ScreenSaver"),
                            "/org/freedesktop/ScreenSaver",
                            Some("org.freedesktop.ScreenSaver"),
                            "Inhibit",
                            &("NOCTURNE", "Playing video"),
                        )
                        .ok()
                        .and_then(|m| m.body().deserialize::<u32>().ok());
                } else if !on && let Some(k) = cookie.take() {
                    let _ = c.call_method(
                        Some("org.freedesktop.ScreenSaver"),
                        "/org/freedesktop/ScreenSaver",
                        Some("org.freedesktop.ScreenSaver"),
                        "UnInhibit",
                        &(k,),
                    );
                }
            }
            Out::Quit => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_carries_title_and_optional_art() {
        let s = Snap {
            title: "T".into(),
            length_us: 5,
            ..Default::default()
        };
        let m = s.metadata();
        assert!(m.contains_key("xesam:title") && m.contains_key("mpris:trackid"));
        assert!(!m.contains_key("mpris:artUrl") && !m.contains_key("xesam:artist"));
        let m = Snap {
            art_url: "file:///x".into(),
            artist: "A".into(),
            ..s
        }
        .metadata();
        assert!(m.contains_key("mpris:artUrl") && m.contains_key("xesam:artist"));
    }
}
