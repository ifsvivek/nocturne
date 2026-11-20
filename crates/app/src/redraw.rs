//! Why the window wakes up. Every Iced message redraws the window, so an idle app must receive none:
//! the root tick runs only while something needs it, and signals and a second launch reach it through
//! [`wakes`], which sends only when one is pending.
//!
//! Debug builds with `NOCTURNE_REDRAW_LOG=1` print one line per message (`msg Discriminant(n)`, n = the variant's index in `app/message.rs`) and per drawn frame of an engine screen (`draw`), so a 10 s idle run can be counted and attributed.
use crate::app::message::Message;
use iced::futures::{SinkExt, Stream};
use std::time::Duration;

/// A message-free poll of the termination flag and the "another launch wants focus" flag.
pub fn wakes() -> impl Stream<Item = ()> {
    iced::stream::channel(1, async |mut out| {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if crate::platform::term_requested() || crate::single::activate_pending() {
                let _ = out.send(()).await;
            }
        }
    })
}

#[cfg(debug_assertions)]
fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NOCTURNE_REDRAW_LOG").is_some_and(|v| v == "1"))
}

#[cfg(debug_assertions)]
pub fn log(m: &Message) {
    if on() {
        // `Message` is not `Debug`: the discriminant is the variant's index in declaration order among the compiled variants.
        #[cfg(feature = "theme-engine")]
        if let Message::Eng(_, e) = m {
            let d = format!("{e:?}");
            let name: String = d.chars().take_while(|c| c.is_alphanumeric()).collect();
            return eprintln!("redraw-log: msg Eng::{name}");
        }
        eprintln!("redraw-log: msg {:?}", std::mem::discriminant(m));
    }
}

#[cfg(debug_assertions)]
pub fn draw(why: impl FnOnce() -> String) {
    if on() {
        eprintln!("redraw-log: draw {}", why());
    }
}

#[cfg(not(debug_assertions))]
pub fn log(_: &Message) {}

#[cfg(not(debug_assertions))]
pub fn draw(_: impl FnOnce() -> String) {}
