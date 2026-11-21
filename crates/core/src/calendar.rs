//! The Calendar's pure part: upcoming episodes from every signed-in server and from the user's Trakt calendar merged into one list of dated entries, marked by what the user can do with each, grouped by local day, with the month, week and agenda ranges. Read only: the reads are `Backend::upcoming` and `Trakt::calendar`. No I/O here, and the time zone is a parameter, so the tests run in any zone.
use crate::merge::{SeriesIds, id_keys, identity_keys};
use crate::source::{Media, ProviderIds, SourceId};
use crate::trakt::{Airing, Ids};
use nocturne_jellyfin as jf;

/// A civil day in the user's local time: days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(pub i64);

impl Day {
    /// Howard Hinnant's `days_from_civil`.
    pub fn from_ymd(y: i64, m: u32, d: u32) -> Day {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Day(era * 146_097 + doe - 719_468)
    }

    /// `(year, month 1-12, day 1-31)`.
    pub fn ymd(self) -> (i64, u32, u32) {
        let z = self.0 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = yoe + era * 400 + i64::from(m <= 2);
        (y, m, d)
    }

    /// 0 = Monday .. 6 = Sunday (1970-01-01 was a Thursday).
    pub fn weekday(self) -> u32 {
        (self.0 + 3).rem_euclid(7) as u32
    }

    pub fn plus(self, n: i64) -> Day {
        Day(self.0 + n)
    }

    /// `YYYY-MM-DD`, the form Trakt's calendar takes.
    pub fn iso(self) -> String {
        let (y, m, d) = self.ymd();
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// The first of this day's month.
    pub fn month_start(self) -> Day {
        let (y, m, _) = self.ymd();
        Day::from_ymd(y, m, 1)
    }
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    let next = if m == 12 {
        Day::from_ymd(y + 1, 1, 1)
    } else {
        Day::from_ymd(y, m + 1, 1)
    };
    (next.0 - Day::from_ymd(y, m, 1).0) as u32
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub fn month_name(m: u32) -> &'static str {
    MONTHS[(m as usize + 11) % 12]
}

pub fn weekday_name(wd: u32) -> &'static str {
    WEEKDAYS[wd as usize % 7]
}

/// Seconds east of UTC of the local zone at the UTC instant `utc` (unix seconds). Other platforms than unix: UTC until a platform implementation exists.
pub fn local_offset(utc: i64) -> i64 {
    #[cfg(unix)]
    // SAFETY: plain libc time call with a zeroed out-struct.
    unsafe {
        let t = utc as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
    #[cfg(not(unix))]
    {
        let _ = utc;
        0
    }
}

/// The local day now.
pub fn today() -> Day {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    Day((now + local_offset(now)).div_euclid(86_400))
}

/// Unix seconds of an ISO 8601 string (`2026-12-01`, `2026-12-01T20:00:00Z`, `...00.0000000Z`, `...+02:00`) and whether it carries a time of day.
pub fn parse_utc(s: &str) -> Option<(i64, bool)> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes().get(4) != Some(&b'-') || s.as_bytes().get(7) != Some(&b'-') {
        return None;
    }
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let day = Day::from_ymd(y, m as u32, d as u32).0 * 86_400;
    if s.len() == 10 {
        return Some((day, false));
    }
    if !matches!(s.as_bytes().get(10), Some(b'T' | b' ')) {
        return None;
    }
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19).unwrap_or(0));
    if hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // After the seconds: an optional fraction, then `Z` or `+hh:mm` / `-hh:mm`.
    let rest = s.get(19..).unwrap_or("");
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match rest.as_bytes().first() {
        None | Some(b'Z') => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let h: i64 = rest.get(1..3)?.parse().ok()?;
            let mi: i64 = rest.get(4..6).and_then(|v| v.parse().ok()).unwrap_or(0);
            let v = h * 3600 + mi * 60;
            if sign == b'+' { v } else { -v }
        }
        _ => return None,
    };
    Some((day + hh * 3600 + mm * 60 + ss - off, true))
}

/// The local day (and minute of the day) of an air time. A date alone, or exactly midnight UTC (what a server stores when it knows only the date), stays on that date with no time.
pub fn place(iso: &str, off: &dyn Fn(i64) -> i64) -> Option<(Day, Option<u32>)> {
    let (secs, timed) = parse_utc(iso)?;
    if !timed || secs.rem_euclid(86_400) == 0 {
        return Some((Day(secs.div_euclid(86_400)), None));
    }
    let local = secs + off(secs);
    Some((
        Day(local.div_euclid(86_400)),
        Some((local.rem_euclid(86_400) / 60) as u32),
    ))
}

/// What the user can do with an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// A server has the file: play it now.
    OnServer,
    /// A server has the show but not this episode yet.
    Soon,
    /// Only Trakt lists it.
    NotOnServers,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::OnServer => "On your server",
            Status::Soon => "Airing soon",
            Status::NotOnServers => "Not on your servers",
        }
    }

    /// The word a stylesheet selects on (`.calendar-entry[data-status=...]`).
    pub fn key(self) -> &'static str {
        match self {
            Status::OnServer => "on-server",
            Status::Soon => "soon",
            Status::NotOnServers => "elsewhere",
        }
    }
}

/// One dated episode.
#[derive(Clone, Debug)]
pub struct Entry {
    pub day: Day,
    /// Local minute of the day; `None` when only the date is known.
    pub minute: Option<u32>,
    pub show: String,
    pub season: u32,
    pub number: u32,
    pub title: String,
    pub status: Status,
    /// The server's episode (playable when `status` is `OnServer`).
    pub episode: Option<Media>,
    /// The server's series, to open the show (also carries the poster).
    pub series: Option<Media>,
}

impl Entry {
    /// `S1·E5`.
    pub fn code(&self) -> String {
        format!("S{}\u{b7}E{}", self.season, self.number)
    }

    pub fn time_text(&self) -> Option<String> {
        self.minute.map(crate::video::clock_text)
    }
}

/// An episode a server lists as upcoming.
#[derive(Clone, Debug)]
pub struct Upcoming {
    pub media: Media,
    /// The show, with its poster tag.
    pub series: Option<Media>,
    /// A file exists for it (not a placeholder for a missing episode).
    pub on_disk: bool,
}

impl Upcoming {
    pub fn from_item(origin: SourceId, i: &jf::Item) -> Self {
        let series = i.series_id.as_ref().map(|id| {
            let mut tags = std::collections::HashMap::new();
            if let Some(t) = &i.x.series_primary_image_tag {
                tags.insert("Primary".to_string(), t.clone());
            }
            Media::from_item_in(
                origin,
                &jf::Item {
                    id: id.clone(),
                    name: i.series_name.clone().unwrap_or_default(),
                    kind: "Series".into(),
                    image_tags: tags,
                    ..Default::default()
                },
            )
        });
        Self {
            media: Media::from_item_in(origin, i),
            series,
            on_disk: i.x.location_type.as_deref() != Some("Virtual"),
        }
    }
}

fn provider(i: &Ids) -> ProviderIds {
    ProviderIds {
        imdb: i.imdb.clone(),
        tmdb: i.tmdb.map(|v| v.to_string()),
        tvdb: i.tvdb.map(|v| v.to_string()),
    }
}

/// Merge every source's upcoming episodes (best source first) and the Trakt calendar into one list, oldest first. The same episode on several servers is one entry (shared series id plus season and episode number), the copy with a file wins; a Trakt episode a server already lists adds nothing but its air time. A Trakt episode of a show a server has is "airing soon", any other is "not on your servers".
pub fn build(
    servers: &[(SourceId, Vec<Upcoming>)],
    series: &SeriesIds,
    trakt: &[Airing],
    off: &dyn Fn(i64) -> i64,
) -> Vec<Entry> {
    let mut out: Vec<(Entry, Vec<String>)> = vec![];
    for u in servers.iter().flat_map(|(_, v)| v) {
        let m = &u.media;
        let Some((day, minute)) = m.premiere.as_deref().and_then(|p| place(p, off)) else {
            continue;
        };
        let e = Entry {
            day,
            minute,
            show: m.series_name.clone().unwrap_or_else(|| m.title.clone()),
            season: m.season_number.unwrap_or(0),
            number: m.number.unwrap_or(0),
            title: m.title.clone(),
            status: if u.on_disk {
                Status::OnServer
            } else {
                Status::Soon
            },
            episode: Some(m.clone()),
            series: u.series.clone(),
        };
        let keys = identity_keys(m, series);
        match out
            .iter_mut()
            .find(|(_, k)| keys.iter().any(|x| k.contains(x)))
        {
            Some((have, k)) => {
                if have.status != Status::OnServer && e.status == Status::OnServer {
                    *have = e;
                }
                k.extend(keys);
            }
            None => out.push((e, keys)),
        }
    }
    for a in trakt {
        let Some((day, minute)) = place(&a.first_aired, off) else {
            continue;
        };
        let ids = provider(&a.ids);
        let keys: Vec<String> = id_keys("series", &ids)
            .into_iter()
            .map(|k| format!("ep:{k}:{}:{}", a.season, a.number))
            .collect();
        if let Some((have, _)) = out
            .iter_mut()
            .find(|(_, k)| keys.iter().any(|x| k.contains(x)))
        {
            if have.minute.is_none() && have.day == day {
                have.minute = minute;
            }
            continue;
        }
        let found = series.find(&ids);
        out.push((
            Entry {
                day,
                minute,
                show: a.show.clone(),
                season: a.season,
                number: a.number,
                title: a.title.clone(),
                status: if found.is_some() {
                    Status::Soon
                } else {
                    Status::NotOnServers
                },
                episode: None,
                series: found.map(|(origin, id)| {
                    Media::from_item_in(
                        origin,
                        &jf::Item {
                            id,
                            name: a.show.clone(),
                            kind: "Series".into(),
                            ..Default::default()
                        },
                    )
                }),
            },
            keys,
        ));
    }
    let mut v: Vec<Entry> = out.into_iter().map(|(e, _)| e).collect();
    v.sort_by(|a, b| {
        (a.day, a.minute, &a.show, a.season, a.number)
            .cmp(&(b.day, b.minute, &b.show, b.season, b.number))
    });
    v
}

/// How the calendar is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Month,
    Week,
    Agenda,
}

impl View {
    pub const ALL: [View; 3] = [View::Month, View::Week, View::Agenda];

    pub fn label(self) -> &'static str {
        match self {
            View::Month => "Month",
            View::Week => "Week",
            View::Agenda => "Agenda",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            View::Month => "month",
            View::Week => "week",
            View::Agenda => "agenda",
        }
    }
}

/// Days the agenda covers from its anchor.
pub const AGENDA_DAYS: i64 = 30;

/// The Monday on or before `d`. Weeks start on Monday (ISO 8601); the locale's first weekday is not read yet.
pub fn week_start(d: Day) -> Day {
    d.plus(-(d.weekday() as i64))
}

/// The days shown for `view` around `anchor`: `[start, end)`. A month is whole weeks (4 to 6 of them), so the grid's first and last cells may belong to the neighbouring months.
pub fn range(view: View, anchor: Day) -> (Day, Day) {
    match view {
        View::Month => {
            let first = anchor.month_start();
            let (y, m, _) = first.ymd();
            let last = first.plus(days_in_month(y, m) as i64 - 1);
            (week_start(first), week_start(last).plus(7))
        }
        View::Week => (week_start(anchor), week_start(anchor).plus(7)),
        View::Agenda => (anchor, anchor.plus(AGENDA_DAYS)),
    }
}

/// The anchor one step before (`-1`) or after (`1`) `anchor`: a month, a week, or the agenda's span.
pub fn step(view: View, anchor: Day, dir: i64) -> Day {
    match view {
        View::Month => {
            let (y, m, _) = anchor.ymd();
            let n = y * 12 + (m as i64 - 1) + dir;
            Day::from_ymd(n.div_euclid(12), n.rem_euclid(12) as u32 + 1, 1)
        }
        View::Week => anchor.plus(7 * dir),
        View::Agenda => anchor.plus(AGENDA_DAYS * dir),
    }
}

/// "November 2026", "Nov 16 - 22, 2026", "Nov 28 - Dec 4, 2026", "Dec 28, 2026 - Jan 3, 2027".
pub fn title(view: View, anchor: Day) -> String {
    if view == View::Month {
        let (y, m, _) = anchor.ymd();
        return format!("{} {y}", month_name(m));
    }
    let (s, e) = range(view, anchor);
    let last = e.plus(-1);
    let ((y1, m1, d1), (y2, m2, d2)) = (s.ymd(), last.ymd());
    let short = |m: u32| &month_name(m)[..3];
    if y1 != y2 {
        format!("{} {d1}, {y1} - {} {d2}, {y2}", short(m1), short(m2))
    } else if m1 != m2 {
        format!("{} {d1} - {} {d2}, {y2}", short(m1), short(m2))
    } else {
        format!("{} {d1} - {d2}, {y2}", short(m1))
    }
}

/// "Today", "Tomorrow", "Yesterday", else "Friday, Nov 27".
pub fn day_heading(d: Day, today: Day) -> String {
    match d.0 - today.0 {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        _ => {
            let (_, m, n) = d.ymd();
            format!("{}, {} {n}", weekday_name(d.weekday()), &month_name(m)[..3])
        }
    }
}

/// The entries of `[start, end)`.
pub fn within(entries: &[Entry], start: Day, end: Day) -> Vec<&Entry> {
    entries
        .iter()
        .filter(|e| e.day >= start && e.day < end)
        .collect()
}

/// The entries of `[start, end)` grouped by day, days with entries only, in order.
pub fn by_day(entries: &[Entry], start: Day, end: Day) -> Vec<(Day, Vec<&Entry>)> {
    let mut out: Vec<(Day, Vec<&Entry>)> = vec![];
    for e in within(entries, start, end) {
        match out.last_mut() {
            Some((d, v)) if *d == e.day => v.push(e),
            _ => out.push((e.day, vec![e])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Kind;

    const UTC: &dyn Fn(i64) -> i64 = &|_| 0;
    /// UTC-5 all year: a zone behind UTC, so a late evening UTC air time is the same day.
    const WEST: &dyn Fn(i64) -> i64 = &|_| -5 * 3600;
    /// UTC+9: an early UTC time is already the next local day... or a late one is.
    const EAST: &dyn Fn(i64) -> i64 = &|_| 9 * 3600;

    fn ep(origin: u32, id: &str, series: &str, s: u32, n: u32, premiere: &str) -> Media {
        let mut m = Media::from_item_in(
            SourceId(origin),
            &jf::Item {
                id: id.into(),
                name: format!("Episode {n}"),
                kind: "Episode".into(),
                index_number: Some(n),
                parent_index_number: Some(s),
                series_id: Some(series.into()),
                series_name: Some("Show".into()),
                ..Default::default()
            },
        );
        m.premiere = Some(premiere.into());
        m
    }

    fn up(m: Media, on_disk: bool) -> Upcoming {
        Upcoming {
            series: None,
            media: m,
            on_disk,
        }
    }

    fn ids(tvdb: &str) -> ProviderIds {
        ProviderIds {
            tvdb: Some(tvdb.into()),
            ..Default::default()
        }
    }

    fn airing(tvdb: u64, s: u32, n: u32, at: &str) -> Airing {
        Airing {
            show: "Show".into(),
            ids: Ids {
                tvdb: Some(tvdb),
                ..Default::default()
            },
            season: s,
            number: n,
            title: "T".into(),
            first_aired: at.into(),
        }
    }

    #[test]
    fn civil_dates_round_trip_and_weekdays() {
        assert_eq!(Day::from_ymd(1970, 1, 1), Day(0));
        assert_eq!(Day(0).weekday(), 3, "Thursday");
        let d = Day::from_ymd(2026, 11, 21);
        assert_eq!(d.ymd(), (2026, 11, 21));
        assert_eq!(d.weekday(), 5, "Saturday");
        assert_eq!(d.iso(), "2026-11-21");
        for n in -800..800 {
            let d = Day(n * 37);
            let (y, m, dd) = d.ymd();
            assert_eq!(Day::from_ymd(y, m, dd), d);
        }
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn parses_iso_forms() {
        let d = Day::from_ymd(2026, 12, 1).0 * 86_400;
        assert_eq!(parse_utc("2026-12-01"), Some((d, false)));
        assert_eq!(
            parse_utc("2026-12-01T20:30:00.0000000Z"),
            Some((d + 20 * 3600 + 1800, true))
        );
        assert_eq!(
            parse_utc("2026-12-01T20:30:00+02:00"),
            Some((d + 18 * 3600 + 1800, true))
        );
        assert_eq!(parse_utc("soon"), None);
        assert_eq!(parse_utc("2026-13-01"), None);
        assert_eq!(parse_utc(""), None);
    }

    #[test]
    fn air_time_moves_with_the_zone_and_a_bare_date_does_not() {
        let at = "2026-12-01T02:30:00Z";
        assert_eq!(
            place(at, UTC),
            Some((Day::from_ymd(2026, 12, 1), Some(150)))
        );
        // 02:30 UTC is the evening before at UTC-5.
        assert_eq!(
            place(at, WEST),
            Some((Day::from_ymd(2026, 11, 30), Some(21 * 60 + 30)))
        );
        assert_eq!(
            place("2026-12-01T20:00:00Z", EAST),
            Some((Day::from_ymd(2026, 12, 2), Some(5 * 60)))
        );
        // Date only, or midnight UTC: the date as written in every zone, no time.
        for z in [UTC, WEST, EAST] {
            assert_eq!(
                place("2026-12-01T00:00:00.0000000Z", z),
                Some((Day::from_ymd(2026, 12, 1), None))
            );
            assert_eq!(
                place("2026-12-01", z),
                Some((Day::from_ymd(2026, 12, 1), None))
            );
        }
    }

    #[test]
    fn servers_merge_and_the_copy_with_a_file_wins() {
        let mut sid = SeriesIds::default();
        sid.set(SourceId(1), "a".into(), ids("100"));
        sid.set(SourceId(2), "b".into(), ids("100"));
        let e1 = ep(1, "e1", "a", 1, 5, "2026-12-01");
        let e2 = ep(2, "e2", "b", 1, 5, "2026-12-01");
        let other = ep(2, "e3", "b", 1, 6, "2026-12-08");
        let v = build(
            &[
                (SourceId(1), vec![up(e1, false)]),
                (SourceId(2), vec![up(e2, true), up(other, false)]),
            ],
            &sid,
            &[],
            UTC,
        );
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].status, Status::OnServer);
        assert_eq!(v[0].episode.as_ref().unwrap().id, "e2");
        assert_eq!(v[1].status, Status::Soon);
        assert_eq!((v[1].season, v[1].number), (1, 6));
    }

    #[test]
    fn items_without_ids_are_never_merged() {
        let a = ep(1, "e1", "a", 1, 5, "2026-12-01");
        let b = ep(2, "e2", "b", 1, 5, "2026-12-01");
        let v = build(
            &[
                (SourceId(1), vec![up(a, true)]),
                (SourceId(2), vec![up(b, true)]),
            ],
            &SeriesIds::default(),
            &[],
            UTC,
        );
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn trakt_entries_are_marked_by_what_a_server_has() {
        let mut sid = SeriesIds::default();
        sid.set(SourceId(1), "a".into(), ids("100"));
        let on = ep(1, "e1", "a", 1, 5, "2026-12-01");
        let trakt = [
            // Same episode as the server's: merged, and the real air time is adopted.
            airing(100, 1, 5, "2026-12-01T21:00:00Z"),
            // A show a server has, another episode: airing soon, opens the show.
            airing(100, 1, 6, "2026-12-08T21:00:00Z"),
            // A show no server has.
            airing(200, 3, 1, "2026-12-02T01:00:00Z"),
        ];
        let v = build(&[(SourceId(1), vec![up(on, true)])], &sid, &trakt, UTC);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].status, Status::OnServer);
        assert_eq!(v[0].minute, Some(21 * 60), "Trakt's time fills the date");
        assert_eq!(v[1].status, Status::NotOnServers);
        assert!(v[1].series.is_none() && v[1].episode.is_none());
        assert_eq!(v[2].status, Status::Soon);
        let s = v[2].series.as_ref().unwrap();
        assert_eq!(
            (s.origin, s.id.as_str(), s.kind),
            (SourceId(1), "a", Kind::Series)
        );
    }

    #[test]
    fn entries_sort_by_day_then_time_with_dates_first() {
        let t = [
            airing(1, 1, 1, "2026-12-01T20:00:00Z"),
            airing(2, 1, 1, "2026-12-01"),
            airing(3, 1, 1, "2026-11-30T23:00:00Z"),
        ];
        let v = build(&[], &SeriesIds::default(), &t, UTC);
        let order: Vec<_> = v.iter().map(|e| (e.day.iso(), e.minute)).collect();
        assert_eq!(
            order,
            [
                ("2026-11-30".into(), Some(23 * 60)),
                ("2026-12-01".into(), None),
                ("2026-12-01".into(), Some(20 * 60)),
            ]
        );
    }

    #[test]
    fn ranges_steps_and_titles() {
        // November 2026 starts on a Sunday and ends on a Monday: six rows.
        let nov = Day::from_ymd(2026, 11, 21);
        let (s, e) = range(View::Month, nov);
        assert_eq!(s, Day::from_ymd(2026, 10, 26));
        assert_eq!(e, Day::from_ymd(2026, 12, 7));
        assert_eq!((e.0 - s.0) % 7, 0);
        // February 2027 starts on a Monday and has 28 days: four rows.
        let (s, e) = range(View::Month, Day::from_ymd(2027, 2, 14));
        assert_eq!((e.0 - s.0) / 7, 4);
        assert_eq!(s, Day::from_ymd(2027, 2, 1));
        assert_eq!(
            range(View::Week, nov),
            (Day::from_ymd(2026, 11, 16), Day::from_ymd(2026, 11, 23))
        );
        assert_eq!(range(View::Agenda, nov), (nov, nov.plus(30)));
        assert_eq!(step(View::Month, nov, 1), Day::from_ymd(2026, 12, 1));
        assert_eq!(
            step(View::Month, Day::from_ymd(2026, 1, 20), -1),
            Day::from_ymd(2025, 12, 1)
        );
        assert_eq!(step(View::Week, nov, -1), nov.plus(-7));
        assert_eq!(title(View::Month, nov), "November 2026");
        assert_eq!(title(View::Week, nov), "Nov 16 - 22, 2026");
        assert_eq!(
            title(View::Week, Day::from_ymd(2026, 12, 30)),
            "Dec 28, 2026 - Jan 3, 2027"
        );
        assert_eq!(day_heading(nov, nov), "Today");
        assert_eq!(day_heading(nov.plus(1), nov), "Tomorrow");
        assert_eq!(day_heading(nov.plus(6), nov), "Friday, Nov 27");
    }

    #[test]
    fn grouping_keeps_days_with_entries_inside_the_range() {
        let t = [
            airing(1, 1, 1, "2026-12-01"),
            airing(2, 1, 1, "2026-12-01"),
            airing(3, 1, 1, "2026-12-03"),
            airing(4, 1, 1, "2026-12-20"),
        ];
        let v = build(&[], &SeriesIds::default(), &t, UTC);
        let g = by_day(&v, Day::from_ymd(2026, 12, 1), Day::from_ymd(2026, 12, 8));
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].1.len(), 2);
        assert_eq!(g[1].0, Day::from_ymd(2026, 12, 3));
    }
}
