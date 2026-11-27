//! Live checks against the throwaway Radarr and Sonarr of `scripts/test-arr.sh` (ignored by default).
//! `ArrTestGuard::connect()` refuses anything that is not a loopback test instance with its data
//! under /tmp. Prints booleans and counts only, never the keys.
//!
//! ```sh
//! scripts/test-arr.sh start
//! NOCTURNE_ARR_TEST_ENV=/tmp/nocturne-arr-test/test.env cargo test -p nocturne-arr -- --ignored --test-threads=1
//! scripts/test-arr.sh stop
//! ```
use nocturne_arr::guard::ArrTestGuard;
use nocturne_arr::*;

fn clients() -> (ArrClient, ArrClient) {
    let env = ArrTestGuard::connect().expect("guard");
    (
        ArrClient::new(Service::Radarr, &env.radarr_url, &env.radarr_key).unwrap(),
        ArrClient::new(Service::Sonarr, &env.sonarr_url, &env.sonarr_key).unwrap(),
    )
}

#[tokio::test]
#[ignore = "needs the throwaway Radarr and Sonarr (scripts/test-arr.sh start)"]
async fn connect_and_read() {
    let (r, s) = clients();
    for c in [&r, &s] {
        let st = c.status().await.expect("status");
        let profiles = c.quality_profiles().await.expect("profiles").len();
        let folders = c.root_folders().await.expect("folders").len();
        println!(
            "{}: version ok={} profiles={profiles} root folders={folders}",
            c.service().name(),
            !st.version.is_empty()
        );
        assert!(profiles > 0 && folders > 0);
    }
    // a wrong key is refused
    let env = ArrTestGuard::connect().unwrap();
    let bad = ArrClient::new(Service::Radarr, &env.radarr_url, "wrong").unwrap();
    assert!(matches!(bad.status().await, Err(Error::Unauthorized)));
}

#[tokio::test]
#[ignore = "needs the throwaway Radarr (scripts/test-arr.sh start); adds a movie there"]
async fn add_a_movie_and_follow_it() {
    let (r, _) = clients();
    let profile = r.quality_profiles().await.unwrap()[0].id;
    let root = r.root_folders().await.unwrap()[0].path.clone();
    let hits = r.lookup("tmdb:603").await.expect("lookup");
    let hit = hits.first().expect("a lookup hit (needs internet)");
    let m = NewMovie {
        title: hit.title.clone(),
        year: hit.year,
        tmdb_id: hit.tmdb_id,
        profile_id: profile,
        root_folder: root,
    };
    let ok = ArrConfirmed::i_have_asked_the_user();
    match r.add_movie(&m, ok).await {
        Ok(()) | Err(Error::Rejected(_)) => {}
        Err(e) => panic!("add failed: {e}"),
    }
    // adding twice is refused with the service's own words
    assert!(matches!(r.add_movie(&m, ok).await, Err(Error::Rejected(_))));
    let p = r.progress(m.tmdb_id).await.unwrap();
    println!("progress known={}", p != Progress::Unknown);
    assert_ne!(p, Progress::Unknown);
}

#[tokio::test]
#[ignore = "needs the throwaway Sonarr (scripts/test-arr.sh start); adds a show there"]
async fn add_a_series_with_chosen_seasons() {
    let (_, s) = clients();
    let profile = s.quality_profiles().await.unwrap()[0].id;
    let root = s.root_folders().await.unwrap()[0].path.clone();
    let hits = s.lookup("tvdb:81189").await.expect("lookup");
    let hit = hits.first().expect("a lookup hit (needs internet)");
    let seasons: Vec<SeasonInfo> = hit
        .seasons
        .iter()
        .map(|x| SeasonInfo {
            season_number: x.season_number,
            monitored: x.season_number == 1,
        })
        .collect();
    let n = NewSeries {
        title: hit.title.clone(),
        tvdb_id: hit.tvdb_id,
        profile_id: profile,
        root_folder: root,
        seasons,
    };
    match s
        .add_series(&n, ArrConfirmed::i_have_asked_the_user())
        .await
    {
        Ok(()) | Err(Error::Rejected(_)) => {}
        Err(e) => panic!("add failed: {e}"),
    }
    assert_ne!(s.progress(n.tvdb_id).await.unwrap(), Progress::Unknown);
}
