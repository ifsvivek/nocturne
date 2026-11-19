//! Read-only checks of the catalog against a THROWAWAY Jellyfin (`scripts/test-jellyfin.sh start`).
//! `NOCTURNE_JF_TEST_ENV` names the env file; a second server (`NOCTURNE_JF_TEST_PORT=8098`, its own
//! `NOCTURNE_JF_TEST_DIR`) can be named by `NOCTURNE_JF_TEST_ENV_2`. Only loopback URLs are accepted, and
//! the real server and the repo `.env` are never read. Nothing is written; output is counts only.
use nocturne_core::backend::Backend;
use nocturne_core::catalog::{Catalog, Env, Status};
use nocturne_core::merge::{Preferred, SeriesIds, merge};
use nocturne_core::profile::{Profile, ProfileKind};
use nocturne_core::source::SourceId;
use nocturne_jellyfin::BrowseQuery;

fn creds(var: &str) -> Option<(String, String)> {
    let path = std::env::var(var).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .map(str::to_string)
    };
    let (url, token) = (get("NOCTURNE_JF_TEST_URL")?, get("NOCTURNE_JF_TEST_TOKEN")?);
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "live tests only talk to a loopback throwaway server"
    );
    Some((url, token))
}

async fn profile(id: u32, url: &str, token: &str) -> Profile {
    let cfg = nocturne_jellyfin::Config {
        base_url: url.into(),
        token: token.into(),
    };
    let user = nocturne_core::source::Source::connect(Some(cfg))
        .await
        .unwrap_or_else(|e| panic!("throwaway server answers: {e:?}"))
        .client()
        .1;
    Profile {
        id: SourceId(id),
        label: format!("s{id}"),
        enabled: true,
        online: Default::default(),
        server_ui: false,
        kind: ProfileKind::Jellyfin {
            url: url.into(),
            user_id: user,
            user_name: "test".into(),
        },
    }
}

#[tokio::test]
#[ignore = "needs a throwaway Jellyfin: NOCTURNE_JF_TEST_ENV"]
async fn every_list_read_still_answers_with_the_provider_id_field() {
    let (url, token) = creds("NOCTURNE_JF_TEST_ENV").expect("NOCTURNE_JF_TEST_ENV");
    let mut profiles = vec![profile(1, &url, &token).await];
    let mut tokens = vec![(url.clone(), token.clone())];
    if let Some((u2, t2)) = creds("NOCTURNE_JF_TEST_ENV_2") {
        profiles.push(profile(2, &u2, &t2).await);
        tokens.push((u2, t2));
    }
    let lookup = |u: &str, _: &str| tokens.iter().find(|(x, _)| x == u).map(|(_, t)| t.clone());
    let dir = std::env::temp_dir().join(format!("nocturne-live-{}", std::process::id()));
    let c = Catalog::from_profiles(
        &profiles,
        &Env {
            token: &lookup,
            data_dir: &dir,
            disk: None,
        },
    );
    let mut results = vec![];
    for slot in c.online() {
        let b: &Backend = slot.backend.as_ref().unwrap();
        assert_eq!(slot.status, Status::Connecting);
        let libs = b.libraries().await.expect("libraries");
        b.resume().await.expect("resume");
        b.next_up().await.expect("next up");
        let (items, total) = b
            .browse(&BrowseQuery {
                recursive: true,
                limit: Some(50),
                include_types: vec!["Movie".into(), "Series".into()],
                ..Default::default()
            })
            .await
            .expect("browse");
        for l in &libs {
            b.latest(l).await.expect("latest");
        }
        assert!(items.iter().all(|m| m.origin == slot.id));
        println!(
            "source {}: {} libraries, {total} titles",
            slot.id.0,
            libs.len()
        );
        results.push((slot.id, items));
    }
    let prio = c.priority();
    let n: usize = results.iter().map(|(_, v)| v.len()).sum();
    let entries = merge(results, &prio, &Preferred::default(), &SeriesIds::default());
    assert!(entries.len() <= n);
    let _ = std::fs::remove_dir_all(dir);
}
