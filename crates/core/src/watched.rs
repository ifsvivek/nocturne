//! One watched state across sources (opt-in: `Settings::sync_watched`). When the user marks a title
//! watched or finishes it, the same state is applied to its other copies: the same title on another
//! source by provider-id identity (movies, series) or series ids + season + episode (episodes).
//! Copies that merely look alike ("possible duplicates") are never touched. Pure decisions live in
//! [`plan`]; [`find_copies`] does the reads. Docs: docs/WRITES.md.
use crate::backend::Backend;
use crate::merge::{SeriesIds, id_keys, identity_keys};
use crate::source::{Kind, Media, SourceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// The local library's own state file: needs no setting.
    Local,
    /// A server: needs the master write switch and a real login.
    Server,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Played(bool),
    /// A stop position (seconds) in an item of `dur` seconds.
    Resume {
        secs: f64,
        dur: f64,
    },
}

/// Why a change happens. Only the user (a click, a finished playback) cascades; a state that came
/// in from elsewhere (Trakt history import, a server refresh) never does, so nothing ping-pongs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    User,
    Incoming,
}

#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub enabled: bool,
    pub allow_server_writes: bool,
    pub dev_token: bool,
    pub sync_progress: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub origin: SourceId,
    pub id: String,
    pub via: Via,
    pub change: Change,
}

/// A stop position carried over to a copy with another runtime (a different cut): by fraction, and
/// not at all when either length is unknown.
pub fn scaled(secs: f64, dur: f64, to_dur: f64) -> Option<f64> {
    (secs > 0.0 && dur > 0.0 && to_dur > 0.0).then(|| (secs / dur).min(1.0) * to_dur)
}

/// The writes to make for `change` on `copies` of the item `from`. `via_of` says how a source is
/// written to (`None`: unavailable, skipped). Server copies are skipped silently when server changes
/// are off or in developer-token mode; resume positions also need `sync_progress`.
pub fn plan(
    from: (SourceId, &str),
    copies: &[Media],
    change: Change,
    cause: Cause,
    gate: Gate,
    via_of: impl Fn(SourceId) -> Option<Via>,
) -> Vec<Step> {
    if !gate.enabled || cause != Cause::User {
        return vec![];
    }
    if matches!(change, Change::Resume { .. }) && !gate.sync_progress {
        return vec![];
    }
    let mut out: Vec<Step> = vec![];
    for c in copies {
        if (c.origin, c.id.as_str()) == from || out.iter().any(|s| s.origin == c.origin) {
            continue;
        }
        let Some(via) = via_of(c.origin) else {
            continue;
        };
        if via == Via::Server && (!gate.allow_server_writes || gate.dev_token) {
            continue;
        }
        let change = match change {
            Change::Played(_) => change,
            Change::Resume { secs, dur } => match c.duration_s.and_then(|d| scaled(secs, dur, d)) {
                Some(s) => Change::Resume {
                    secs: s,
                    dur: c.duration_s.unwrap_or(0.0),
                },
                None => continue,
            },
        };
        out.push(Step {
            origin: c.origin,
            id: c.id.clone(),
            via,
            change,
        });
    }
    out
}

/// A finished item: past this fraction of its length counts as watched (Jellyfin's default is 90%).
pub const FINISHED: f64 = 0.9;

pub fn finished(pos: f64, dur: f64) -> bool {
    dur > 0.0 && pos / dur >= FINISHED
}

/// Does `cand` (ids filled in) share a strong identity with `keys`?
fn shares(keys: &[String], cand: &Media, series: &SeriesIds) -> bool {
    identity_keys(cand, series).iter().any(|x| keys.contains(x))
}

/// The copy of `m` on every other source in `backends`, by strong identity only. Reads: a title
/// search per source, then the provider ids of the hits (episodes: the series first, then its season).
/// ponytail: relies on the title search finding the other copy; a copy under a very different name is missed.
pub async fn find_copies(m: &Media, backends: &[(SourceId, Backend)]) -> Vec<Media> {
    if !matches!(m.kind, Kind::Movie | Kind::Series | Kind::Episode) {
        return vec![];
    }
    let mut series = SeriesIds::default();
    let own = backends.iter().find(|(o, _)| *o == m.origin);
    if m.kind == Kind::Episode
        && let (Some((_, b)), Some(sid)) = (own, &m.series_id)
        && let Ok(found) = b.provider_ids(std::slice::from_ref(sid)).await
        && let Some(ids) = found.get(sid)
    {
        series.set(m.origin, sid.clone(), ids.clone());
    }
    let keys = identity_keys(m, &series);
    if keys.is_empty() {
        return vec![];
    }
    let (term, want) = match m.kind {
        Kind::Episode => (m.series_name.clone().unwrap_or_default(), Kind::Series),
        _ => (m.title.clone(), m.kind),
    };
    if term.is_empty() {
        return vec![];
    }
    let series_keys = match (&m.series_id, m.kind) {
        (Some(sid), Kind::Episode) => series
            .get(m.origin, sid)
            .map(|i| id_keys("series", i))
            .unwrap_or_default(),
        _ => vec![],
    };
    let mut out = vec![];
    for (_, b) in backends.iter().filter(|(o, _)| *o != m.origin) {
        let Ok(hits) = b.search(&term).await else {
            continue;
        };
        let mut hits: Vec<Media> = hits
            .into_iter()
            .filter(|h| h.kind == want)
            .take(20)
            .collect();
        let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
        if let Ok(found) = b.provider_ids(&ids).await {
            for h in &mut hits {
                if let Some(p) = found.get(&h.id) {
                    h.ids = p.clone();
                }
            }
        }
        let none = SeriesIds::default();
        let Some(hit) = hits.iter().find(|h| {
            if m.kind == Kind::Episode {
                id_keys("series", &h.ids)
                    .iter()
                    .any(|k| series_keys.contains(k))
            } else {
                shares(&keys, h, &none)
            }
        }) else {
            continue;
        };
        if m.kind != Kind::Episode {
            out.push(hit.clone());
            continue;
        }
        let Ok(seasons) = b.seasons(&hit.id).await else {
            continue;
        };
        let Some(season) = seasons.iter().find(|s| s.number == m.season_number) else {
            continue;
        };
        if let Ok(eps) = b.episodes(&hit.id, &season.id).await
            && let Some(e) = eps.into_iter().find(|e| e.number == m.number)
        {
            out.push(e);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ProviderIds;

    fn item(origin: u32, id: &str, dur: Option<f64>) -> Media {
        Media {
            origin: SourceId(origin),
            id: id.into(),
            duration_s: dur,
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        }
    }

    const ON: Gate = Gate {
        enabled: true,
        allow_server_writes: true,
        dev_token: false,
        sync_progress: true,
    };

    fn via(o: SourceId) -> Option<Via> {
        match o.0 {
            1 => Some(Via::Local),
            2 | 3 => Some(Via::Server),
            _ => None,
        }
    }

    fn run(change: Change, cause: Cause, gate: Gate) -> Vec<Step> {
        let copies = [
            item(2, "a", None),
            item(1, "b", None),
            item(3, "c", None),
            item(9, "d", None),
        ];
        plan((SourceId(2), "a"), &copies, change, cause, gate, via)
    }

    #[test]
    fn off_changes_nothing() {
        let g = Gate {
            enabled: false,
            ..ON
        };
        assert!(run(Change::Played(true), Cause::User, g).is_empty());
    }

    #[test]
    fn on_reaches_other_copies_not_itself_not_unavailable() {
        let s = run(Change::Played(true), Cause::User, ON);
        let ids: Vec<_> = s.iter().map(|s| (s.origin.0, s.via)).collect();
        assert_eq!(ids, [(1, Via::Local), (3, Via::Server)]);
    }

    #[test]
    fn writes_off_or_dev_token_leaves_only_local() {
        for g in [
            Gate {
                allow_server_writes: false,
                ..ON
            },
            Gate {
                dev_token: true,
                ..ON
            },
        ] {
            let s = run(Change::Played(false), Cause::User, g);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].via, Via::Local);
        }
    }

    #[test]
    fn incoming_state_never_cascades() {
        assert!(run(Change::Played(true), Cause::Incoming, ON).is_empty());
    }

    #[test]
    fn resume_needs_sync_progress_and_scales_by_fraction() {
        let r = Change::Resume {
            secs: 30.0,
            dur: 120.0,
        };
        let g = Gate {
            sync_progress: false,
            ..ON
        };
        assert!(run(r, Cause::User, g).is_empty());
        let copies = [item(1, "b", Some(200.0)), item(3, "c", None)];
        let s = plan((SourceId(2), "a"), &copies, r, Cause::User, ON, via);
        assert_eq!(s.len(), 1, "a copy of unknown length is skipped");
        assert_eq!(
            s[0].change,
            Change::Resume {
                secs: 50.0,
                dur: 200.0
            }
        );
        assert_eq!(scaled(0.0, 10.0, 10.0), None);
    }

    #[test]
    fn finished_at_ninety_percent() {
        assert!(finished(91.0, 100.0) && !finished(80.0, 100.0) && !finished(5.0, 0.0));
    }

    #[test]
    fn identity_requires_a_shared_id() {
        let mut a = item(1, "x", None);
        a.kind = Kind::Movie;
        a.ids = ProviderIds {
            imdb: Some("tt1".into()),
            ..Default::default()
        };
        let mut b = a.clone();
        b.ids = ProviderIds {
            imdb: Some("TT1".into()),
            ..Default::default()
        };
        let s = SeriesIds::default();
        assert!(shares(&identity_keys(&a, &s), &b, &s));
        b.ids = ProviderIds::default();
        assert!(
            !shares(&identity_keys(&a, &s), &b, &s),
            "title alone never matches"
        );
    }
}
