use crate::model::*;
use crate::{Error, Result, UserConfirmed};
use reqwest::{Method, Url, header};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

/// User-scope Jellyfin client. Anonymous (login, Quick Connect) or bound to a [`Session`].
#[derive(Clone)]
pub struct UserClient {
    http: reqwest::Client,
    base: Url,
    user_id: String,
}

impl std::fmt::Debug for UserClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserClient")
            .field("base", &self.base.as_str())
            .field("token", &"<redacted>")
            .finish()
    }
}

fn http_err(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

fn device_id() -> String {
    let id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let id: String = id.trim().chars().take(12).collect();
    format!("nocturne-{}", if id.is_empty() { "unknown" } else { &id })
}

impl UserClient {
    /// Not logged in: only the AUTH methods work.
    pub fn anonymous(base_url: &str) -> Result<Self> {
        Self::build(base_url, None, String::new())
    }

    /// Logged-in client; every request carries the session token (header marked sensitive).
    pub fn from_session(base_url: &str, s: &Session) -> Result<Self> {
        Self::build(base_url, Some(&s.access_token), s.user_id.clone())
    }

    fn build(base_url: &str, token: Option<&str>, user_id: String) -> Result<Self> {
        let base = Url::parse(base_url.trim_end_matches('/'))
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
            .ok_or_else(|| Error::Config("base_url is not a valid http(s) URL".into()))?;
        let mut v = format!(
            "MediaBrowser Client=\"NOCTURNE\", Device=\"Linux\", DeviceId=\"{}\", Version=\"{}\"",
            device_id(),
            env!("CARGO_PKG_VERSION")
        );
        if let Some(t) = token {
            v.push_str(&format!(", Token=\"{t}\""));
        }
        let mut auth = header::HeaderValue::from_str(&v)
            .map_err(|_| Error::Config("token contains invalid header characters".into()))?;
        auth.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .default_headers(headers)
            .build()
            .map_err(http_err)?;
        Ok(Self {
            http,
            base,
            user_id,
        })
    }

    async fn send(
        &self,
        method: Method,
        segs: &[&str],
        pairs: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<reqwest::Response> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("http url has a base")
            .pop_if_empty()
            .extend(segs);
        if !pairs.is_empty() {
            url.query_pairs_mut().extend_pairs(pairs);
        }
        let path = url.path().to_string();
        let mut req = self.http.request(method, url);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(http_err)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                path,
            });
        }
        Ok(resp)
    }

    async fn json<T: DeserializeOwned>(
        &self,
        m: Method,
        segs: &[&str],
        pairs: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<T> {
        self.send(m, segs, pairs, body)
            .await?
            .json()
            .await
            .map_err(http_err)
    }

    async fn run(
        &self,
        m: Method,
        segs: &[&str],
        pairs: &[(&str, String)],
        b: Option<Value>,
    ) -> Result<()> {
        self.send(m, segs, pairs, b).await.map(drop)
    }

    fn uid(&self) -> (&'static str, String) {
        ("userId", self.user_id.clone())
    }

    // ---- AUTH ----

    /// WRITE `POST /Users/AuthenticateByName`. Wrong credentials give [`Error::InvalidCredentials`].
    /// The password is only sent in the request body; it is never logged or stored.
    pub async fn authenticate_by_name(
        base_url: &str,
        username: &str,
        password: &str,
    ) -> Result<Session> {
        let body = json!({"Username": username, "Pw": password});
        let r: Result<AuthResult> = Self::anonymous(base_url)?
            .json(
                Method::POST,
                &["Users", "AuthenticateByName"],
                &[],
                Some(body),
            )
            .await;
        match r {
            Err(Error::Status { status: 401, .. }) => Err(Error::InvalidCredentials),
            r => r.map(Into::into),
        }
    }

    /// READ `GET /System/Info/Public` (no login needed): is this address a media server, and what is it called?
    pub async fn public_info(&self) -> Result<PublicInfo> {
        self.json(Method::GET, &["System", "Info", "Public"], &[], None)
            .await
    }

    /// READ `GET /Users/Me`: cheap authenticated read that proves the stored token still works.
    /// Returns `stored` refreshed with the current name and admin flag (401 means the token is dead).
    pub async fn me(&self, mut stored: Session) -> Result<Session> {
        let u: AuthUser = self.json(Method::GET, &["Users", "Me"], &[], None).await?;
        stored.user_name = u.name;
        stored.is_admin = u.policy.is_administrator;
        Ok(stored)
    }

    /// READ `GET /QuickConnect/Enabled`.
    pub async fn quick_connect_enabled(&self) -> Result<bool> {
        self.json(Method::GET, &["QuickConnect", "Enabled"], &[], None)
            .await
    }

    /// WRITE `POST /QuickConnect/Initiate`: show the returned code to the user.
    pub async fn quick_connect_initiate(&self) -> Result<QuickConnect> {
        self.json(Method::POST, &["QuickConnect", "Initiate"], &[], None)
            .await
    }

    /// READ `GET /QuickConnect/Connect?secret=`: has another device authorized the code yet?
    pub async fn quick_connect_poll(&self, secret: &str) -> Result<bool> {
        let s: QuickConnectState = self
            .json(
                Method::GET,
                &["QuickConnect", "Connect"],
                &[("secret", secret.into())],
                None,
            )
            .await?;
        Ok(s.authenticated)
    }

    /// WRITE `POST /Users/AuthenticateWithQuickConnect`: exchange an authorized secret for a session.
    pub async fn authenticate_with_quick_connect(&self, secret: &str) -> Result<Session> {
        let r: AuthResult = self
            .json(
                Method::POST,
                &["Users", "AuthenticateWithQuickConnect"],
                &[],
                Some(json!({"Secret": secret})),
            )
            .await?;
        Ok(r.into())
    }

    /// WRITE `POST /QuickConnect/Authorize?code=`: approve another device's code (logged in).
    pub async fn quick_connect_authorize(&self, code: &str) -> Result<()> {
        self.run(
            Method::POST,
            &["QuickConnect", "Authorize"],
            &[("code", code.into()), self.uid()],
            None,
        )
        .await
    }

    /// READ `GET /Sessions`: this account's own sessions (an administrator also gets other
    /// users' sessions from the server; they are filtered out). Sign-in sessions only; the
    /// server offers no way to end other devices without admin rights.
    pub async fn own_sessions(&self) -> Result<Vec<OwnSession>> {
        let all: Vec<OwnSession> = self.json(Method::GET, &["Sessions"], &[], None).await?;
        Ok(own_sessions(all, &self.user_id, &device_id()))
    }

    /// WRITE `POST /Sessions/Logout`: invalidates this token on the server.
    pub async fn logout(&self) -> Result<()> {
        self.run(Method::POST, &["Sessions", "Logout"], &[], None)
            .await
    }

    // ---- PLAYBACK REPORTING (all WRITE) ----

    /// WRITE `POST /Sessions/Playing`.
    pub async fn report_playback_start(&self, r: &PlaybackReport) -> Result<()> {
        self.run(
            Method::POST,
            &["Sessions", "Playing"],
            &[],
            Some(report_body(r)),
        )
        .await
    }

    /// WRITE `POST /Sessions/Playing/Progress`.
    pub async fn report_playback_progress(&self, r: &PlaybackReport) -> Result<()> {
        self.run(
            Method::POST,
            &["Sessions", "Playing", "Progress"],
            &[],
            Some(report_body(r)),
        )
        .await
    }

    /// WRITE `POST /Sessions/Playing/Stopped`: the server stores the resume position and may mark played.
    pub async fn report_playback_stopped(&self, r: &PlaybackReport) -> Result<()> {
        self.run(
            Method::POST,
            &["Sessions", "Playing", "Stopped"],
            &[],
            Some(report_body(r)),
        )
        .await
    }

    // ---- USER STATE (WRITE) ----

    /// WRITE `POST /UserPlayedItems/{id}`.
    pub async fn mark_played(&self, item: &str) -> Result<()> {
        self.run(
            Method::POST,
            &["UserPlayedItems", item],
            &[self.uid()],
            None,
        )
        .await
    }

    /// WRITE `DELETE /UserPlayedItems/{id}`.
    pub async fn mark_unplayed(&self, item: &str) -> Result<()> {
        self.run(
            Method::DELETE,
            &["UserPlayedItems", item],
            &[self.uid()],
            None,
        )
        .await
    }

    /// WRITE `POST|DELETE /UserFavoriteItems/{id}`.
    pub async fn set_favorite(&self, item: &str, favorite: bool) -> Result<()> {
        let m = if favorite {
            Method::POST
        } else {
            Method::DELETE
        };
        self.run(m, &["UserFavoriteItems", item], &[self.uid()], None)
            .await
    }

    // ---- PLAYLISTS (the user's own) ----

    /// WRITE `POST /Playlists`; returns the new playlist id.
    pub async fn create_playlist(&self, name: &str, item_ids: &[&str]) -> Result<String> {
        let body = json!({"Name": name, "Ids": item_ids, "UserId": self.user_id});
        let c: Created = self
            .json(Method::POST, &["Playlists"], &[], Some(body))
            .await?;
        Ok(c.id)
    }

    /// WRITE `POST /Playlists/{id}/Items?ids=`.
    pub async fn add_to_playlist(&self, playlist: &str, item_ids: &[&str]) -> Result<()> {
        let p = [("ids", item_ids.join(",")), self.uid()];
        self.run(Method::POST, &["Playlists", playlist, "Items"], &p, None)
            .await
    }

    /// WRITE `DELETE /Playlists/{id}/Items?entryIds=` (entry ids from [`Self::playlist_items`]).
    pub async fn remove_from_playlist(&self, playlist: &str, entry_ids: &[&str]) -> Result<()> {
        let p = [("entryIds", entry_ids.join(","))];
        self.run(Method::DELETE, &["Playlists", playlist, "Items"], &p, None)
            .await
    }

    /// WRITE `POST /Playlists/{id}/Items/{entryId}/Move/{newIndex}`.
    pub async fn move_playlist_item(
        &self,
        playlist: &str,
        entry_id: &str,
        new_index: u32,
    ) -> Result<()> {
        let i = new_index.to_string();
        let segs = ["Playlists", playlist, "Items", entry_id, "Move", &i];
        self.run(Method::POST, &segs, &[], None).await
    }

    /// READ `GET /Playlists/{id}/Items`.
    pub async fn playlist_items(&self, playlist: &str) -> Result<Vec<PlaylistEntry>> {
        let p = [self.uid()];
        let page: EntryPage = self
            .json(Method::GET, &["Playlists", playlist, "Items"], &p, None)
            .await?;
        Ok(page.items)
    }

    /// WRITE `DELETE /Items/{id}`: delete one of the user's own playlists.
    pub async fn delete_playlist(&self, playlist: &str) -> Result<()> {
        self.run(Method::DELETE, &["Items", playlist], &[], None)
            .await
    }

    // ---- OWN ACCOUNT ----

    /// WRITE `POST /Users/Password?userId=`: change the logged-in user's own password.
    pub async fn change_own_password(
        &self,
        current: &str,
        new: &str,
        _confirmed: UserConfirmed,
    ) -> Result<()> {
        let body = json!({"CurrentPw": current, "NewPw": new});
        self.run(
            Method::POST,
            &["Users", "Password"],
            &[self.uid()],
            Some(body),
        )
        .await
    }
}

fn report_body(r: &PlaybackReport) -> Value {
    json!({
        "ItemId": r.item_id,
        "MediaSourceId": r.media_source_id,
        "PositionTicks": r.position_ticks,
        "IsPaused": r.is_paused,
        "PlayMethod": "DirectPlay",
        "CanSeek": true,
        "AudioStreamIndex": r.audio_stream_index,
        "SubtitleStreamIndex": r.subtitle_stream_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            access_token: "s3cret-token".into(),
            user_id: "u".into(),
            server_id: "s".into(),
            user_name: "n".into(),
            is_admin: false,
        }
    }

    #[test]
    fn debug_redacts_secrets() {
        let c = UserClient::from_session("http://jf.test:8096/", &session()).unwrap();
        let q = QuickConnect {
            code: "123456".into(),
            secret: "qc-secret".into(),
        };
        let d = format!("{c:?} {:?} {q:?}", session());
        assert!(!d.contains("s3cret-token") && !d.contains("qc-secret") && d.contains("123456"));
    }

    #[test]
    fn rejects_bad_urls() {
        for bad in ["nope", "ftp://h", ""] {
            assert!(
                matches!(UserClient::anonymous(bad), Err(Error::Config(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn deserializes_auth_and_entries() {
        let a: AuthResult = serde_json::from_str(
            r#"{"AccessToken":"t","ServerId":"s","User":{"Id":"u","Name":"alice","Policy":{"IsAdministrator":true,"X":1}},"Y":2}"#,
        )
        .unwrap();
        let s: Session = a.into();
        assert_eq!(
            (s.user_id.as_str(), s.user_name.as_str(), s.is_admin),
            ("u", "alice", true)
        );
        let p: EntryPage =
            serde_json::from_str(r#"{"Items":[{"Id":"i","Name":"n","PlaylistItemId":"e"}]}"#)
                .unwrap();
        assert_eq!(
            (p.items[0].entry_id.as_str(), p.items[0].item_id.as_str()),
            ("e", "i")
        );
    }

    #[test]
    fn report_body_shape() {
        let b = report_body(&PlaybackReport {
            item_id: "i".into(),
            position_ticks: 5,
            ..Default::default()
        });
        assert_eq!(
            (b["PlayMethod"].as_str(), b["PositionTicks"].as_u64()),
            (Some("DirectPlay"), Some(5))
        );
    }
}
