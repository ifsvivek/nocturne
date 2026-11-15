//! The user's own playlists (create, rename, add, remove, reorder, delete) and the music queue
//! that lives for the session. Every playlist change goes through the account dialog, which is
//! the only place the user's confirmation turns into a write.
use super::*;
use crate::account::{DlgKind, Target};
use crate::video::Repeat;

/// The music queue kept while the app runs: what to play next, where it stands, how it plays.
#[derive(Clone, Default)]
pub(super) struct AudioQueue {
    pub items: Vec<Media>,
    pub index: usize,
    pub repeat: Option<Repeat>,
    pub shuffle: bool,
}

/// What a keypress does to the selected entry of an owned playlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edit {
    Remove,
    Up,
    Down,
}

impl Edit {
    pub(super) fn of(key: &str) -> Option<Self> {
        match key {
            "x" | "delete" => Some(Self::Remove),
            "[" => Some(Self::Up),
            "]" => Some(Self::Down),
            _ => None,
        }
    }

    /// The server call for entry `idx` of `len`: Some(Remove) or Some(Move to n), None at an edge.
    pub(super) fn plan(self, idx: usize, len: usize) -> Option<Option<usize>> {
        if idx >= len {
            return None;
        }
        match self {
            Self::Remove => Some(None),
            Self::Up => idx.checked_sub(1).map(Some),
            Self::Down => (idx + 1 < len).then_some(Some(idx + 1)),
        }
    }
}

impl Root {
    /// A user client for the signed-in account, only while server changes are allowed.
    fn writer(&mut self, cx: &mut Context<Self>) -> Option<UserClient> {
        if !self.hooks.can_write() {
            self.toast(self.writes_off(), cx);
            return None;
        }
        self.session
            .as_ref()
            .and_then(|(u, s)| UserClient::from_session(u, s).ok())
    }

    pub(super) fn playlist_dialog(
        &mut self,
        kind: DlgKind,
        target: Target,
        cx: &mut Context<Self>,
    ) {
        let (Some(c), Some(a)) = (self.writer(cx), self.accounts.clone()) else {
            return;
        };
        a.update(cx, |a, cx| a.open_playlist(kind, c, target, cx));
    }

    /// Ask which playlist `ids` go to: the dialog lists the playlists the user owns.
    pub(super) fn add_to_playlist(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        if ids.is_empty() {
            return self.toast("Nothing to add here.", cx);
        }
        if !self.hooks.can_write() {
            return self.toast(self.writes_off(), cx);
        }
        self.run(
            cx,
            async move { src.own_playlists().await },
            move |t, r, cx| {
                let own = r.unwrap_or_default();
                t.playlist_dialog(DlgKind::AddToPlaylist, Target::Items(ids, own), cx);
            },
        );
    }

    /// The open page is a playlist the user may edit.
    pub(super) fn owned_playlist(&self) -> Option<Media> {
        self.stack
            .last()
            .and_then(|p| p.media.clone())
            .filter(|m| m.kind == Kind::Playlist && m.can_delete)
    }

    /// Remove or move the selected entry of the open (owned) playlist.
    pub(super) fn playlist_item_edit(&mut self, e: Edit, cx: &mut Context<Self>) {
        let Some(pl) = self.owned_playlist() else {
            return;
        };
        let Some(p) = self.stack.last() else { return };
        let (pid, idx) = (p.pid, p.ep_sel);
        let len = p.episodes.items().len();
        let Some(item) = p.episodes.items().get(idx).cloned() else {
            return;
        };
        let Some(c) = self.writer(cx) else { return };
        let Some(plan) = e.plan(idx, len) else {
            return self.toast("Already at the end.", cx);
        };
        let id = pl.id.clone();
        self.run(
            cx,
            async move {
                // Entry ids come fresh from the server; refuse if the list moved under us.
                let entries = c.playlist_items(&id).await.map_err(|_| ())?;
                let entry = entries
                    .get(idx)
                    .filter(|x| x.item_id == item.id)
                    .ok_or(())?;
                match plan {
                    None => {
                        c.remove_from_playlist(&id, &[entry.entry_id.as_str()])
                            .await
                    }
                    Some(to) => c.move_playlist_item(&id, &entry.entry_id, to as u32).await,
                }
                .map_err(|_| ())
            },
            move |t, r, cx| match r {
                Ok(()) => {
                    if let Some(p) = t.stack.iter_mut().find(|p| p.pid == pid) {
                        p.ep_sel = match plan {
                            Some(to) => to,
                            None => idx.min(len.saturating_sub(2)),
                        };
                    }
                    t.retry_page(cx);
                }
                Err(()) => t.toast("That didn't work. The playlist may have changed.", cx),
            },
        );
    }

    // ---- music queue ----

    /// Put tracks after the playing one or at the end of the music queue. Nothing is sent to the
    /// server; the queue lives for this session.
    pub(super) fn queue_tracks(&mut self, tracks: Vec<Media>, next: bool, cx: &mut Context<Self>) {
        let tracks: Vec<Media> = tracks
            .into_iter()
            .filter(|m| m.kind == Kind::Audio)
            .collect();
        if tracks.is_empty() {
            return self.toast("No music to queue here.", cx);
        }
        let q = &mut self.audio_queue;
        crate::player_logic::queue_insert(&mut q.items, q.index, tracks, next);
        self.toast(
            if next {
                "Playing next."
            } else {
                "Added to the queue."
            },
            cx,
        );
    }

    /// Start (or resume) the session's music queue where it stood.
    pub(super) fn play_queue(&mut self, cx: &mut Context<Self>) {
        let q = self.audio_queue.clone();
        let Some(first) = q.items.get(q.index).cloned() else {
            return self.toast("The queue is empty.", cx);
        };
        let rest = q.items[q.index + 1..].to_vec();
        self.play_media(first, rest, cx);
        if let Some(p) = self.player.clone() {
            p.update(cx, |p, _| {
                p.queue = q.items.clone();
                p.qi = q.index;
                if let Some(r) = q.repeat {
                    p.set.repeat = r;
                }
                if q.shuffle {
                    // Keep the saved order as it is: it already is the shuffled one.
                    p.shuffle = true;
                    p.unshuffled = q.items.clone();
                }
            });
        }
    }

    pub(super) fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.audio_queue = AudioQueue::default();
        self.toast("Queue cleared.", cx);
    }

    /// Called when the player goes away: keep its music queue for the session.
    pub(super) fn keep_music(&mut self, q: Option<(Vec<Media>, usize, Repeat, bool)>) {
        if let Some((items, index, repeat, shuffle)) = q {
            self.audio_queue = AudioQueue {
                items,
                index,
                repeat: Some(repeat),
                shuffle,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test; // gpui::* also exports a `test` macro

    #[test]
    fn playlist_key_edits_follow_the_list_edges() {
        assert_eq!(Edit::of("x"), Some(Edit::Remove));
        assert_eq!(Edit::of("]"), Some(Edit::Down));
        assert_eq!(Edit::of("q"), None);
        assert_eq!(Edit::Remove.plan(0, 1), Some(None));
        assert_eq!(Edit::Up.plan(0, 3), None, "first entry cannot go up");
        assert_eq!(Edit::Up.plan(2, 3), Some(Some(1)));
        assert_eq!(Edit::Down.plan(2, 3), None, "last entry cannot go down");
        assert_eq!(Edit::Down.plan(0, 3), Some(Some(1)));
        assert_eq!(Edit::Remove.plan(5, 3), None, "stale selection");
    }
}
