use serde::{Deserialize, Serialize};

/// A login. `Debug` redacts the token. Store it in the keyring (`nocturne-session`), never a file.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub user_id: String,
    pub server_id: String,
    pub user_name: String,
    pub is_admin: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("user_name", &self.user_name)
            .field("access_token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct AuthResult {
    pub access_token: String,
    pub server_id: String,
    pub user: AuthUser,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct AuthUser {
    pub id: String,
    pub name: String,
    pub policy: AuthPolicy,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct AuthPolicy {
    pub is_administrator: bool,
}

impl From<AuthResult> for Session {
    fn from(a: AuthResult) -> Self {
        Session {
            access_token: a.access_token,
            user_id: a.user.id,
            server_id: a.server_id,
            user_name: a.user.name,
            is_admin: a.user.policy.is_administrator,
        }
    }
}

/// Quick Connect handshake: show `code` to the user, keep `secret` private. `Debug` hides the secret.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuickConnect {
    pub code: String,
    pub secret: String,
}

impl std::fmt::Debug for QuickConnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickConnect")
            .field("code", &self.code)
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct QuickConnectState {
    pub authenticated: bool,
}

/// What is being reported to `/Sessions/Playing*`. Position is in 100 ns ticks.
#[derive(Debug, Clone, Default)]
pub struct PlaybackReport {
    pub item_id: String,
    pub media_source_id: Option<String>,
    pub position_ticks: u64,
    pub is_paused: bool,
    pub audio_stream_index: Option<u32>,
    pub subtitle_stream_index: Option<u32>,
}

/// An entry of a playlist; `entry_id` (not `item_id`) is what remove/move take.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PlaylistEntry {
    #[serde(rename = "PlaylistItemId")]
    pub entry_id: String,
    #[serde(rename = "Id")]
    pub item_id: String,
    pub name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct EntryPage {
    pub items: Vec<PlaylistEntry>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct Created {
    pub id: String,
}
