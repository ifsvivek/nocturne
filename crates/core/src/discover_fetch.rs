//! Reading the Discover lists: Seerr's discover and search, TMDB with the user's own key, Trakt's
//! public lists, and Radarr / Sonarr lookup for searches. Reads only. The request building is pure
//! (and tested); the calls run on the tokio bridge like every network future (`bridge::spawn`,
//! wrapped by the UI in a task). What is sent: the list or search text asked for and, for TMDB and
//! Trakt, the IP address. Never a library title, a file name or a server address.
use crate::discover::{
    self as d, Card, Filter, List, Scope, Src, card_from_lookup, cards_from_hits, cards_from_tmdb,
    cards_from_trakt,
};
use crate::online::{Endpoints, Engine};
use crate::trakt::{self, Trakt};
use nocturne_arr::{ArrClient, Service};
use nocturne_seerr::{DiscoverList, DiscoverQuery, Kind, SeerrClient};
use std::sync::{Arc, OnceLock};

/// What can be read from; the caller fills what is connected.
#[derive(Clone, Default)]
pub struct Sources {
    pub seerr: Option<Arc<SeerrClient>>,
    pub tmdb_key: Option<String>,
    pub trakt: Option<Arc<Trakt>>,
    pub arr: Vec<Arc<ArrClient>>,
}

impl Sources {
    pub fn have(&self) -> d::Have {
        d::Have {
            seerr: self.seerr.is_some(),
            tmdb: self.tmdb_key.is_some(),
            trakt: self.trakt.is_some(),
            arr: !self.arr.is_empty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fail {
    /// Seerr says the sign-in ended.
    SignedOut,
    /// A plain sentence for the user.
    Other(String),
}

fn other(src: Src) -> Fail {
    Fail::Other(format!(
        "{} did not answer. Try again in a moment.",
        src.label()
    ))
}

/// Debug builds may point TMDB and its images at a mock (`NOCTURNE_TMDB_URL`, `NOCTURNE_TMDB_IMG_URL`); a
/// release build never leaves themoviedb.org.
pub fn endpoints() -> Endpoints {
    let mut e = Endpoints::default();
    if cfg!(debug_assertions) {
        if let Ok(u) = std::env::var("NOCTURNE_TMDB_URL") {
            e.tmdb = u;
        }
        if let Ok(u) = std::env::var("NOCTURNE_TMDB_IMG_URL") {
            e.tmdb_img = u;
        }
    }
    e
}

fn engine() -> &'static Engine {
    static E: OnceLock<Engine> = OnceLock::new();
    E.get_or_init(|| Engine::new(endpoints(), Arc::default()))
}

type Q = Vec<(String, String)>;

/// The TMDB calls for a list or a search: (kind when the endpoint has only one, path, query).
pub fn tmdb_calls(f: &Filter, page: u32, today: &str) -> Vec<(Option<Kind>, String, Q)> {
    let p = |k: &str, v: String| (k.to_string(), v);
    let page = p("page", page.max(1).to_string());
    if f.searching() {
        let q = p("query", f.query.trim().to_string());
        return match f.scope {
            Scope::All => vec![(None, "/search/multi".into(), vec![q, page])],
            s => s
                .kinds()
                .iter()
                .map(|k| {
                    (
                        Some(*k),
                        format!("/search/{}", k.as_str()),
                        vec![q.clone(), page.clone()],
                    )
                })
                .collect(),
        };
    }
    // Genre and year only exist on the discover endpoint: they take over the Popular list.
    let list = if f.narrowed() { List::Popular } else { f.list };
    if list == List::Trending {
        let kind = match f.scope {
            Scope::All => "all",
            Scope::Movies => "movie",
            Scope::Shows => "tv",
        };
        let fixed = match f.scope {
            Scope::All => None,
            Scope::Movies => Some(Kind::Movie),
            Scope::Shows => Some(Kind::Tv),
        };
        return vec![(fixed, format!("/trending/{kind}/week"), vec![page])];
    }
    f.scope
        .kinds()
        .iter()
        .map(|k| {
            let tv = *k == Kind::Tv;
            let mut q = vec![page.clone(), p("sort_by", "popularity.desc".into())];
            if let Some(g) = f.genre {
                q.push(p("with_genres", g.to_string()));
            }
            if let Some(y) = f.year {
                q.push(p(
                    if tv {
                        "first_air_date_year"
                    } else {
                        "primary_release_year"
                    },
                    y.to_string(),
                ));
            }
            if list == List::Upcoming {
                q.push(p(
                    if tv {
                        "first_air_date.gte"
                    } else {
                        "primary_release_date.gte"
                    },
                    today.to_string(),
                ));
            }
            (Some(*k), format!("/discover/{}", k.as_str()), q)
        })
        .collect()
}

/// The Trakt calls: (kind when the endpoint has only one, path, query).
pub fn trakt_calls(f: &Filter, page: u32) -> Vec<(Option<Kind>, String, Q)> {
    let p = |k: &str, v: String| (k.to_string(), v);
    let mut q = vec![
        p("extended", "full".into()),
        p("page", page.max(1).to_string()),
        p("limit", "30".into()),
    ];
    if f.searching() {
        let types: Vec<&str> = f
            .scope
            .kinds()
            .iter()
            .map(|k| if *k == Kind::Tv { "show" } else { "movie" })
            .collect();
        q.push(p("query", f.query.trim().to_string()));
        return vec![(None, format!("/search/{}", types.join(",")), q)];
    }
    let list = if f.narrowed() { List::Popular } else { f.list };
    if let Some(y) = f.year {
        q.push(p("years", y.to_string()));
    }
    if let Some(g) = f.genre
        && let Some(slug) = d::genres(f.scope)
            .iter()
            .find(|x| x.id == g)
            .map(|x| x.slug)
    {
        q.push(p("genres", slug.to_string()));
    }
    f.scope
        .kinds()
        .iter()
        .map(|k| {
            let noun = if *k == Kind::Tv { "shows" } else { "movies" };
            let which = match list {
                List::Trending => "trending",
                List::Popular => "popular",
                List::Upcoming => "anticipated",
            };
            (Some(*k), format!("/{noun}/{which}"), q.clone())
        })
        .collect()
}

fn seerr_lists(f: &Filter) -> Vec<DiscoverList> {
    let list = if f.narrowed() { List::Popular } else { f.list };
    match list {
        List::Trending => vec![DiscoverList::Trending],
        List::Popular => f
            .scope
            .kinds()
            .iter()
            .map(|k| {
                if *k == Kind::Tv {
                    DiscoverList::Tv
                } else {
                    DiscoverList::Movies
                }
            })
            .collect(),
        List::Upcoming => f
            .scope
            .kinds()
            .iter()
            .map(|k| {
                if *k == Kind::Tv {
                    DiscoverList::UpcomingTv
                } else {
                    DiscoverList::UpcomingMovies
                }
            })
            .collect(),
    }
}

/// One page of cards for the filter from `src`: the list, or the search when there is a query. Movies
/// and shows of one list are interleaved. Scope and year are applied to what a source cannot filter.
pub async fn load(src: Src, s: &Sources, f: &Filter, page: u32) -> Result<Vec<Card>, Fail> {
    let cards = match src {
        Src::Seerr => seerr(s, f, page).await?,
        Src::Tmdb => tmdb(s, f, page).await?,
        Src::Trakt => trakt_load(s, f, page).await?,
        Src::Arr => arr(s, f).await?,
    };
    Ok(d::narrow(d::dedup(cards), f))
}

async fn seerr(s: &Sources, f: &Filter, page: u32) -> Result<Vec<Card>, Fail> {
    let c = s.seerr.as_ref().ok_or_else(|| other(Src::Seerr))?;
    let map = |e: nocturne_seerr::Error| match e {
        nocturne_seerr::Error::Unauthorized => Fail::SignedOut,
        _ => other(Src::Seerr),
    };
    if f.searching() {
        let mut p = c.search(f.query.trim()).await.map_err(map)?;
        if p.results.is_empty() {
            // Seerr's first TMDB call after a start now and then answers nothing: ask once more.
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            p = c.search(f.query.trim()).await.map_err(map)?;
        }
        return Ok(cards_from_hits(&p.results));
    }
    let mut lists = vec![];
    for l in seerr_lists(f) {
        let q = DiscoverQuery {
            list: l,
            genre: f.genre,
            year: f.year,
            page,
        };
        lists.push(cards_from_hits(&c.discover(&q).await.map_err(map)?.results));
    }
    Ok(mix(lists))
}

fn mix(mut lists: Vec<Vec<Card>>) -> Vec<Card> {
    match lists.len() {
        2 => {
            let b = lists.pop().unwrap_or_default();
            d::interleave(lists.pop().unwrap_or_default(), b)
        }
        _ => lists.into_iter().flatten().collect(),
    }
}

async fn tmdb(s: &Sources, f: &Filter, page: u32) -> Result<Vec<Card>, Fail> {
    let key = s.tmdb_key.as_deref().ok_or_else(|| other(Src::Tmdb))?;
    let today = crate::calendar::today().iso();
    let mut lists = vec![];
    for (fixed, path, q) in tmdb_calls(f, page, &today) {
        let q: Vec<(&str, &str)> = q.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let v = engine()
            .tmdb_client(key)
            .raw(&path, &q)
            .await
            .map_err(|e| match e {
                crate::online::http::Err::Auth => {
                    Fail::Other("TMDB did not accept your key. Check it in Settings.".into())
                }
                _ => other(Src::Tmdb),
            })?;
        lists.push(cards_from_tmdb(&v, fixed));
    }
    Ok(mix(lists))
}

async fn trakt_load(s: &Sources, f: &Filter, page: u32) -> Result<Vec<Card>, Fail> {
    let t = s.trakt.as_ref().ok_or_else(|| other(Src::Trakt))?;
    let mut lists = vec![];
    for (fixed, path, q) in trakt_calls(f, page) {
        let v = t.public_get(&path, &q).await.map_err(|e| match e {
            trakt::Error::Auth => Fail::Other(
                "Trakt did not accept the connection. Connect it again in Settings.".into(),
            ),
            _ => other(Src::Trakt),
        })?;
        lists.push(cards_from_trakt(&v, fixed));
    }
    Ok(mix(lists))
}

async fn arr(s: &Sources, f: &Filter) -> Result<Vec<Card>, Fail> {
    if !f.searching() {
        return Ok(vec![]);
    }
    let mut lists = vec![];
    let mut failed = 0;
    for c in &s.arr {
        let tv = c.service() == Service::Sonarr;
        if !f.scope.admits(if tv { Kind::Tv } else { Kind::Movie }) {
            continue;
        }
        match c.lookup(f.query.trim()).await {
            Ok(v) => lists.push(v.iter().filter_map(|l| card_from_lookup(l, tv)).collect()),
            Err(_) => failed += 1,
        }
    }
    if lists.is_empty() && failed > 0 {
        return Err(other(Src::Arr));
    }
    Ok(mix(lists))
}

/// What a native request of a show needs to know: its seasons (number, episode count; 0 when unknown)
/// and its TVDB id (Sonarr's key).
#[derive(Clone, Debug, Default)]
pub struct TvInfo {
    pub seasons: Vec<(u32, u32)>,
    pub tvdb: Option<u64>,
}

/// Read a show's seasons and TVDB id: from TMDB with the user's key, else from Sonarr's lookup.
pub async fn tv_info(s: &Sources, tmdb: u64, title: &str, year: Option<u32>) -> Option<TvInfo> {
    if let Some(key) = s.tmdb_key.as_deref() {
        let v = engine()
            .tmdb_client(key)
            .raw(
                &format!("/tv/{tmdb}"),
                &[("append_to_response", "external_ids")],
            )
            .await
            .ok()?;
        return Some(parse_tv_info(&v));
    }
    // Sonarr's own lookup (the first connected one).
    let c = s.arr.iter().find(|c| c.service() == Service::Sonarr)?;
    let found = c.lookup(title).await.ok()?;
    let hit = found.iter().find(|l| l.tmdb_id == tmdb).or_else(|| {
        found.iter().find(|l| {
            l.title.eq_ignore_ascii_case(title) && (year.is_none() || l.year == year.unwrap_or(0))
        })
    })?;
    Some(TvInfo {
        seasons: hit.seasons.iter().map(|x| (x.season_number, 0)).collect(),
        tvdb: (hit.tvdb_id > 0).then_some(hit.tvdb_id),
    })
}

/// TMDB's `/tv/{id}?append_to_response=external_ids`.
pub fn parse_tv_info(v: &serde_json::Value) -> TvInfo {
    TvInfo {
        seasons: v["seasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some((
                    s["season_number"].as_u64()? as u32,
                    s["episode_count"].as_u64().unwrap_or(0) as u32,
                ))
            })
            .collect(),
        tvdb: v["external_ids"]["tvdb_id"].as_u64().filter(|n| *n > 0),
    }
}

/// A poster: a TMDB path (`/abc.jpg`, from TMDB's image host) or a full http(s) URL (Radarr / Sonarr
/// lookups). Sent without any credential. At most 2 MB.
pub async fn poster(spec: &str) -> Option<Vec<u8>> {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    let url = if spec.starts_with("http://") || spec.starts_with("https://") {
        spec.to_string()
    } else if spec.starts_with('/') && !spec.contains(['?', '#']) && !spec.contains("..") {
        format!("{}w342{spec}", endpoints().tmdb_img)
    } else {
        return None;
    };
    let c = C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(crate::online::http::user_agent())
            .build()
            .expect("http client")
    });
    let r = c
        .get(url)
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success())?;
    let b = r.bytes().await.ok()?;
    (b.len() <= 2 << 20).then(|| b.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(list: List, scope: Scope) -> Filter {
        Filter {
            list,
            scope,
            ..Default::default()
        }
    }

    #[test]
    fn tmdb_calls_per_list() {
        let c = tmdb_calls(&f(List::Trending, Scope::All), 1, "2026-11-27");
        assert_eq!(
            (c.len(), c[0].1.as_str(), c[0].0),
            (1, "/trending/all/week", None)
        );
        let c = tmdb_calls(&f(List::Trending, Scope::Shows), 2, "2026-11-27");
        assert_eq!(
            (c[0].1.as_str(), c[0].0),
            ("/trending/tv/week", Some(Kind::Tv))
        );
        assert!(c[0].2.contains(&("page".into(), "2".into())));
        let c = tmdb_calls(&f(List::Popular, Scope::All), 1, "2026-11-27");
        assert_eq!(
            c.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
            ["/discover/movie", "/discover/tv"]
        );
        let c = tmdb_calls(&f(List::Upcoming, Scope::Movies), 1, "2026-11-27");
        assert!(
            c[0].2
                .contains(&("primary_release_date.gte".into(), "2026-11-27".into()))
        );
        let c = tmdb_calls(&f(List::Upcoming, Scope::Shows), 1, "2026-11-27");
        assert!(c[0].2.iter().any(|(k, _)| k == "first_air_date.gte"));
    }

    #[test]
    fn genre_and_year_take_over_the_popular_list() {
        let filter = Filter {
            list: List::Trending,
            scope: Scope::Movies,
            genre: Some(28),
            year: Some(1999),
            ..Default::default()
        };
        let c = tmdb_calls(&filter, 1, "x");
        assert_eq!(c[0].1, "/discover/movie");
        assert!(c[0].2.contains(&("with_genres".into(), "28".into())));
        assert!(
            c[0].2
                .contains(&("primary_release_year".into(), "1999".into()))
        );
        assert_eq!(seerr_lists(&filter), [DiscoverList::Movies]);
        let t = trakt_calls(&filter, 1);
        assert_eq!(t[0].1, "/movies/popular");
        assert!(t[0].2.contains(&("genres".into(), "action".into())));
        assert!(t[0].2.contains(&("years".into(), "1999".into())));
    }

    #[test]
    fn searches_replace_the_lists() {
        let s = Filter {
            query: " alien ".into(),
            ..Default::default()
        };
        let c = tmdb_calls(&s, 1, "x");
        assert_eq!(c[0].1, "/search/multi");
        assert!(c[0].2.contains(&("query".into(), "alien".into())));
        let s = Filter {
            scope: Scope::Shows,
            ..s
        };
        assert_eq!(tmdb_calls(&s, 1, "x")[0].1, "/search/tv");
        let t = trakt_calls(&s, 1);
        assert_eq!(t[0].1, "/search/show");
        assert!(t[0].2.contains(&("query".into(), "alien".into())));
        assert_eq!(
            trakt_calls(
                &Filter {
                    query: "a".into(),
                    ..Default::default()
                },
                1
            )[0]
            .1,
            "/search/movie,show"
        );
    }

    #[test]
    fn trakt_lists() {
        let c = trakt_calls(&f(List::Upcoming, Scope::All), 1);
        assert_eq!(
            c.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
            ["/movies/anticipated", "/shows/anticipated"]
        );
        assert_eq!(
            trakt_calls(&f(List::Trending, Scope::Movies), 1)[0].1,
            "/movies/trending"
        );
    }

    #[test]
    fn seerr_lists_follow_the_scope() {
        assert_eq!(
            seerr_lists(&f(List::Trending, Scope::Movies)),
            [DiscoverList::Trending]
        );
        assert_eq!(
            seerr_lists(&f(List::Upcoming, Scope::All)),
            [DiscoverList::UpcomingMovies, DiscoverList::UpcomingTv]
        );
    }

    #[test]
    fn tv_info_reads_seasons_and_tvdb() {
        let v = serde_json::json!({"seasons":[{"season_number":0,"episode_count":2},{"season_number":1,"episode_count":7},{"name":"x"}],
            "external_ids":{"tvdb_id":81189,"imdb_id":"tt1"}});
        let i = parse_tv_info(&v);
        assert_eq!(i.seasons, [(0, 2), (1, 7)]);
        assert_eq!(i.tvdb, Some(81189));
        assert_eq!(parse_tv_info(&serde_json::json!({})).tvdb, None);
    }

    #[test]
    fn what_is_connected_is_reported() {
        let h = Sources::default().have();
        assert!(!h.seerr && !h.tmdb && !h.trakt && !h.arr);
        let s = Sources {
            tmdb_key: Some("k".into()),
            ..Default::default()
        };
        assert!(s.have().tmdb);
    }
}
