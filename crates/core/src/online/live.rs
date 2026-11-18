//! Live checks against the real TMDB and OpenSubtitles APIs, with the maintainer's own keys from the environment
//! (`NOCTURNE_TMDB_KEY`, `NOCTURNE_OPENSUBTITLES_KEY`). Ignored by default; they print booleans and counts only,
//! never a key. Uses a public-domain film (Night of the Living Dead, 1968, tt0063350).
//!
//!   NOCTURNE_TMDB_KEY=… cargo test -p nocturne-core online::live -- --ignored --nocapture

use super::*;

fn key(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|k| !k.trim().is_empty())
}

fn engine() -> Engine {
    Engine::new(Endpoints::default(), Arc::new(AtomicBool::new(false)))
}

#[tokio::test]
#[ignore]
async fn tmdb_live() {
    let Some(k) = key("NOCTURNE_TMDB_KEY") else {
        eprintln!("NOCTURNE_TMDB_KEY not set: skipped");
        return;
    };
    let e = engine();
    let valid = e.validate_tmdb(&k).await.is_ok();
    eprintln!("tmdb key accepted: {valid}");
    assert!(valid);
    let t = e.tmdb_client(&k);
    let hits = t
        .search(false, "Night of the Living Dead", Some(1968))
        .await
        .unwrap();
    let hit = hits.iter().find(|h| h.year == Some(1968));
    eprintln!(
        "search hits: {} (1968 film found: {})",
        hits.len(),
        hit.is_some()
    );
    assert!(hit.is_some());
    let by_imdb = t.find_imdb(false, "tt0063350").await.unwrap();
    eprintln!("find by imdb id: {} hit(s)", by_imdb.len());
    assert_eq!(
        by_imdb.first().map(|h| h.id.as_str()),
        hit.map(|h| h.id.as_str())
    );
    let d = t.details(false, &hit.unwrap().id).await.unwrap();
    eprintln!(
        "details: overview {}, poster {}, backdrop {}, imdb id {}",
        d.meta.plot.as_deref().is_some_and(|p| !p.is_empty()),
        d.poster.is_some(),
        d.backdrop.is_some(),
        d.meta.ids.imdb.as_deref() == Some("tt0063350"),
    );
    assert!(d.poster.is_some());
}

#[tokio::test]
#[ignore]
async fn opensubtitles_search_live() {
    let Some(k) = key("NOCTURNE_OPENSUBTITLES_KEY") else {
        eprintln!("NOCTURNE_OPENSUBTITLES_KEY not set: skipped");
        return;
    };
    let q = subs::Query {
        title: "Night of the Living Dead".into(),
        year: Some(1968),
        imdb: Some("0063350".into()),
        season: None,
        episode: None,
        langs: vec!["en".into()],
    };
    let hits = engine().find_subtitles(&k, &q).await;
    eprintln!("opensubtitles search ok: {}", hits.is_ok());
    let n = hits.map(|h| h.len()).unwrap_or(0);
    eprintln!("english subtitles found: {n}");
    assert!(n > 0);
}
