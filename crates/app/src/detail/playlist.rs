//! The user's own playlists: what a detail page can ask for and how the root carries it out.
//! Every op is a server write; the page only emits one after the user confirmed it (a dialog, or an
//! explicit key on a playlist they own) and only while server changes are on (`allow_server_writes`,
//! checked by the root again). The calm sentences are the ones the GPUI account dialog used.
use nocturne_core::account::ops::playlist_fail;
use nocturne_jellyfin_user::UserClient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaylistOp {
    /// `ids` into the playlist called `name`: one of `own` (case-insensitive) or a new one.
    Add {
        ids: Vec<String>,
        name: String,
        own: Vec<(String, String)>,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    /// Entry `idx` of the playlist (which must still be `item`), dropped.
    Remove {
        id: String,
        idx: usize,
        item: String,
    },
    /// Entry `idx` of the playlist (which must still be `item`), moved to position `to`.
    Move {
        id: String,
        idx: usize,
        item: String,
        to: usize,
    },
}

impl PlaylistOp {
    /// Toast shown when it went through.
    pub fn done_text(&self) -> &'static str {
        match self {
            Self::Add { .. } => "Added to the playlist.",
            Self::Rename { .. } => "Playlist renamed.",
            Self::Delete { .. } => "Playlist deleted.",
            Self::Remove { .. } => "Removed from the playlist.",
            Self::Move { .. } => "Moved.",
        }
    }
}

/// Run `op` with the signed-in user's client. `Err` is the sentence to show.
pub async fn perform(c: UserClient, op: PlaylistOp) -> Result<(), &'static str> {
    let fail = |e| playlist_fail(e).text();
    match op {
        PlaylistOp::Add { ids, name, own } => {
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            match own.iter().find(|o| o.1.eq_ignore_ascii_case(&name)) {
                Some((pid, _)) => c.add_to_playlist(pid, &ids).await,
                None => c.create_playlist(&name, &ids).await.map(drop),
            }
            .map_err(fail)
        }
        PlaylistOp::Rename { id, name } => c.rename_playlist(&id, &name).await.map_err(fail),
        PlaylistOp::Delete { id } => c.delete_playlist(&id).await.map_err(fail),
        PlaylistOp::Remove { id, idx, item } => {
            let e = entry(&c, &id, idx, &item).await?;
            c.remove_from_playlist(&id, &[e.as_str()])
                .await
                .map_err(fail)
        }
        PlaylistOp::Move { id, idx, item, to } => {
            let e = entry(&c, &id, idx, &item).await?;
            c.move_playlist_item(&id, &e, to as u32).await.map_err(fail)
        }
    }
}

/// Entry ids come fresh from the server; refuse if the list moved under us.
async fn entry(c: &UserClient, id: &str, idx: usize, item: &str) -> Result<String, &'static str> {
    const MOVED: &str = "That didn't work. The playlist may have changed.";
    let entries = c.playlist_items(id).await.map_err(|_| MOVED)?;
    entries
        .get(idx)
        .filter(|x| x.item_id == item)
        .map(|x| x.entry_id.clone())
        .ok_or(MOVED)
}
