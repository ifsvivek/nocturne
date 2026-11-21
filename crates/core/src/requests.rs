//! Seerr requests, minus the window: permission checks, status badges, the season picker, quota
//! and error wording, and which Seerr hits are not already in the catalog. Pure, so it is unit
//! tested offline; the wire is `nocturne-seerr`, the glue is the UI crate's `requests.rs`.
//! A request goes to the Seerr server only, never to a media server (docs/WRITES.md).
use crate::source::{Kind as MediaKind, Media};
use nocturne_seerr::{
    Detail, Error, Kind, MediaInfo, Quota, QuotaPart, Request, SearchHit, Seasons, perm,
};
use std::collections::BTreeSet;

/// Shown wherever the user decides to send a request.
pub const SENDS_TO_SEERR: &str =
    "This sends a request to your Seerr server. Nothing is changed on your media servers.";

/// Help text of the "Allow requests" setting.
pub const ALLOW_HINT: &str = "Off by default. When on, titles you find in Search can be requested from your Seerr server, and your own pending requests can be cancelled. A request is sent to your Seerr server, not to your media servers.";

/// May this user request a movie / a show? (`REQUEST` covers both; the type flags cover one.)
pub fn can_request(perms: u64, kind: Kind) -> bool {
    perm::has(perms, perm::REQUEST)
        || perm::has(
            perms,
            match kind {
                Kind::Movie => perm::REQUEST_MOVIE,
                Kind::Tv => perm::REQUEST_TV,
            },
        )
}

/// What a title's (or a request's) state is called on a card or a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    Requested,
    Approved,
    Declined,
    Failed,
    PartlyAvailable,
    Available,
}

impl Badge {
    pub fn label(self) -> &'static str {
        match self {
            Self::Requested => "Requested",
            Self::Approved => "Approved",
            Self::Declined => "Declined",
            Self::Failed => "Failed",
            Self::PartlyAvailable => "Partly available",
            Self::Available => "Available",
        }
    }
}

/// The badge of a title from what Seerr says about it (`mediaInfo`). Search results carry only the
/// media status, details carry the requests as well. `None`: nothing worth a badge.
pub fn badge_of_info(i: &MediaInfo) -> Option<Badge> {
    let has = |s| i.requests.iter().any(|r| r.status == s);
    Some(match i.status {
        5 => Badge::Available,
        4 => Badge::PartlyAvailable,
        _ if has(1) => Badge::Requested,
        _ if has(2) => Badge::Approved,
        2 => Badge::Requested,
        3 => Badge::Approved,
        _ if has(4) => Badge::Failed,
        _ if has(3) => Badge::Declined,
        _ => return None,
    })
}

/// A request's own status.
pub fn badge_of_request(status: u32) -> Option<Badge> {
    Some(match status {
        1 => Badge::Requested,
        2 => Badge::Approved,
        3 => Badge::Declined,
        4 => Badge::Failed,
        5 => Badge::Available,
        _ => return None,
    })
}

/// Can a request for this still be sent? A movie, once requested or available, cannot; a show can
/// while some season is free (the dialog decides which).
pub fn requestable(kind: Kind, info: Option<&MediaInfo>) -> bool {
    let Some(i) = info else { return true };
    match kind {
        Kind::Movie => {
            matches!(i.status, 0 | 1 | 7) && !i.requests.iter().any(|r| matches!(r.status, 1 | 2))
        }
        Kind::Tv => !matches!(i.status, 5 | 6),
    }
}

// ---- seasons ----

/// One season line of the request dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeasonRow {
    pub number: u32,
    pub episodes: u32,
    /// Already available or requested: shown ticked and disabled.
    pub taken: Option<Badge>,
}

/// The seasons of a show with what is already taken. Specials (season 0) only when the server
/// allows requesting them.
pub fn season_rows(d: &Detail, specials: bool) -> Vec<SeasonRow> {
    let info = d.media_info.as_ref();
    let taken = |n: u32| -> Option<Badge> {
        let known = info.and_then(|i| i.seasons.iter().find(|s| s.season_number == n));
        match known.map(|s| s.status) {
            Some(5) => return Some(Badge::Available),
            Some(4) => return Some(Badge::PartlyAvailable),
            Some(3) => return Some(Badge::Approved),
            Some(2) => return Some(Badge::Requested),
            _ => {}
        }
        // A pending or approved request names its seasons even before the media row does.
        info?
            .requests
            .iter()
            .filter(|r| matches!(r.status, 1 | 2))
            .find(|r| r.seasons.iter().any(|s| s.season_number == n))
            .and_then(|r| badge_of_request(r.status))
    };
    let mut rows: Vec<_> = d
        .seasons
        .iter()
        .filter(|s| specials || s.season_number > 0)
        .map(|s| SeasonRow {
            number: s.season_number,
            episodes: s.episode_count,
            taken: taken(s.season_number),
        })
        .collect();
    rows.sort_by_key(|r| r.number);
    rows
}

/// Which seasons the user has ticked. When the server does not allow partial requests the whole
/// show goes in one piece and ticking is off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub rows: Vec<SeasonRow>,
    pub picked: BTreeSet<u32>,
    pub partial: bool,
}

impl Picker {
    /// Everything that is still free starts ticked ("All seasons" preselected).
    pub fn new(rows: Vec<SeasonRow>, partial: bool) -> Self {
        let picked = rows
            .iter()
            .filter(|r| r.taken.is_none())
            .map(|r| r.number)
            .collect();
        Self {
            rows,
            picked,
            partial,
        }
    }

    fn free(&self) -> impl Iterator<Item = u32> + '_ {
        self.rows
            .iter()
            .filter(|r| r.taken.is_none())
            .map(|r| r.number)
    }

    pub fn toggle(&mut self, n: u32) {
        if !self.partial || !self.free().any(|f| f == n) {
            return;
        }
        if !self.picked.remove(&n) {
            self.picked.insert(n);
        }
    }

    pub fn all_picked(&self) -> bool {
        self.free().all(|n| self.picked.contains(&n))
    }

    /// "All seasons": tick every free season, or untick them all when they already are.
    pub fn toggle_all(&mut self) {
        if !self.partial {
            return;
        }
        if self.all_picked() {
            self.picked.clear();
        } else {
            self.picked = self.free().collect();
        }
    }

    /// Is anything left to ask for?
    pub fn any_free(&self) -> bool {
        self.free().next().is_some()
    }

    /// What to send; `None` when nothing is ticked (the Request button is then off).
    pub fn seasons(&self) -> Option<Seasons> {
        if !self.any_free() {
            return None;
        }
        if !self.partial {
            return Some(Seasons::All);
        }
        (!self.picked.is_empty()).then(|| Seasons::Some(self.picked.iter().copied().collect()))
    }
}

// ---- quota and errors ----

/// "2 requests left this week", or `None` when the account has no quota for this kind.
pub fn quota_text(q: &Quota, kind: Kind) -> Option<String> {
    let p: &QuotaPart = match kind {
        Kind::Movie => &q.movie,
        Kind::Tv => &q.tv,
    };
    if !p.restricted {
        return None;
    }
    let n = p.remaining.unwrap_or(0);
    let when = match p.days {
        Some(1) => "today".to_string(),
        Some(7) => "this week".to_string(),
        Some(30) => "this month".to_string(),
        Some(d) => format!("in {d} days"),
        None => "for now".to_string(),
    };
    Some(match n {
        0 => format!("You have no requests left {when}"),
        1 => format!("1 request left {when}"),
        n => format!("{n} requests left {when}"),
    })
}

/// A Seerr failure in plain words, for the dialog and notices.
pub fn error_text(e: &Error) -> String {
    match e {
        Error::Conflict => "Already requested.".into(),
        Error::QuotaExceeded(_) => "You have used up your request quota.".into(),
        Error::Forbidden(m) if m.contains("4K") => "Your Seerr account may not request 4K.".into(),
        Error::Forbidden(_) => "Your Seerr account may not request this.".into(),
        Error::Unauthorized => "Your Seerr sign-in has ended. Sign in again in Settings.".into(),
        Error::InvalidCredentials => "That username or password was not accepted.".into(),
        Error::ReadOnly => "Requests are off in developer mode.".into(),
        Error::NotFound => "Seerr does not know that title.".into(),
        Error::Http(_) => "Couldn't reach Seerr. Check the address and try again.".into(),
        Error::Status { status, .. } if *status >= 500 => {
            "Seerr could not do that just now (it may be fetching from TMDB). Try again.".into()
        }
        Error::Status { .. } | Error::Config(_) => "Seerr gave an unexpected answer.".into(),
    }
}

/// Plain `http` to a server that is not on this machine sends the sign-in unencrypted.
pub fn insecure_remote(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    !(host == "localhost" || host.starts_with("127.") || host == "[::1]")
}

// ---- the catalog ----

/// Is this Seerr title already in the user's catalog? By TMDB id first; when the catalog item has
/// no TMDB id, by title and year.
pub fn in_catalog(
    tmdb: u64,
    kind: Kind,
    title: &str,
    year: Option<u32>,
    catalog: &[&Media],
) -> bool {
    let want = match kind {
        Kind::Movie => MediaKind::Movie,
        Kind::Tv => MediaKind::Series,
    };
    let id = tmdb.to_string();
    let title = title.trim().to_lowercase();
    catalog
        .iter()
        .filter(|m| m.kind == want)
        .any(|m| match &m.ids.tmdb {
            Some(t) => *t == id,
            None => {
                !title.is_empty()
                    && m.title.trim().to_lowercase() == title
                    && (year.is_none() || m.year.is_none() || m.year == year)
            }
        })
}

/// The Seerr hits worth a "Request" row: movies and shows not already in the catalog.
pub fn not_in_catalog<'a>(hits: &'a [SearchHit], catalog: &[&Media]) -> Vec<&'a SearchHit> {
    hits.iter()
        .filter(|h| {
            h.kind()
                .is_some_and(|k| !in_catalog(h.id, k, &h.display_title(), h.year(), catalog))
        })
        .collect()
}

// ---- the Requests page ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    All,
    Pending,
    Approved,
    Available,
    Declined,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::All,
        Tab::Pending,
        Tab::Approved,
        Tab::Available,
        Tab::Declined,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Pending => "Pending",
            Self::Approved => "Approved",
            Self::Available => "Available",
            Self::Declined => "Declined",
        }
    }

    pub fn admits(self, r: &Request) -> bool {
        match self {
            Self::All => true,
            Self::Pending => r.status == 1,
            Self::Approved => r.status == 2,
            Self::Available => r.status == 5,
            Self::Declined => r.status == 3,
        }
    }
}

/// Own requests that can still be cancelled: pending approval.
pub fn can_cancel(r: &Request) -> bool {
    r.status == 1
}

/// "Seasons 1, 2" / "Season 3" / "" for a movie.
pub fn seasons_text(r: &Request) -> String {
    let mut n: Vec<u32> = r.seasons.iter().map(|s| s.season_number).collect();
    n.sort_unstable();
    match n.as_slice() {
        [] => String::new(),
        [one] => format!("Season {one}"),
        many => format!(
            "Seasons {}",
            many.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// `2026-11-21T10:00:00.000Z` as `2026-11-21`.
pub fn date_text(r: &Request) -> String {
    r.created_at
        .as_deref()
        .and_then(|d| d.get(..10))
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture<T: serde::de::DeserializeOwned>(s: &str) -> T {
        serde_json::from_str(s).unwrap()
    }

    fn info(status: u32, req: &[u32]) -> MediaInfo {
        MediaInfo {
            status,
            requests: req
                .iter()
                .map(|s| Request {
                    status: *s,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn permissions() {
        assert!(can_request(perm::REQUEST, Kind::Movie) && can_request(perm::REQUEST, Kind::Tv));
        assert!(
            can_request(perm::REQUEST_TV, Kind::Tv) && !can_request(perm::REQUEST_TV, Kind::Movie)
        );
        assert!(
            can_request(perm::ADMIN, Kind::Movie),
            "admin passes everything"
        );
        assert!(!can_request(0, Kind::Movie) && !can_request(perm::MANAGE_REQUESTS, Kind::Tv));
    }

    #[test]
    fn badges_follow_the_status_tables() {
        use Badge::*;
        let b = |i: MediaInfo| badge_of_info(&i);
        assert_eq!(b(info(1, &[])), None, "known but not requested");
        assert_eq!(b(info(7, &[])), None, "deleted");
        assert_eq!(
            b(info(2, &[])),
            Some(Requested),
            "search results carry only the media status"
        );
        assert_eq!(b(info(2, &[1])), Some(Requested));
        assert_eq!(b(info(3, &[2])), Some(Approved));
        assert_eq!(b(info(3, &[])), Some(Approved));
        assert_eq!(b(info(1, &[3])), Some(Declined));
        assert_eq!(b(info(1, &[4])), Some(Failed));
        assert_eq!(b(info(4, &[2])), Some(PartlyAvailable));
        assert_eq!(b(info(5, &[5])), Some(Available));
        assert_eq!(
            b(info(1, &[3, 1])),
            Some(Requested),
            "a live request wins over an old declined one"
        );
        assert_eq!(badge_of_request(1), Some(Requested));
        assert_eq!(badge_of_request(5), Some(Available));
        assert_eq!(badge_of_request(9), None);
    }

    #[test]
    fn who_can_still_be_requested() {
        assert!(requestable(Kind::Movie, None));
        assert!(requestable(Kind::Movie, Some(&info(1, &[]))));
        assert!(
            requestable(Kind::Movie, Some(&info(1, &[3]))),
            "declined: ask again"
        );
        assert!(!requestable(Kind::Movie, Some(&info(2, &[1]))));
        assert!(!requestable(Kind::Movie, Some(&info(5, &[]))));
        assert!(
            !requestable(Kind::Movie, Some(&info(6, &[]))),
            "blocklisted"
        );
        assert!(
            requestable(Kind::Tv, Some(&info(4, &[2]))),
            "partly available: other seasons"
        );
        assert!(requestable(Kind::Tv, Some(&info(3, &[2]))));
        assert!(!requestable(Kind::Tv, Some(&info(5, &[]))));
    }

    fn show() -> Detail {
        fixture(include_str!("../../seerr/tests/fixtures/tv_detail.json"))
    }

    #[test]
    fn seasons_show_what_is_taken_and_hide_specials() {
        let rows = season_rows(&show(), false);
        let taken: Vec<_> = rows.iter().map(|r| (r.number, r.taken)).collect();
        assert_eq!(
            taken,
            [
                (1, Some(Badge::Available)),
                (2, Some(Badge::Approved)),
                (3, None),
                (4, None)
            ]
        );
        assert_eq!(
            season_rows(&show(), true)[0].number,
            0,
            "specials only when the server allows"
        );
        assert_eq!(rows[2].episodes, 13);
    }

    #[test]
    fn a_pending_request_marks_its_seasons_before_the_media_row_does() {
        let mut d = show();
        let m = d.media_info.as_mut().unwrap();
        m.seasons.clear();
        m.requests[0].status = 1;
        let rows = season_rows(&d, false);
        assert_eq!(rows[1].taken, Some(Badge::Requested));
        assert_eq!(rows[0].taken, None);
    }

    #[test]
    fn the_picker_starts_with_everything_free_and_sends_explicit_seasons() {
        let mut p = Picker::new(season_rows(&show(), false), true);
        assert_eq!(p.picked.iter().copied().collect::<Vec<_>>(), [3, 4]);
        assert!(p.all_picked());
        assert_eq!(p.seasons(), Some(Seasons::Some(vec![3, 4])));
        p.toggle(4);
        assert!(!p.all_picked());
        assert_eq!(p.seasons(), Some(Seasons::Some(vec![3])));
        p.toggle(1); // taken: nothing happens
        p.toggle(99);
        assert_eq!(p.seasons(), Some(Seasons::Some(vec![3])));
        p.toggle(3);
        assert_eq!(p.seasons(), None, "nothing ticked: no request");
        p.toggle_all();
        assert_eq!(p.seasons(), Some(Seasons::Some(vec![3, 4])));
        p.toggle_all();
        assert!(p.picked.is_empty());
    }

    #[test]
    fn without_partial_requests_the_show_goes_whole() {
        let mut p = Picker::new(season_rows(&show(), false), false);
        p.toggle(3);
        p.toggle_all();
        assert_eq!(p.seasons(), Some(Seasons::All));
        let none_free = Picker::new(
            vec![SeasonRow {
                number: 1,
                episodes: 1,
                taken: Some(Badge::Available),
            }],
            true,
        );
        assert_eq!(none_free.seasons(), None);
        assert!(!none_free.any_free());
    }

    #[test]
    fn quota_wording() {
        let restricted: Quota = fixture(include_str!(
            "../../seerr/tests/fixtures/quota_restricted.json"
        ));
        let free: Quota = fixture(include_str!("../../seerr/tests/fixtures/quota_free.json"));
        assert_eq!(
            quota_text(&restricted, Kind::Movie).as_deref(),
            Some("You have no requests left this week")
        );
        assert_eq!(quota_text(&restricted, Kind::Tv), None);
        assert_eq!(
            quota_text(&free, Kind::Tv).as_deref(),
            Some("2 requests left this month")
        );
        let mut one = free.clone();
        one.tv.remaining = Some(1);
        one.tv.days = Some(7);
        assert_eq!(
            quota_text(&one, Kind::Tv).as_deref(),
            Some("1 request left this week")
        );
    }

    #[test]
    fn errors_are_plain_words() {
        assert_eq!(error_text(&Error::Conflict), "Already requested.");
        assert!(
            error_text(&Error::QuotaExceeded("Movie Quota exceeded.".into())).contains("quota")
        );
        assert!(
            error_text(&Error::Forbidden(
                "You do not have permission to make 4K movie requests.".into()
            ))
            .contains("4K")
        );
        assert!(
            error_text(&Error::Status {
                status: 500,
                path: "/x".into()
            })
            .contains("TMDB")
        );
        assert!(error_text(&Error::Unauthorized).contains("Sign in again"));
        for e in [
            Error::NotFound,
            Error::ReadOnly,
            Error::InvalidCredentials,
            Error::Config("x".into()),
        ] {
            assert!(
                !error_text(&e).contains("Error"),
                "no debug text for people"
            );
        }
    }

    #[test]
    fn plain_http_to_another_machine_is_flagged() {
        assert!(insecure_remote("http://192.168.1.5:5055"));
        assert!(insecure_remote("http://seerr.example.com"));
        assert!(!insecure_remote("https://seerr.example.com"));
        assert!(!insecure_remote("http://127.0.0.1:5057"));
        assert!(!insecure_remote("http://localhost:5055"));
    }

    fn media(kind: MediaKind, title: &str, year: Option<u32>, tmdb: Option<&str>) -> Media {
        let mut m = Media::from_item(&nocturne_jellyfin::Item::default());
        m.kind = kind;
        m.title = title.into();
        m.year = year;
        m.ids.tmdb = tmdb.map(str::to_string);
        m
    }

    #[test]
    fn the_catalog_is_matched_by_tmdb_id_first_then_title_and_year() {
        let have = [
            media(MediaKind::Movie, "Alien", Some(1979), Some("348")),
            media(MediaKind::Series, "Breaking Bad", Some(2008), None),
            media(MediaKind::Movie, "Wrong id", Some(2000), Some("1")),
        ];
        let c: Vec<&Media> = have.iter().collect();
        assert!(in_catalog(348, Kind::Movie, "Alien", Some(1979), &c));
        assert!(
            in_catalog(348, Kind::Movie, "Another name", None, &c),
            "id wins over the title"
        );
        assert!(
            in_catalog(1396, Kind::Tv, "breaking bad", Some(2008), &c),
            "no id: title and year"
        );
        assert!(!in_catalog(1396, Kind::Tv, "Breaking Bad", Some(2010), &c));
        assert!(
            !in_catalog(348, Kind::Tv, "Alien", Some(1979), &c),
            "a movie is not a show"
        );
        assert!(
            !in_catalog(2, Kind::Movie, "Wrong id", Some(2000), &c),
            "a different id is a different title"
        );
        let hits: nocturne_seerr::SearchPage =
            fixture(include_str!("../../seerr/tests/fixtures/search.json"));
        let left = not_in_catalog(&hits.results, &c);
        assert!(
            left.is_empty(),
            "Alien is owned by id, Breaking Bad by title and year, the person is skipped"
        );
        let only_alien = [&have[0]];
        let left = not_in_catalog(&hits.results, &only_alien);
        assert_eq!(left.iter().map(|h| h.id).collect::<Vec<_>>(), [1396]);
    }

    #[test]
    fn page_helpers() {
        let rs: nocturne_seerr::RequestsPage = fixture(include_str!(
            "../../seerr/tests/fixtures/requests_page.json"
        ));
        let r = &rs.results;
        assert_eq!(seasons_text(&r[0]), "Seasons 1, 2");
        assert_eq!(seasons_text(&r[1]), "");
        assert_eq!(date_text(&r[0]), "2026-11-21");
        assert!(can_cancel(&r[0]) && !can_cancel(&r[1]) && !can_cancel(&r[2]));
        let n = |t: Tab| r.iter().filter(|x| t.admits(x)).count();
        assert_eq!(
            (
                n(Tab::All),
                n(Tab::Pending),
                n(Tab::Approved),
                n(Tab::Available),
                n(Tab::Declined)
            ),
            (3, 1, 1, 1, 0)
        );
    }
}
