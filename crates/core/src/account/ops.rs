//! Account I/O helpers shared by the front ends: server probe, session persistence, login result
//! mapping, Quick Connect polling result mapping, profile-image checks. No UI types.
use super::state::*;
use nocturne_jellyfin_user::{Error, Session, UserClient};

/// Why a profile change did not go through (calm one-liners for the dialog).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ProfileFail {
    File,
    Image,
    Language,
    NotOwner,
    Server,
}

pub fn playlist_fail(e: Error) -> ProfileFail {
    match e {
        Error::Status { status: 403, .. } => ProfileFail::NotOwner,
        Error::Config(_) => ProfileFail::Language,
        _ => ProfileFail::Server,
    }
}

impl ProfileFail {
    pub fn text(self) -> &'static str {
        match self {
            ProfileFail::File => "That file couldn't be read. Check the path.",
            ProfileFail::Image => "That isn't a PNG, JPEG, WebP or GIF image of at most 10 MB.",
            ProfileFail::Language => "The server doesn't offer that language code.",
            ProfileFail::NotOwner => "You can only change playlists you own.",
            ProfileFail::Server => "Couldn't save that. Check the server and try again.",
        }
    }
}

/// The file's bytes, if it looks like an acceptable image (checked again before upload).
pub fn read_image(path: &str) -> Result<Vec<u8>, ProfileFail> {
    let p = std::path::Path::new(path);
    let len = std::fs::metadata(p).map_err(|_| ProfileFail::File)?.len();
    if len > nocturne_jellyfin_user::MAX_IMAGE_BYTES as u64 {
        return Err(ProfileFail::Image);
    }
    let bytes = std::fs::read(p).map_err(|_| ProfileFail::File)?;
    nocturne_jellyfin_user::image_mime(&bytes).map_err(|_| ProfileFail::Image)?;
    Ok(bytes)
}

pub fn classify(e: &Error) -> ConnectFail {
    match e {
        Error::Http(h) if !h.is_decode() => ConnectFail::Unreachable,
        _ => ConnectFail::NotServer,
    }
}

pub fn poll_of(r: Result<bool, Error>) -> Poll {
    match r {
        Ok(true) => Poll::Authorized,
        Ok(false) => Poll::Pending,
        Err(Error::Status {
            status: 400 | 401 | 404,
            ..
        }) => Poll::Gone,
        Err(_) => Poll::Hiccup,
    }
}

/// Is `url` a server, what is it called, and does the keyring hold a session that still works?
pub async fn probe(url: String, restore: bool) -> Result<(Probe, Option<Session>), ConnectFail> {
    let c = UserClient::anonymous(&url).map_err(|e| classify(&e))?;
    let info = c.public_info().await.map_err(|e| classify(&e))?;
    if info.id.is_empty() {
        return Err(ConnectFail::NotServer);
    }
    let mut p = Probe {
        name: info.server_name,
        quick_connect: c.quick_connect_enabled().await.unwrap_or(false),
        ..Default::default()
    };
    let u = url.clone();
    if !restore {
        return Ok((p, None));
    }
    match tokio::task::spawn_blocking(move || nocturne_session::load(&u)).await {
        Ok(Ok(Some(s))) => {
            let me = UserClient::from_session(&url, &s).map_err(|e| classify(&e))?;
            match me.me(s).await {
                Ok(s) => {
                    p.restored = true;
                    // Also list it as a remembered account (older installs only had the active one).
                    let (u, s2) = (url.clone(), s.clone());
                    let _ = tokio::task::spawn_blocking(move || {
                        nocturne_session::remember_login(&u, &s2)
                    })
                    .await;
                    return Ok((p, Some(s)));
                }
                Err(Error::Status { status: 401, .. }) => {
                    let _ = nocturne_session::delete(&url);
                    p.expired = true;
                }
                Err(_) => {} // can't tell right now: sign in again, the stored login stays
            }
        }
        Ok(Err(nocturne_session::Error::Unavailable(_))) => p.keyring_down = true,
        _ => {}
    }
    Ok((p, None))
}

/// Keyring write; `false` means no keyring (the session then lives in memory only).
pub async fn persist(url: &str, s: &Session) -> bool {
    let (url, s) = (url.to_string(), s.clone());
    matches!(
        tokio::task::spawn_blocking(move || nocturne_session::remember_login(&url, &s)).await,
        Ok(Ok(()))
    )
}

pub async fn finish(url: String, r: Result<Session, Error>) -> Result<(Session, bool), LoginFail> {
    match r {
        Ok(s) => {
            let saved = persist(&url, &s).await;
            Ok((s, saved))
        }
        Err(Error::InvalidCredentials) => Err(LoginFail::Invalid),
        Err(_) => Err(LoginFail::Unreachable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_images_are_checked_before_upload() {
        let dir = std::env::temp_dir().join(format!("nocturne-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 0, 1]).unwrap();
        let txt = dir.join("a.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(read_image(png.to_str().unwrap()).is_ok());
        assert_eq!(read_image(txt.to_str().unwrap()), Err(ProfileFail::Image));
        assert_eq!(read_image("/nonexistent/x.png"), Err(ProfileFail::File));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn error_classes() {
        let st = |s| Error::Status {
            status: s,
            path: String::new(),
        };
        assert_eq!(classify(&st(404)), ConnectFail::NotServer);
        assert_eq!(
            classify(&Error::Config(String::new())),
            ConnectFail::NotServer
        );
        assert_eq!(poll_of(Ok(true)), Poll::Authorized);
        assert_eq!(poll_of(Ok(false)), Poll::Pending);
        assert_eq!(poll_of(Err(st(404))), Poll::Gone);
        assert_eq!(poll_of(Err(st(500))), Poll::Hiccup);
    }
}
