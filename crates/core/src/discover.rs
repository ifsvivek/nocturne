//! The Requests page's Discover view, minus the window and the network: which data source serves
//! it, the card model every source is turned into, the filters, and the status a card shows
//! (on your servers, requested, pending, not requested). Pure, so it is unit tested offline; the
//! fetching is `discover_fetch.rs`, the painting the UI crate's `discover*.rs`.
//! Discovery reads only. Nothing here sends a request; that is the request dialog.
use crate::requests::Badge;
use crate::source::{Kind as MediaKind, Media};
use crate::wishlist;
use nocturne_arr::Lookup;
use nocturne_seerr::{Kind, MediaInfo, SearchHit};
use serde_json::Value;

/// Where a list comes from, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Src {
    /// The signed-in Seerr server's own discover and search.
    Seerr,
    /// TMDB with the user's own key.
    Tmdb,
    /// Trakt's public lists (needs the Trakt connection).
    Trakt,
    /// Radarr's / Sonarr's lookup: search only, no browse lists.
    Arr,
}

impl Src {
    pub fn label(self) -> &'static str {
        match self {
            Self::Seerr => "Seerr",
            Self::Tmdb => "TMDB",
            Self::Trakt => "Trakt",
            Self::Arr => "Radarr / Sonarr",
        }
    }
}

/// What is connected right now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Have {
    pub seerr: bool,
    pub tmdb: bool,
    pub trakt: bool,
    pub arr: bool,
}

/// The source to use: Seerr, then TMDB, then Trakt; for a search Radarr / Sonarr also count, last.
pub fn pick(h: Have, searching: bool) -> Option<Src> {
    if h.seerr {
        Some(Src::Seerr)
    } else if h.tmdb {
        Some(Src::Tmdb)
    } else if h.trakt {
        Some(Src::Trakt)
    } else if h.arr && searching {
        Some(Src::Arr)
    } else {
        None
    }
}

/// What the Discover view says when there is nothing to read from.
pub fn connect_text(h: Have) -> &'static str {
    if h.arr {
        "Browsing needs a Seerr server, your own TMDB key or Trakt. Radarr and Sonarr can search by name: type a title above. Connect one of the others in Settings to browse trending and popular titles."
    } else {
        "Discover needs a source of titles. In Settings, sign in to a Seerr server, add your own TMDB key (Settings, your sources, online details), or connect Trakt. Radarr or Sonarr alone can search."
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum List {
    #[default]
    Trending,
    Popular,
    Upcoming,
}

impl List {
    pub const ALL: [List; 3] = [List::Trending, List::Popular, List::Upcoming];

    pub fn label(self) -> &'static str {
        match self {
            Self::Trending => "Trending",
            Self::Popular => "Popular",
            Self::Upcoming => "Upcoming",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    #[default]
    All,
    Movies,
    Shows,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::All, Scope::Movies, Scope::Shows];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Movies => "Movies",
            Self::Shows => "Shows",
        }
    }

    pub fn kinds(self) -> &'static [Kind] {
        match self {
            Self::All => &[Kind::Movie, Kind::Tv],
            Self::Movies => &[Kind::Movie],
            Self::Shows => &[Kind::Tv],
        }
    }

    pub fn admits(self, k: Kind) -> bool {
        self.kinds().contains(&k)
    }
}

/// A genre filter: TMDB's id (which differs between movies and shows for a few), the label and
/// Trakt's slug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Genre {
    pub id: u32,
    pub name: &'static str,
    pub slug: &'static str,
}

const G: fn(u32, &'static str, &'static str) -> Genre = |id, name, slug| Genre { id, name, slug };

/// The genres offered for a scope (the ones both kinds share, plus those of the chosen kind).
pub fn genres(scope: Scope) -> Vec<Genre> {
    let mut v = vec![
        G(16, "Animation", "animation"),
        G(35, "Comedy", "comedy"),
        G(80, "Crime", "crime"),
        G(99, "Documentary", "documentary"),
        G(18, "Drama", "drama"),
        G(10751, "Family", "family"),
        G(9648, "Mystery", "mystery"),
    ];
    if scope != Scope::Shows {
        v.extend([
            G(28, "Action", "action"),
            G(12, "Adventure", "adventure"),
            G(14, "Fantasy", "fantasy"),
            G(27, "Horror", "horror"),
            G(10749, "Romance", "romance"),
            G(878, "Science Fiction", "science-fiction"),
            G(53, "Thriller", "thriller"),
        ]);
    }
    if scope == Scope::Shows {
        v.extend([
            G(10759, "Action and Adventure", "action"),
            G(10765, "Sci-Fi and Fantasy", "science-fiction"),
            G(10764, "Reality", "reality"),
        ]);
    }
    v.sort_by_key(|g| g.name);
    v
}

/// Years offered by the year filter: this year back to 1990 (descending).
pub fn years(this_year: u32) -> Vec<u32> {
    (1990..=this_year + 1).rev().collect()
}

/// What the user asked the Discover view to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub list: List,
    pub scope: Scope,
    pub genre: Option<u32>,
    pub year: Option<u32>,
    /// A typed search replaces the lists.
    pub query: String,
}

impl Filter {
    /// Genre and year narrow the Popular list (the sources filter only there); choosing either moves to it.
    pub fn narrowed(&self) -> bool {
        self.genre.is_some() || self.year.is_some()
    }

    pub fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

/// One title in the grid, whichever source it came from.
#[derive(Clone, Debug, Default)]
pub struct Card {
    pub tv: bool,
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
    pub imdb: Option<String>,
    pub title: String,
    pub year: Option<u32>,
    pub rating: Option<f32>,
    pub overview: String,
    /// A TMDB poster path (`/abc.jpg`) or a full URL.
    pub poster: Option<String>,
    /// Seerr's own state of the title (Seerr cards only).
    pub info: Option<MediaInfo>,
}

impl Card {
    pub fn kind(&self) -> Kind {
        if self.tv { Kind::Tv } else { Kind::Movie }
    }

    /// A stable key for caches and de-duplication.
    pub fn key(&self) -> String {
        let k = if self.tv { "tv" } else { "movie" };
        match (&self.tmdb, &self.tvdb, &self.imdb) {
            (Some(t), ..) => format!("{k}:tmdb:{t}"),
            (None, Some(t), _) => format!("{k}:tvdb:{t}"),
            (None, None, Some(i)) => format!("{k}:imdb:{i}"),
            _ => format!("{k}:{}:{:?}", self.title.to_lowercase(), self.year),
        }
    }

    /// Can a request for it be made (it needs a TMDB id for Seerr and the native list)?
    pub fn requestable(&self) -> bool {
        self.tmdb.is_some()
    }
}

/// Keep the first of each title, in order.
pub fn dedup(cards: Vec<Card>) -> Vec<Card> {
    let mut seen = std::collections::HashSet::new();
    cards.into_iter().filter(|c| seen.insert(c.key())).collect()
}

/// Alternate two lists (movies and shows read as one mixed grid).
pub fn interleave(a: Vec<Card>, b: Vec<Card>) -> Vec<Card> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut a, mut b) = (a.into_iter(), b.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return out,
            (x, y) => out.extend(x.into_iter().chain(y)),
        }
    }
}

/// Apply the search-side filters a source cannot (scope and year).
pub fn narrow(cards: Vec<Card>, f: &Filter) -> Vec<Card> {
    cards
        .into_iter()
        .filter(|c| f.scope.admits(c.kind()))
        .filter(|c| f.year.is_none_or(|y| c.year == Some(y)))
        .collect()
}

// ---- turning the sources' answers into cards ----

fn year_of(s: Option<&str>) -> Option<u32> {
    s?.get(..4)?.parse().ok()
}

/// Seerr's discover / search hits (people and unknown kinds dropped).
pub fn cards_from_hits(hits: &[SearchHit]) -> Vec<Card> {
    hits.iter()
        .filter_map(|h| {
            let kind = h.kind()?;
            Some(Card {
                tv: kind == Kind::Tv,
                tmdb: Some(h.id),
                title: h.display_title(),
                year: h.year(),
                rating: h.vote_average.filter(|r| *r > 0.0),
                overview: h.overview.clone().unwrap_or_default(),
                poster: h.poster_path.clone(),
                info: h.media_info.clone(),
                ..Default::default()
            })
        })
        .collect()
}

/// TMDB's `results[]`. `fixed` names the kind when the endpoint is movie-only or tv-only; otherwise each
/// result's `media_type` decides (people are dropped).
pub fn cards_from_tmdb(v: &Value, fixed: Option<Kind>) -> Vec<Card> {
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let kind = fixed.or_else(|| Kind::parse(r["media_type"].as_str()?))?;
            let tv = kind == Kind::Tv;
            let (t, d) = if tv {
                ("name", "first_air_date")
            } else {
                ("title", "release_date")
            };
            let text = |k: &str| {
                r[k].as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            };
            Some(Card {
                tv,
                tmdb: Some(r["id"].as_u64()?),
                title: text(t)?,
                year: year_of(r[d].as_str()),
                rating: r["vote_average"]
                    .as_f64()
                    .filter(|x| *x > 0.0)
                    .map(|x| x as f32),
                overview: text("overview").unwrap_or_default(),
                poster: text("poster_path"),
                ..Default::default()
            })
        })
        .collect()
}

/// Trakt's list shapes: `[{title, year, ids}]` (popular), `[{"movie"|"show": {...}}]` (trending,
/// anticipated, search). `fixed` is the kind of a movie-only or show-only endpoint.
pub fn cards_from_trakt(v: &Value, fixed: Option<Kind>) -> Vec<Card> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let (kind, o) = if let Some(m) = item.get("movie") {
                (Kind::Movie, m)
            } else if let Some(s) = item.get("show") {
                (Kind::Tv, s)
            } else {
                (fixed?, item)
            };
            let ids = &o["ids"];
            let num = |k: &str| ids[k].as_u64();
            let imdb = ids["imdb"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from);
            let (tmdb, tvdb) = (num("tmdb"), num("tvdb"));
            if tmdb.is_none() && tvdb.is_none() && imdb.is_none() {
                return None;
            }
            Some(Card {
                tv: kind == Kind::Tv,
                tmdb,
                tvdb,
                imdb,
                title: o["title"].as_str()?.trim().to_string(),
                year: o["year"].as_u64().map(|y| y as u32),
                rating: o["rating"].as_f64().filter(|x| *x > 0.0).map(|x| x as f32),
                overview: o["overview"].as_str().unwrap_or_default().to_string(),
                // Trakt has no pictures; the poster, if any, comes from TMDB later.
                poster: None,
                info: None,
            })
        })
        .collect()
}

/// A Radarr (`tv` false) or Sonarr lookup result.
pub fn card_from_lookup(l: &Lookup, tv: bool) -> Option<Card> {
    let (tmdb, tvdb) = (
        (l.tmdb_id > 0).then_some(l.tmdb_id),
        (l.tvdb_id > 0).then_some(l.tvdb_id),
    );
    if tmdb.is_none() && tvdb.is_none() {
        return None;
    }
    Some(Card {
        tv,
        tmdb,
        tvdb,
        imdb: l.imdb_id.clone().filter(|s| !s.is_empty()),
        title: l.title.clone(),
        year: (l.year > 0).then_some(l.year),
        rating: l.rating(),
        overview: l.overview.clone().unwrap_or_default(),
        poster: l.poster().map(String::from),
        info: None,
    })
}

// ---- what a card says about itself ----

/// Seerr's own state as a card badge. A request still waiting for approval is Pending, one Seerr
/// is already working on is Requested.
pub fn seerr_badge(i: &MediaInfo) -> Option<Badge> {
    let has = |s| i.requests.iter().any(|r| r.status == s);
    Some(match i.status {
        5 => Badge::Available,
        4 => Badge::PartlyAvailable,
        3 => Badge::Requested,
        2 => Badge::Pending,
        _ if has(1) => Badge::Pending,
        _ if has(2) => Badge::Requested,
        _ if has(4) => Badge::Failed,
        _ if has(3) => Badge::Declined,
        _ => return None,
    })
}

/// A native request as a card badge.
pub fn native_badge(s: wishlist::Status) -> Badge {
    match s {
        wishlist::Status::Requested => Badge::Requested,
        wishlist::Status::Queued => Badge::Queued,
        wishlist::Status::Downloading => Badge::Downloading,
        wishlist::Status::Available => Badge::Available,
    }
}

/// What a card wears. The catalog comes first (a title any source has is Available and opens its
/// page), then this session's or the native list's request, then Seerr's state. `None`: not requested.
pub fn standing(
    on_servers: bool,
    native: Option<wishlist::Status>,
    seerr: Option<&MediaInfo>,
) -> Option<Badge> {
    if on_servers {
        return Some(Badge::Available);
    }
    native
        .map(native_badge)
        .or_else(|| seerr.and_then(seerr_badge))
}

/// The line under a card.
pub fn card_label(b: Option<Badge>) -> &'static str {
    match b {
        None => "Not requested",
        Some(Badge::Available) => "Available on your servers",
        Some(b) => b.label(),
    }
}

/// May a Request button be shown for this standing? Not for what is on a server, already requested
/// or partly available (a show's missing seasons are asked for from its page).
pub fn can_ask(b: Option<Badge>) -> bool {
    matches!(b, None | Some(Badge::Declined) | Some(Badge::Failed))
}

// ---- matching a card against the catalog ----

/// Is this catalog item the card's title? By provider ids as `merge.rs` does (any shared TMDB,
/// IMDb or TVDB id); an item with no ids at all falls back to title and year.
pub fn matches(card: &Card, m: &Media) -> bool {
    let want = if card.tv {
        MediaKind::Series
    } else {
        MediaKind::Movie
    };
    if m.kind != want {
        return false;
    }
    let same = |a: Option<&str>, b: Option<&str>| match (a, b) {
        (Some(a), Some(b)) => Some(a.trim().eq_ignore_ascii_case(b.trim())),
        _ => None,
    };
    let tmdb = card.tmdb.map(|t| t.to_string());
    let tvdb = card.tvdb.map(|t| t.to_string());
    let verdicts = [
        same(tmdb.as_deref(), m.ids.tmdb.as_deref()),
        same(tvdb.as_deref(), m.ids.tvdb.as_deref()),
        same(card.imdb.as_deref(), m.ids.imdb.as_deref()),
    ];
    if verdicts.contains(&Some(true)) {
        return true;
    }
    // Ids exist on both sides and none agree: a different title. No ids on the item: title and year.
    if m.ids.is_empty() {
        let t = card.title.trim().to_lowercase();
        return !t.is_empty()
            && m.title.trim().to_lowercase() == t
            && (card.year.is_none() || m.year.is_none() || m.year == card.year);
    }
    false
}

pub fn find_in<'a>(card: &Card, catalog: &'a [Media]) -> Option<&'a Media> {
    catalog.iter().find(|m| matches(card, m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn have(s: bool, t: bool, k: bool, a: bool) -> Have {
        Have {
            seerr: s,
            tmdb: t,
            trakt: k,
            arr: a,
        }
    }

    #[test]
    fn the_source_order_is_seerr_tmdb_trakt_then_arr_for_search_only() {
        assert_eq!(pick(have(true, true, true, true), false), Some(Src::Seerr));
        assert_eq!(pick(have(false, true, true, true), false), Some(Src::Tmdb));
        assert_eq!(pick(have(false, false, true, true), true), Some(Src::Trakt));
        assert_eq!(pick(have(false, false, false, true), true), Some(Src::Arr));
        assert_eq!(
            pick(have(false, false, false, true), false),
            None,
            "arr cannot browse"
        );
        assert_eq!(pick(have(false, false, false, false), true), None);
        assert!(connect_text(have(false, false, false, false)).contains("TMDB"));
        assert!(connect_text(have(false, false, false, true)).contains("search"));
    }

    fn card(tv: bool, tmdb: u64, title: &str) -> Card {
        Card {
            tv,
            tmdb: Some(tmdb),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn cards_dedup_interleave_and_narrow() {
        let a = vec![
            card(false, 1, "A"),
            card(false, 2, "B"),
            card(false, 1, "A again"),
        ];
        let d = dedup(a);
        assert_eq!(d.len(), 2);
        let m = interleave(d, vec![card(true, 9, "S")]);
        assert_eq!(
            m.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["A", "S", "B"]
        );
        let f = Filter {
            scope: Scope::Shows,
            ..Default::default()
        };
        assert_eq!(narrow(m.clone(), &f).len(), 1);
        let f = Filter {
            year: Some(1999),
            ..Default::default()
        };
        assert!(narrow(m, &f).is_empty());
        assert_ne!(card(false, 1, "x").key(), card(true, 1, "x").key());
    }

    #[test]
    fn tmdb_results_become_cards() {
        let v = json!({"results":[
            {"id":603,"media_type":"movie","title":"The Matrix","release_date":"1999-03-30","vote_average":8.2,"poster_path":"/p.jpg","overview":"x"},
            {"id":1396,"media_type":"tv","name":"Show","first_air_date":"2008-01-20","vote_average":0},
            {"id":5,"media_type":"person","name":"Someone"},
            {"id":6,"media_type":"movie","title":"  "}]});
        let c = cards_from_tmdb(&v, None);
        assert_eq!(c.len(), 2);
        assert_eq!(
            (c[0].tv, c[0].tmdb, c[0].year, c[0].rating),
            (false, Some(603), Some(1999), Some(8.2))
        );
        assert_eq!(c[0].poster.as_deref(), Some("/p.jpg"));
        assert!(c[1].tv && c[1].rating.is_none());
        let only = json!({"results":[{"id":7,"title":"M","release_date":""}]});
        assert_eq!(cards_from_tmdb(&only, Some(Kind::Movie))[0].year, None);
        assert!(cards_from_tmdb(&only, None).is_empty(), "no kind, no card");
    }

    #[test]
    fn trakt_shapes_become_cards() {
        let trending = json!([{"watchers":9,"movie":{"title":"Alien","year":1979,"ids":{"trakt":1,"imdb":"tt0078748","tmdb":348}}},
            {"watchers":3,"show":{"title":"Show","year":2008,"rating":8.4,"ids":{"tvdb":81189,"tmdb":1396}}}]);
        let c = cards_from_trakt(&trending, None);
        assert_eq!(c.len(), 2);
        assert_eq!(
            (c[0].imdb.as_deref(), c[0].tmdb),
            (Some("tt0078748"), Some(348))
        );
        assert!(c[1].tv && c[1].tvdb == Some(81189) && c[1].rating == Some(8.4));
        let popular =
            json!([{"title":"P","year":2020,"ids":{"tmdb":5}},{"title":"No ids","ids":{}}]);
        assert_eq!(cards_from_trakt(&popular, Some(Kind::Movie)).len(), 1);
        assert!(cards_from_trakt(&popular, None).is_empty());
    }

    #[test]
    fn lookups_become_cards() {
        let l: Lookup = serde_json::from_str(
            r#"{"title":"Alien","year":1979,"tmdbId":348,"images":[{"coverType":"poster","remoteUrl":"https://e/p.jpg"}]}"#,
        )
        .unwrap();
        let c = card_from_lookup(&l, false).unwrap();
        assert_eq!(
            (c.tmdb, c.poster.as_deref()),
            (Some(348), Some("https://e/p.jpg"))
        );
        assert!(card_from_lookup(&Lookup::default(), true).is_none());
    }

    fn info(status: u32, req: &[u32]) -> MediaInfo {
        MediaInfo {
            status,
            requests: req
                .iter()
                .map(|s| nocturne_seerr::Request {
                    status: *s,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn standing_prefers_the_catalog_then_native_then_seerr() {
        use Badge::*;
        assert_eq!(seerr_badge(&info(2, &[])), Some(Pending));
        assert_eq!(seerr_badge(&info(3, &[])), Some(Requested));
        assert_eq!(seerr_badge(&info(1, &[1])), Some(Pending));
        assert_eq!(seerr_badge(&info(1, &[2])), Some(Requested));
        assert_eq!(seerr_badge(&info(1, &[])), None);
        assert_eq!(seerr_badge(&info(5, &[])), Some(Available));
        let i = info(2, &[]);
        assert_eq!(
            standing(true, Some(wishlist::Status::Queued), Some(&i)),
            Some(Available)
        );
        assert_eq!(
            standing(false, Some(wishlist::Status::Queued), Some(&i)),
            Some(Queued)
        );
        assert_eq!(standing(false, None, Some(&i)), Some(Pending));
        assert_eq!(standing(false, None, None), None);
        assert_eq!(card_label(None), "Not requested");
        assert_eq!(card_label(Some(Available)), "Available on your servers");
        assert_eq!(card_label(Some(Pending)), "Pending");
        assert!(can_ask(None) && can_ask(Some(Declined)));
        assert!(
            !can_ask(Some(Pending)) && !can_ask(Some(Available)) && !can_ask(Some(Downloading))
        );
    }

    fn item(
        kind: MediaKind,
        title: &str,
        year: Option<u32>,
        tmdb: Option<&str>,
        imdb: Option<&str>,
    ) -> Media {
        Media {
            kind,
            title: title.into(),
            year,
            ids: crate::source::ProviderIds {
                tmdb: tmdb.map(String::from),
                imdb: imdb.map(String::from),
                tvdb: None,
            },
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        }
    }

    #[test]
    fn catalog_match_is_by_ids_with_a_title_fallback_for_items_without_any() {
        let mut c = card(false, 348, "Alien");
        c.year = Some(1979);
        c.imdb = Some("tt0078748".into());
        assert!(matches(
            &c,
            &item(MediaKind::Movie, "Alien (1979)", None, Some("348"), None)
        ));
        assert!(
            matches(
                &c,
                &item(MediaKind::Movie, "x", None, None, Some("TT0078748"))
            ),
            "any shared id"
        );
        assert!(
            !matches(
                &c,
                &item(MediaKind::Movie, "Alien", Some(1979), Some("999"), None)
            ),
            "other id: other title"
        );
        assert!(
            matches(
                &c,
                &item(MediaKind::Movie, " alien ", Some(1979), None, None)
            ),
            "no ids: title and year"
        );
        assert!(!matches(
            &c,
            &item(MediaKind::Movie, "Alien", Some(1986), None, None)
        ));
        assert!(
            !matches(
                &c,
                &item(MediaKind::Series, "Alien", Some(1979), Some("348"), None)
            ),
            "kind must agree"
        );
        let cat = vec![
            item(MediaKind::Movie, "Other", None, Some("1"), None),
            item(MediaKind::Movie, "Alien", None, Some("348"), None),
        ];
        assert_eq!(find_in(&c, &cat).map(|m| m.title.as_str()), Some("Alien"));
    }

    #[test]
    fn filters_and_genres() {
        assert!(genres(Scope::Movies).iter().any(|g| g.name == "Horror"));
        assert!(!genres(Scope::Shows).iter().any(|g| g.name == "Horror"));
        assert!(genres(Scope::Shows).iter().any(|g| g.id == 10759));
        assert!(
            genres(Scope::All)
                .windows(2)
                .all(|w| w[0].name <= w[1].name)
        );
        assert_eq!(years(2026)[0], 2027);
        assert!(
            Filter {
                genre: Some(1),
                ..Default::default()
            }
            .narrowed()
        );
        assert!(!Filter::default().narrowed() && !Filter::default().searching());
        assert!(
            Filter {
                query: " a ".into(),
                ..Default::default()
            }
            .searching()
        );
    }
}
