//! The decision rules (docs/dev/ARR_PLAN.md section 5), pure: which units a release would fill, why it is rejected, how acceptable releases rank, which to grab, and what a download's state means. No I/O.
use nocturne_arr_types::{
    MediaKind, ParsedRelease, Protocol, Quality, Release, SearchQuery, TorrentState, TorrentStatus,
};
use nocturne_core::server::{
    BlockEntry, Candidate, FileRecord, Grab, GrabState, Profile, Title, Unit,
};
use std::cmp::Reverse;

/// A download with no new bytes for this long (and not paused) has failed.
pub const STALL_SECS: u64 = 6 * 3600;

/// Lowercase letters and digits only, `&` as "and": what two spellings of a name share.
pub fn norm(s: &str) -> String {
    s.replace('&', "and")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Released by `today` (`YYYY-MM-DD`). Unknown dates count as released for movies, albums and books (catalogues often lack them) but not for episodes (usually a future one).
pub fn released(u: &Unit, today: &str) -> bool {
    match &u.air_date {
        Some(d) => d.as_str() <= today,
        None => u.episode.is_none(),
    }
}

/// Position in the profile's allowed list (better is higher); `None` = not allowed.
pub fn rank(p: &Profile, q: Quality) -> Option<usize> {
    p.allowed.iter().position(|a| *a == q)
}

fn below_cutoff(p: &Profile, f: &FileRecord) -> bool {
    match (rank(p, f.quality), rank(p, p.cutoff)) {
        (Some(have), Some(cut)) => have < cut,
        // A file of a quality the profile no longer allows is worth replacing.
        (None, _) => true,
        (_, None) => false,
    }
}

/// The unit wants a file: monitored and released, and either without one or (upgrades on) below the cutoff.
pub fn wants(u: &Unit, p: &Profile, today: &str) -> bool {
    u.monitored
        && released(u, today)
        && match &u.file {
            None => true,
            Some(f) => p.upgrades && below_cutoff(p, f),
        }
}

/// Whether `new` would replace the unit's current file: anything beats nothing; a better allowed quality while below the cutoff; or a PROPER / REPACK of the same quality.
pub fn is_upgrade(p: &Profile, current: Option<&FileRecord>, new: &ParsedRelease) -> bool {
    let Some(f) = current else { return true };
    let Some(n) = rank(p, new.quality) else {
        return false;
    };
    let better = rank(p, f.quality).is_none_or(|o| n > o);
    (better && p.upgrades && below_cutoff(p, f))
        || (new.quality == f.quality && new.revision() > f.revision)
}

/// The units of `title` a release is for, or `None` when it is about something else.
pub fn units_for(rel: &Release, title: &Title, units: &[Unit]) -> Option<Vec<u64>> {
    let p = &rel.parsed;
    let id_match = title.ids.imdb.is_some() && title.ids.imdb == rel.ids.imdb
        || title.ids.tvdb.is_some() && title.ids.tvdb == rel.ids.tvdb
        || title.ids.tmdb.is_some() && title.ids.tmdb == rel.ids.tmdb;
    if !id_match && norm(&p.title) != norm(&title.name) {
        return None;
    }
    let ids: Vec<u64> = match title.kind {
        MediaKind::Movie => {
            if let (Some(a), Some(b)) = (p.year, title.year)
                && a.abs_diff(b) > 1
            {
                return None;
            }
            units.iter().map(|u| u.id).collect()
        }
        MediaKind::Series => {
            let season = p.season?;
            units
                .iter()
                .filter(|u| u.season == Some(season))
                .filter(|u| {
                    if p.full_season {
                        u.monitored
                    } else {
                        u.episode.is_some_and(|e| p.episodes.contains(&e))
                    }
                })
                .map(|u| u.id)
                .collect()
        }
        MediaKind::Music | MediaKind::Book => {
            let work = norm(p.work.as_deref()?);
            units
                .iter()
                .filter(|u| norm(&u.name) == work)
                .map(|u| u.id)
                .collect()
        }
    };
    (!ids.is_empty()).then_some(ids)
}

/// Why a release must not be grabbed for `units` (empty = acceptable). `blocked`: on the blocklist; `queued`: one of the units is already downloading.
pub fn rejections(
    rel: &Release,
    units: &[&Unit],
    p: &Profile,
    today: &str,
    blocked: bool,
    queued: bool,
) -> Vec<String> {
    let mut r = Vec::new();
    let name = rel.title.to_lowercase();
    if rel.protocol == Protocol::Usenet {
        r.push(
            "Usenet releases need a Usenet download client, which NOCTURNE does not have yet"
                .into(),
        );
    }
    if blocked {
        r.push("on the blocklist".into());
    }
    if queued {
        r.push("already downloading".into());
    }
    if rank(p, rel.parsed.quality).is_none() {
        r.push(format!(
            "{} is not allowed by the profile",
            rel.parsed.quality.label()
        ));
    }
    for u in units {
        if !u.monitored {
            r.push(format!("{} is not monitored", u.name));
        } else if !released(u, today) {
            r.push(format!("{} is not out yet", u.name));
        } else if !wants(u, p, today) || !is_upgrade(p, u.file.as_ref(), &rel.parsed) {
            r.push(format!(
                "{} already has a file that this would not improve",
                u.name
            ));
        }
    }
    for t in &p.required {
        if !name.contains(&t.to_lowercase()) {
            r.push(format!("does not contain \"{t}\""));
        }
    }
    for t in &p.ignored {
        if name.contains(&t.to_lowercase()) {
            r.push(format!("contains \"{t}\""));
        }
    }
    let per_unit_mb = rel.size / units.len().max(1) as u64 / (1024 * 1024);
    if p.min_mb > 0 && per_unit_mb < p.min_mb {
        r.push(format!("too small ({per_unit_mb} MiB per item)"));
    }
    if p.max_mb > 0 && per_unit_mb > p.max_mb {
        r.push(format!("too large ({per_unit_mb} MiB per item)"));
    }
    if rel.protocol == Protocol::Torrent
        && let Some(s) = rel.seeders
        && s < p.min_seeders
    {
        r.push(format!("{s} seeders, the profile wants {}", p.min_seeders));
    }
    if !p.languages.is_empty() {
        let langs: &[String] = &rel.parsed.languages;
        let en = ["en".to_string()];
        let langs = if langs.is_empty() { &en[..] } else { langs };
        if !langs.iter().any(|l| p.languages.contains(l)) {
            r.push(format!("language {} not wanted", langs.join(", ")));
        }
    }
    r
}

/// Sum of the preferred-term scores the release name contains.
pub fn preferred(rel: &Release, p: &Profile) -> i32 {
    let name = rel.title.to_lowercase();
    p.preferred
        .iter()
        .filter(|w| name.contains(&w.term.to_lowercase()))
        .map(|w| w.score)
        .sum()
}

/// Ordering key, bigger is better: quality, then PROPER / REPACK, preferred terms, seeders.
pub fn score(rel: &Release, p: &Profile) -> (usize, u8, i32, u32) {
    (
        rank(p, rel.parsed.quality).unwrap_or(0),
        rel.parsed.revision(),
        preferred(rel, p),
        rel.seeders.unwrap_or(0),
    )
}

/// From acceptable `(release, units)` pairs, the set to grab: best first (ties: the one filling more units, so a season pack beats single episodes of the same rank), each unit at most once.
pub fn choose(mut cands: Vec<(Release, Vec<u64>)>, p: &Profile) -> Vec<(Release, Vec<u64>)> {
    cands.sort_by_key(|(r, u)| Reverse((score(r, p), u.len())));
    let mut taken = std::collections::HashSet::new();
    cands
        .into_iter()
        .filter(|(_, units)| {
            let free = units.iter().all(|u| !taken.contains(u));
            if free {
                taken.extend(units.iter().copied());
            }
            free
        })
        .collect()
}

/// The searches for `targets` (units of `t`): one per movie, album or book; per season when two or more episodes of it are wanted (finds packs and single episodes alike), else per episode.
pub fn queries(t: &Title, targets: &[&Unit]) -> Vec<SearchQuery> {
    let base = SearchQuery {
        kind: t.kind,
        text: t.name.clone(),
        ids: t.ids.clone(),
        year: None,
        season: None,
        episode: None,
        artist: None,
        album: None,
        author: None,
        book: None,
    };
    match t.kind {
        MediaKind::Movie => vec![SearchQuery {
            text: match t.year {
                Some(y) => format!("{} {y}", t.name),
                None => t.name.clone(),
            },
            year: t.year,
            ..base
        }],
        MediaKind::Series => {
            let mut seasons: Vec<u32> = targets.iter().filter_map(|u| u.season).collect();
            seasons.sort_unstable();
            seasons.dedup();
            let mut out = Vec::new();
            for s in seasons {
                let eps: Vec<u32> = targets
                    .iter()
                    .filter(|u| u.season == Some(s))
                    .filter_map(|u| u.episode)
                    .collect();
                if eps.len() >= 2 {
                    out.push(SearchQuery {
                        season: Some(s),
                        ..base.clone()
                    });
                } else {
                    out.extend(eps.into_iter().map(|e| SearchQuery {
                        season: Some(s),
                        episode: Some(e),
                        ..base.clone()
                    }));
                }
            }
            out
        }
        MediaKind::Music | MediaKind::Book => targets
            .iter()
            .map(|u| {
                let mut q = SearchQuery {
                    text: format!("{} {}", t.name, u.name),
                    year: u.year,
                    ..base.clone()
                };
                if t.kind == MediaKind::Music {
                    q.artist = Some(t.name.clone());
                    q.album = Some(u.name.clone());
                } else {
                    q.author = Some(t.name.clone());
                    q.book = Some(u.name.clone());
                }
                q
            })
            .collect(),
    }
}

/// Is the release on the blocklist (same indexer entry, same info hash, or the same name for this title)?
pub fn blocked(rel: &Release, title: u64, list: &[BlockEntry]) -> bool {
    list.iter().any(|b| {
        (b.indexer_id == rel.indexer_id && b.guid == rel.guid)
            || (b.info_hash.is_some() && b.info_hash == rel.info_hash)
            || (b.title == title && b.release == rel.title)
    })
}

/// Every release with the units it would fill and why it must not be grabbed. `targets` are the units asked for; a release that also fills others is rejected. `active` are the grabs still downloading.
#[allow(clippy::too_many_arguments)]
pub fn evaluate(
    t: &Title,
    p: &Profile,
    all: &[Unit],
    targets: &[u64],
    rels: Vec<Release>,
    block: &[BlockEntry],
    active: &[Grab],
    today: &str,
) -> Vec<Candidate> {
    rels.into_iter()
        .map(|rel| {
            let Some(ids) = units_for(&rel, t, all) else {
                return Candidate {
                    release: rel,
                    units: vec![],
                    rejections: vec!["not for this title".into()],
                };
            };
            let refs: Vec<&Unit> = all.iter().filter(|u| ids.contains(&u.id)).collect();
            let queued = active.iter().any(|g| {
                g.state == GrabState::Downloading && g.units.iter().any(|u| ids.contains(u))
            });
            let mut rejections =
                rejections(&rel, &refs, p, today, blocked(&rel, t.id, block), queued);
            if !ids.iter().all(|u| targets.contains(u)) {
                rejections.push("also contains items that were not asked for".into());
            }
            Candidate {
                release: rel,
                units: ids,
                rejections,
            }
        })
        .collect()
}

/// What to do about a grab, given the torrent's state.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Wait,
    /// Downloaded: import it.
    Completed,
    /// Blocklist, remove and search again.
    Failed(String),
    /// Seeding limits reached: remove the torrent and its download data.
    SeedingDone,
}

pub fn check(
    g: &Grab,
    st: Option<&TorrentStatus>,
    now: u64,
    seed_ratio: f32,
    seed_hours: u32,
) -> Verdict {
    match g.state {
        GrabState::Downloading => {
            let Some(st) = st else {
                return Verdict::Failed("the torrent disappeared from the download client".into());
            };
            if st.state == TorrentState::Error {
                return Verdict::Failed(
                    st.error.clone().unwrap_or_else(|| "download error".into()),
                );
            }
            if st.state == TorrentState::Seeding || (st.size > 0 && st.done >= st.size) {
                return Verdict::Completed;
            }
            if st.state != TorrentState::Paused
                && st.done <= g.done
                && now.saturating_sub(g.progress_at) >= STALL_SECS
            {
                return Verdict::Failed("stalled: no progress for 6 hours".into());
            }
            Verdict::Wait
        }
        GrabState::Seeding => {
            let Some(st) = st else {
                return Verdict::SeedingDone;
            };
            let by_ratio = seed_ratio > 0.0 && st.ratio() >= seed_ratio;
            let by_time = seed_hours > 0
                && g.seeding_since
                    .is_some_and(|s| now.saturating_sub(s) >= u64::from(seed_hours) * 3600);
            if by_ratio || by_time {
                Verdict::SeedingDone
            } else {
                Verdict::Wait
            }
        }
        _ => Verdict::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_arr_types::{Ids, Link};
    use nocturne_core::server::Preferred;

    pub fn rel(title: &str, parsed: ParsedRelease) -> Release {
        Release {
            guid: title.into(),
            indexer_id: 1,
            indexer: "ix".into(),
            protocol: Protocol::Torrent,
            title: title.into(),
            size: 4 << 30,
            seeders: Some(10),
            leechers: None,
            link: Link::Magnet("magnet:?xt=urn:btih:00".into()),
            info_hash: None,
            published: None,
            categories: vec![],
            ids: Ids::default(),
            parsed,
        }
    }

    fn movie() -> (Title, Vec<Unit>) {
        let t = Title {
            id: 1,
            kind: MediaKind::Movie,
            name: "Night Train".into(),
            year: Some(2020),
            ids: Ids::default(),
            profile: 1,
            library: 1,
            monitored: true,
            added: 0,
        };
        (t, vec![unit(10, "Night Train", None, None)])
    }

    fn unit(id: u64, name: &str, s: Option<u32>, e: Option<u32>) -> Unit {
        Unit {
            id,
            title: 1,
            name: name.into(),
            season: s,
            episode: e,
            year: None,
            air_date: Some("2020-01-01".into()),
            ids: Ids::default(),
            monitored: true,
            file: None,
        }
    }

    fn file(q: Quality, revision: u8) -> FileRecord {
        FileRecord {
            paths: vec!["/l/x.mkv".into()],
            quality: q,
            revision,
            size: 1,
            release: "old".into(),
            imported: 0,
        }
    }

    fn pr(title: &str, q: Quality) -> ParsedRelease {
        ParsedRelease {
            title: title.into(),
            quality: q,
            ..Default::default()
        }
    }

    const TODAY: &str = "2026-11-28";

    #[test]
    fn wanted_until_cutoff_and_only_when_released() {
        let p = Profile::standard(MediaKind::Movie);
        let mut u = unit(1, "x", None, None);
        assert!(wants(&u, &p, TODAY));
        u.file = Some(file(Quality::WebDl720, 1));
        assert!(wants(&u, &p, TODAY), "below the Bluray-1080p cutoff");
        u.file = Some(file(Quality::Bluray1080, 1));
        assert!(!wants(&u, &p, TODAY));
        u.file = None;
        u.air_date = Some("2027-01-01".into());
        assert!(!wants(&u, &p, TODAY));
        u.air_date = None;
        assert!(wants(&u, &p, TODAY), "a movie without a date counts as out");
        u.episode = Some(1);
        assert!(!wants(&u, &p, TODAY), "an episode without a date does not");
        let mut off = p.clone();
        off.upgrades = false;
        let mut u = unit(1, "x", None, None);
        u.file = Some(file(Quality::WebDl720, 1));
        assert!(!wants(&u, &off, TODAY));
        u.monitored = false;
        u.file = None;
        assert!(!wants(&u, &p, TODAY));
    }

    #[test]
    fn upgrades_need_a_better_quality_or_a_proper() {
        let p = Profile::standard(MediaKind::Movie);
        let f = file(Quality::WebDl720, 1);
        assert!(is_upgrade(&p, None, &pr("x", Quality::Hdtv720)));
        assert!(is_upgrade(&p, Some(&f), &pr("x", Quality::Bluray1080)));
        assert!(!is_upgrade(&p, Some(&f), &pr("x", Quality::Hdtv720)));
        assert!(!is_upgrade(&p, Some(&f), &pr("x", Quality::WebDl720)));
        let mut proper = pr("x", Quality::WebDl720);
        proper.proper = true;
        assert!(is_upgrade(&p, Some(&f), &proper));
        // At the cutoff only a proper of the same quality still counts.
        let at = file(Quality::Bluray1080, 1);
        assert!(!is_upgrade(&p, Some(&at), &pr("x", Quality::Remux2160)));
        let mut rp = pr("x", Quality::Bluray1080);
        rp.repack = true;
        assert!(is_upgrade(&p, Some(&at), &rp));
        // A disallowed release never upgrades.
        assert!(!is_upgrade(&p, Some(&f), &pr("x", Quality::Cam)));
    }

    #[test]
    fn releases_map_to_units_by_name_year_season_and_episode() {
        let (t, us) = movie();
        let mut p = pr("Night Train", Quality::Bluray1080);
        p.year = Some(2020);
        assert_eq!(units_for(&rel("a", p.clone()), &t, &us), Some(vec![10]));
        p.year = Some(1990);
        assert_eq!(units_for(&rel("a", p.clone()), &t, &us), None, "a remake");
        p.title = "Night Training".into();
        p.year = None;
        assert_eq!(units_for(&rel("a", p.clone()), &t, &us), None);
        let mut r = rel("a", p);
        r.ids.imdb = Some("tt1".into());
        let mut t2 = t.clone();
        t2.ids.imdb = Some("tt1".into());
        assert_eq!(units_for(&r, &t2, &us), Some(vec![10]), "ids beat names");

        let mut show = t.clone();
        show.kind = MediaKind::Series;
        show.name = "The Office (US)".into();
        let mut eps = vec![
            unit(1, "Pilot", Some(1), Some(1)),
            unit(2, "Diversity Day", Some(1), Some(2)),
            unit(3, "Health Care", Some(1), Some(3)),
            unit(4, "Gay Witch Hunt", Some(2), Some(1)),
        ];
        eps[2].monitored = false;
        let mut sp = pr("the office us", Quality::WebDl1080);
        sp.season = Some(1);
        sp.full_season = true;
        assert_eq!(
            units_for(&rel("p", sp.clone()), &show, &eps),
            Some(vec![1, 2])
        );
        sp.full_season = false;
        sp.episodes = vec![2, 3];
        assert_eq!(
            units_for(&rel("e", sp.clone()), &show, &eps),
            Some(vec![2, 3])
        );
        sp.season = None;
        assert_eq!(units_for(&rel("e", sp), &show, &eps), None);

        let mut artist = t.clone();
        artist.kind = MediaKind::Music;
        artist.name = "Simon & Garfunkel".into();
        let albums = vec![unit(7, "Bookends", None, None)];
        let mut a = pr("Simon and Garfunkel", Quality::Flac);
        a.work = Some("Bookends".into());
        assert_eq!(
            units_for(&rel("m", a.clone()), &artist, &albums),
            Some(vec![7])
        );
        a.work = None;
        assert_eq!(units_for(&rel("m", a), &artist, &albums), None);
    }

    #[test]
    fn rejections_cover_every_rule() {
        let mut p = Profile::standard(MediaKind::Movie);
        let u = unit(1, "Night Train", None, None);
        let ok = rel(
            "Night.Train.2020.1080p.BluRay.x264-GRP",
            pr("Night Train", Quality::Bluray1080),
        );
        assert!(rejections(&ok, &[&u], &p, TODAY, false, false).is_empty());

        let has = |r: &Release, p: &Profile, u: &Unit, b, q, needle: &str| {
            let v = rejections(r, &[u], p, TODAY, b, q);
            assert!(v.iter().any(|s| s.contains(needle)), "{needle}: {v:?}");
        };
        has(&ok, &p, &u, true, false, "blocklist");
        has(&ok, &p, &u, false, true, "already downloading");
        let cam = rel("x", pr("Night Train", Quality::Cam));
        has(&cam, &p, &u, false, false, "not allowed");
        let mut nz = ok.clone();
        nz.protocol = Protocol::Usenet;
        has(&nz, &p, &u, false, false, "Usenet");
        let mut few = ok.clone();
        few.seeders = Some(0);
        has(&few, &p, &u, false, false, "seeders");
        p.min_mb = 5000;
        has(&ok, &p, &u, false, false, "too small");
        p.min_mb = 0;
        p.max_mb = 1000;
        has(&ok, &p, &u, false, false, "too large");
        p.max_mb = 0;
        p.required = vec!["REMUX".into()];
        has(&ok, &p, &u, false, false, "does not contain");
        p.required = vec![];
        p.ignored = vec!["x264".into()];
        has(&ok, &p, &u, false, false, "contains \"x264\"");
        p.ignored = vec![];
        p.languages = vec!["de".into()];
        has(&ok, &p, &u, false, false, "language en");
        p.languages = vec![];
        let mut have = u.clone();
        have.file = Some(file(Quality::Bluray1080, 1));
        has(&ok, &p, &have, false, false, "would not improve");
        let mut off = u.clone();
        off.monitored = false;
        has(&ok, &p, &off, false, false, "not monitored");
        let mut later = u.clone();
        later.air_date = Some("2099-01-01".into());
        has(&ok, &p, &later, false, false, "not out yet");
        // Size is judged per unit: a 4 GiB season pack of four episodes is 1 GiB each.
        p.max_mb = 1500;
        let units: Vec<Unit> = (1..=4)
            .map(|i| unit(i, "e", Some(1), Some(i as u32)))
            .collect();
        let refs: Vec<&Unit> = units.iter().collect();
        assert!(rejections(&ok, &refs, &p, TODAY, false, false).is_empty());
    }

    #[test]
    fn choose_prefers_quality_then_proper_preferred_seeders_and_packs() {
        let mut p = Profile::standard(MediaKind::Series);
        p.preferred = vec![Preferred {
            term: "HDR".into(),
            score: 10,
        }];
        let mk = |t: &str, q, seeders| {
            let mut r = rel(t, pr("s", q));
            r.seeders = Some(seeders);
            r
        };
        let a = mk("a.720p", Quality::WebDl720, 500);
        let b = mk("b.1080p", Quality::WebDl1080, 5);
        let c = mk("c.1080p.HDR", Quality::WebDl1080, 1);
        let picked = choose(vec![(a, vec![1]), (b, vec![1]), (c.clone(), vec![1])], &p);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].0.title, c.title);

        // Same rank: the pack wins and the single episodes it covers are dropped; an uncovered episode still goes.
        let pack = mk("pack", Quality::WebDl1080, 5);
        let e1 = mk("e1", Quality::WebDl1080, 5);
        let e9 = mk("e9", Quality::WebDl720, 5);
        let got = choose(
            vec![(e1, vec![1]), (pack, vec![1, 2, 3]), (e9, vec![9])],
            &p,
        );
        let titles: Vec<_> = got.iter().map(|(r, _)| r.title.as_str()).collect();
        assert_eq!(titles, ["pack", "e9"]);
    }

    fn grab(state: GrabState) -> Grab {
        Grab {
            id: 1,
            title: 1,
            units: vec![1],
            release: rel("r", ParsedRelease::default()),
            torrent: "ab".into(),
            state,
            added: 0,
            done: 100,
            progress_at: 1000,
            seeding_since: Some(1000),
            message: None,
        }
    }

    fn status(state: TorrentState, done: u64, uploaded: u64) -> TorrentStatus {
        TorrentStatus {
            id: "ab".into(),
            name: "r".into(),
            state,
            error: None,
            category: "c".into(),
            save_path: "/d".into(),
            size: 1000,
            done,
            uploaded,
            down_rate: 0,
            up_rate: 0,
            peers: 0,
            eta_secs: None,
            files: vec![],
            added: 0,
        }
    }

    #[test]
    fn download_verdicts() {
        let g = grab(GrabState::Downloading);
        let dl = status(TorrentState::Downloading, 100, 0);
        assert_eq!(check(&g, Some(&dl), 2000, 1.0, 0), Verdict::Wait);
        assert!(matches!(check(&g, None, 2000, 1.0, 0), Verdict::Failed(_)));
        let mut err = dl.clone();
        err.state = TorrentState::Error;
        err.error = Some("disk full".into());
        assert_eq!(
            check(&g, Some(&err), 2000, 1.0, 0),
            Verdict::Failed("disk full".into())
        );
        let done = status(TorrentState::Seeding, 1000, 0);
        assert_eq!(check(&g, Some(&done), 2000, 1.0, 0), Verdict::Completed);
        let late = 1000 + STALL_SECS;
        assert!(
            matches!(check(&g, Some(&dl), late, 1.0, 0), Verdict::Failed(s) if s.contains("stalled"))
        );
        let paused = status(TorrentState::Paused, 100, 0);
        assert_eq!(
            check(&g, Some(&paused), late, 1.0, 0),
            Verdict::Wait,
            "paused is not stalled"
        );

        let s = grab(GrabState::Seeding);
        assert_eq!(
            check(
                &s,
                Some(&status(TorrentState::Seeding, 1000, 500)),
                2000,
                1.0,
                0
            ),
            Verdict::Wait
        );
        assert_eq!(
            check(
                &s,
                Some(&status(TorrentState::Seeding, 1000, 1000)),
                2000,
                1.0,
                0
            ),
            Verdict::SeedingDone
        );
        assert_eq!(
            check(
                &s,
                Some(&status(TorrentState::Seeding, 1000, 0)),
                1000 + 3600,
                0.0,
                1
            ),
            Verdict::SeedingDone
        );
        assert_eq!(
            check(
                &s,
                Some(&status(TorrentState::Seeding, 1000, 5000)),
                2000,
                0.0,
                0
            ),
            Verdict::Wait,
            "no limits: seed on"
        );
        assert_eq!(
            check(&grab(GrabState::Failed), None, 0, 1.0, 0),
            Verdict::Wait
        );
    }
}
