//! Live Trakt check with a sign-in made in a NOCTURNE test profile (tokens in the keyring service named by
//! NOCTURNE_KEYRING_SERVICE). Ignored by default; prints booleans and counts only.
//!   NOCTURNE_KEYRING_SERVICE=nocturne-test cargo test -p nocturne-core --test trakt_live -- --ignored --nocapture
use nocturne_core::trakt::{Action, Ids, Target, Trakt};

#[tokio::test]
#[ignore]
async fn trakt_signed_in_round_trip() {
    let Some(t) = Trakt::load().await else {
        eprintln!("no Trakt sign-in in this keyring service: skipped");
        return;
    };
    let name = t.username().await;
    eprintln!("username read: {}", name.is_ok());
    assert!(name.is_ok());
    let h = t.history().await;
    eprintln!("history read: {}", h.is_ok());
    if let Ok(h) = &h {
        eprintln!("watched movies: {}, watched shows: {}", h.movies.len(), h.shows.len());
    }
    // A public-domain film (Night of the Living Dead, 1968). Stopping at 2% leaves a paused playback, not history.
    let film = Target::Movie(Ids { imdb: Some("tt0063350".into()), tmdb: Some(10331), tvdb: None });
    let start = t.scrobble(Action::Start, &film, 1.0).await;
    eprintln!("scrobble start ok: {}", start.is_ok());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let stop = t.scrobble(Action::Stop, &film, 2.0).await;
    eprintln!("scrobble stop ok: {}", stop.is_ok());
    assert!(start.is_ok() && stop.is_ok() && h.is_ok());
}
