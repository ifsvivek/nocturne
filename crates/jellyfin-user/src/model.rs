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

/// What `/System/Info/Public` tells an anonymous caller.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PublicInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
}

/// One of the account's own sessions (`GET /Sessions`, narrowed to this user).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct OwnSession {
    pub user_id: String,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub application_version: String,
    pub last_activity_date: String,
    /// True for the session this app is using (set by the client, not the server).
    #[serde(skip)]
    pub current: bool,
}

/// Keep only `user_id`'s sessions and flag the one belonging to `device_id`.
pub(crate) fn own_sessions(
    all: Vec<OwnSession>,
    user_id: &str,
    device_id: &str,
) -> Vec<OwnSession> {
    let mut v: Vec<_> = all
        .into_iter()
        .filter(|s| s.user_id == user_id)
        .map(|mut s| {
            s.current = s.device_id == device_id;
            s
        })
        .collect();
    v.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then(b.last_activity_date.cmp(&a.last_activity_date))
    });
    // The server lists only sessions it has seen since it started; after a restart our own
    // restored session can be missing, but we know we are signed in here.
    if !v.first().is_some_and(|s| s.current) {
        v.insert(
            0,
            OwnSession {
                user_id: user_id.into(),
                client: "NOCTURNE".into(),
                device_name: "Linux".into(),
                device_id: device_id.into(),
                application_version: env!("CARGO_PKG_VERSION").into(),
                current: true,
                ..Default::default()
            },
        );
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_sessions_are_filtered_flagged_and_sorted() {
        let all: Vec<OwnSession> = serde_json::from_str(
            r#"[{"UserId":"u","DeviceId":"a","LastActivityDate":"2026-01-01"},
                {"UserId":"other","DeviceId":"x"},
                {"UserId":"u","DeviceId":"me","LastActivityDate":"2025-01-01","Future":1},
                {"UserId":"u","DeviceId":"b","LastActivityDate":"2026-06-01"}]"#,
        )
        .unwrap();
        let v = own_sessions(all, "u", "me");
        let ids: Vec<_> = v.iter().map(|s| s.device_id.as_str()).collect();
        assert_eq!(ids, ["me", "b", "a"], "this device first, then newest");
        assert!(v[0].current && !v[1].current);
        // our own session missing from the server's list: still shown, first
        let v = own_sessions(vec![], "u", "me");
        assert!(v.len() == 1 && v[0].current && v[0].client == "NOCTURNE");
    }
}

/// One display language the server offers (`GET /Localization/Options`), such as
/// `{"Name": "English (United States)", "Value": "en-US"}`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct LanguageOption {
    pub name: String,
    pub value: String,
}

/// The few own-profile fields the app shows (`GET /Users/Me`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Profile {
    pub name: String,
    pub primary_image_tag: Option<String>,
}

/// Largest profile image accepted (the server stores it as is).
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// MIME type of a PNG, JPEG, WebP or GIF by its first bytes; anything else is refused before it
/// is sent.
pub fn image_mime(bytes: &[u8]) -> crate::Result<&'static str> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(crate::Error::Config(
            "the image is larger than 10 MB".into(),
        ));
    }
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Ok("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Ok("image/jpeg"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Ok("image/webp"),
        [b'G', b'I', b'F', b'8', ..] => Ok("image/gif"),
        _ => Err(crate::Error::Config(
            "that file is not a PNG, JPEG, WebP or GIF image".into(),
        )),
    }
}
