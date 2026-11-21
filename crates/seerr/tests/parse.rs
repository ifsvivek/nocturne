//! Recorded (sanitised) Seerr 3.5.2 JSON must parse, and unknown or missing fields must not break it.
use nocturne_seerr::*;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn parse<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_str(&fixture(name)).unwrap()
}

#[test]
fn search_results_with_and_without_media_info() {
    let p: SearchPage = parse("search.json");
    assert_eq!((p.page, p.total_results, p.results.len()), (1, 1616, 3));
    let (movie, tv, person) = (&p.results[0], &p.results[1], &p.results[2]);
    assert_eq!(
        (movie.kind(), movie.id, movie.year()),
        (Some(Kind::Movie), 348, Some(1979))
    );
    assert_eq!(movie.media_info.as_ref().unwrap().status, 2);
    assert_eq!(
        (tv.kind(), tv.display_title().as_str(), tv.year()),
        (Some(Kind::Tv), "Breaking Bad", Some(2008))
    );
    assert!(tv.media_info.is_none());
    assert_eq!(person.kind(), None, "people are skipped by the callers");
}

#[test]
fn details_carry_seasons_and_state() {
    let m: Detail = parse("movie_detail.json");
    let info = m.media_info.unwrap();
    assert_eq!(
        (info.status, info.requests[0].status, info.requests[0].id),
        (3, 2, 2)
    );
    let t: Detail = parse("tv_detail.json");
    let nums: Vec<_> = t
        .seasons
        .iter()
        .map(|s| (s.season_number, s.episode_count))
        .collect();
    assert_eq!(nums[..2], [(0, 9), (1, 7)]);
    let info = t.media_info.unwrap();
    assert_eq!(
        info.seasons.iter().map(|s| s.status).collect::<Vec<_>>(),
        [5, 3]
    );
    assert_eq!(info.requests[0].seasons[0].season_number, 2);
}

#[test]
fn requests_page_quota_me_and_settings() {
    let p: RequestsPage = parse("requests_page.json");
    assert_eq!((p.page_info.results, p.results.len()), (3, 3));
    assert_eq!(
        (p.results[0].media_kind(), p.results[0].tmdb_id()),
        (Some(Kind::Tv), Some(1396))
    );
    assert_eq!(p.results[0].seasons.len(), 2);
    let q: Quota = parse("quota_restricted.json");
    assert_eq!(
        (q.movie.restricted, q.movie.remaining, q.movie.days),
        (true, Some(0), Some(7))
    );
    assert!(!q.tv.restricted);
    let q: Quota = parse("quota_free.json");
    assert_eq!((q.tv.remaining, q.tv.days), (Some(2), Some(30)));
    let me: Me = parse("me.json");
    assert_eq!(
        (me.id, me.permissions, me.name().as_str()),
        (2, 32, "alice")
    );
    assert!(
        !format!("{me:?}").contains("REDACTED"),
        "the echoed Jellyfin token has no field"
    );
    let s: PublicSettings = parse("public_settings.json");
    assert!(s.partial_requests_enabled && !s.movie_4k_enabled && s.media_server_type == 2);
}

#[test]
fn a_created_request_parses_and_odd_json_does_not_panic() {
    let r: Request = parse("request_created.json");
    assert_eq!((r.id, r.status, r.media_kind()), (2, 1, Some(Kind::Movie)));
    // Missing and unknown fields, nulls where text is expected.
    let r: Request =
        serde_json::from_str(r#"{"id":7,"future":{"a":1},"media":null,"seasons":[]}"#).unwrap();
    assert_eq!((r.id, r.status, r.tmdb_id()), (7, 0, None));
    let h: SearchHit =
        serde_json::from_str(r#"{"id":1,"mediaType":"movie","title":null,"releaseDate":""}"#)
            .unwrap();
    assert_eq!((h.display_title().as_str(), h.year()), ("", None));
    let _: SearchPage = serde_json::from_str("{}").unwrap();
}

#[test]
fn permission_bits() {
    use perm::*;
    assert!(has(ADMIN, MANAGE_REQUESTS), "admin passes everything");
    assert!(has(REQUEST, REQUEST) && !has(REQUEST, MANAGE_REQUESTS) && !has(0, REQUEST));
}
