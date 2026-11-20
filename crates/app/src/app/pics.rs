//! Fetching the pictures a theme's `url()` backgrounds name (`widget/pics.rs` holds them, `nocturne_core::customize::import::fetch_picture` has the rules). Run at the end of every update: scenes only record the addresses they saw.
use super::message::Message;
use super::{App, run};
use crate::widget::pics;
use iced::Task;
use nocturne_core::customize::import;
use nocturne_core::profile::ProfileKind;
use std::sync::Arc;

impl App {
    pub(super) fn pics_sync(&mut self) -> Task<Message> {
        let origin = self
            .theme_ui()
            .and_then(|u| self.sources.get(u))
            .and_then(|p| match &p.kind {
                ProfileKind::Jellyfin { url, .. } => Some(url.clone()),
                ProfileKind::Local { .. } => None,
            });
        pics::set_context(origin.as_deref());
        let remote = self.themes.remote();
        let tasks: Vec<_> = pics::take_wanted()
            .into_iter()
            .map(|(raw, abs, origin)| {
                run(
                    async move {
                        let p = import::fetch_picture(&abs, origin.as_deref(), remote).await?;
                        pics::decode(&p.bytes, p.svg)
                    },
                    move |r| {
                        if let Err(e) = &r
                            && std::env::var_os("NOCTURNE_ENGINE_STATS").is_some()
                        {
                            eprintln!("pics: {raw}: {e}");
                        }
                        Message::Pic(raw, r.ok().map(Arc::new))
                    },
                )
            })
            .collect();
        Task::batch(tasks)
    }
}
