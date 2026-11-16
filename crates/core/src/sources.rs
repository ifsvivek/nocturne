//! Pure state behind the Sources screens and the root: status wording, the source filter, the
//! first-run routing and the progressive fan-out (answers from several sources arriving one by
//! one, stale ones dropped). No UI, no I/O.
use crate::catalog::Status;
use crate::merge::{Entry, Preferred, SeriesIds, merge};
use crate::source::{Media, SourceId};
use std::time::Instant;

/// One line for a source's status: Online, Offline (retrying in Ns), Signed out, Indexing n of m,
/// Disabled.
pub fn status_text(s: &Status, now: Instant) -> String {
    match s {
        Status::Connecting => "Connecting".into(),
        Status::Online => "Online".into(),
        Status::Offline { retry_at, .. } => {
            let d = retry_at.saturating_duration_since(now);
            // Round up so it never reads "0s" while still waiting.
            let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
            format!("Offline, retrying in {secs}s")
        }
        Status::SignedOut => "Signed out".into(),
        Status::Indexing { done, total: 0 } => format!("Indexing, {done} found"),
        Status::Indexing { done, total } => format!("Indexing {done} of {total}"),
        Status::Disabled => "Disabled".into(),
    }
}

/// Which sources a screen shows: everything, or one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceFilter {
    #[default]
    All,
    One(SourceId),
}

impl SourceFilter {
    pub fn admits(self, origin: SourceId) -> bool {
        match self {
            Self::All => true,
            Self::One(o) => o == origin,
        }
    }

    /// All, then each source in turn, then All again. A filter on a source that is gone resets.
    pub fn next(self, ids: &[SourceId]) -> Self {
        match self {
            Self::All => ids.first().map_or(Self::All, |i| Self::One(*i)),
            Self::One(o) => match ids.iter().position(|i| *i == o) {
                Some(p) => ids.get(p + 1).map_or(Self::All, |i| Self::One(*i)),
                None => Self::All,
            },
        }
    }
}

/// What the person chose on the welcome screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstRun {
    Local,
    Server,
    Both,
}

/// A step of first-run setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Folders,
    Server,
    Done,
}

impl FirstRun {
    /// Local goes to the folders screen, Server to the server address flow, Both starts with the
    /// server (its sign-in is the longer part) and then the folders.
    pub fn first(self) -> Step {
        match self {
            Self::Local => Step::Folders,
            Self::Server | Self::Both => Step::Server,
        }
    }

    /// The step after `finished` (the server signed in, or the folders confirmed).
    pub fn after(self, finished: Step) -> Step {
        match (self, finished) {
            (Self::Both, Step::Server) => Step::Folders,
            _ => Step::Done,
        }
    }
}

/// One per-source request over every online slot: `epoch` ties answers to the request that asked,
/// so a slower answer for a page the user left is dropped; a source that failed is noted, not fatal.
#[derive(Clone, Debug, Default)]
pub struct Fanout {
    pub epoch: u64,
    asked: Vec<SourceId>,
    answers: Vec<(SourceId, Vec<Media>)>,
    failed: Vec<SourceId>,
}

impl Fanout {
    pub fn new(epoch: u64, asked: Vec<SourceId>) -> Self {
        Self {
            epoch,
            asked,
            ..Self::default()
        }
    }

    /// Take one source's answer. False (and ignored) if it belongs to an older request, is for a
    /// source that was not asked, or came twice.
    pub fn accept(&mut self, epoch: u64, origin: SourceId, r: Option<Vec<Media>>) -> bool {
        if epoch != self.epoch
            || !self.asked.contains(&origin)
            || self.answers.iter().any(|(o, _)| *o == origin)
            || self.failed.contains(&origin)
        {
            return false;
        }
        match r {
            Some(v) => self.answers.push((origin, v)),
            None => self.failed.push(origin),
        }
        true
    }

    /// Sources that have not answered yet.
    pub fn pending(&self) -> usize {
        self.asked.len() - self.answers.len() - self.failed.len()
    }

    pub fn failed(&self) -> &[SourceId] {
        &self.failed
    }

    pub fn is_empty(&self) -> bool {
        self.asked.is_empty()
    }

    /// Every item that arrived so far, in `priority` order of its source, no merging (libraries,
    /// where nothing is a duplicate).
    pub fn flat(&self, priority: &[SourceId]) -> Vec<Media> {
        let mut a: Vec<&(SourceId, Vec<Media>)> = self.answers.iter().collect();
        a.sort_by_key(|(o, _)| {
            priority
                .iter()
                .position(|p| p == o)
                .unwrap_or(priority.len())
        });
        a.into_iter().flat_map(|(_, v)| v.iter().cloned()).collect()
    }

    /// What arrived so far, merged into entries (duplicates grouped).
    pub fn entries(
        &self,
        priority: &[SourceId],
        prefs: &Preferred,
        series: &SeriesIds,
    ) -> Vec<Entry> {
        merge(self.answers.clone(), priority, prefs, series)
    }

    /// Like [`entries`](Self::entries) for lists that are "most recent first" in every source
    /// (continue watching): the first of each source come first, then the seconds, and so on.
    pub fn interleaved(
        &self,
        priority: &[SourceId],
        prefs: &Preferred,
        series: &SeriesIds,
    ) -> Vec<Entry> {
        let mut v = self.entries(priority, prefs, series);
        let pos = |m: &Media| {
            self.answers
                .iter()
                .find(|(o, _)| *o == m.origin)
                .and_then(|(_, l)| l.iter().position(|x| x.id == m.id))
                .unwrap_or(usize::MAX)
        };
        v.sort_by_cached_key(|e| e.all().map(pos).min().unwrap_or(usize::MAX));
        v
    }

    /// Every answer so far, with its source (for ordering by position within a source).
    pub fn answers(&self) -> &[(SourceId, Vec<Media>)] {
        &self.answers
    }

    /// Optimistic edits (played / favourite) reach the answers too, so a rebuild keeps them.
    pub fn for_each_mut(&mut self, mut f: impl FnMut(&mut Media)) {
        self.answers
            .iter_mut()
            .flat_map(|(_, v)| v.iter_mut())
            .for_each(&mut f);
    }

    /// How many distinct sources contributed.
    pub fn contributors(&self) -> usize {
        self.answers.iter().filter(|(_, v)| !v.is_empty()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const A: SourceId = SourceId(1);
    const B: SourceId = SourceId(2);

    fn lib(o: SourceId, id: &str) -> Media {
        Media::from_item_in(
            o,
            &nocturne_jellyfin::Item {
                id: id.into(),
                name: id.into(),
                kind: "CollectionFolder".into(),
                ..Default::default()
            },
        )
    }

    #[test]
    fn status_wording() {
        let now = Instant::now();
        let t = |s| status_text(&s, now);
        assert_eq!(t(Status::Online), "Online");
        assert_eq!(t(Status::SignedOut), "Signed out");
        assert_eq!(t(Status::Disabled), "Disabled");
        assert_eq!(
            t(Status::Indexing { done: 3, total: 10 }),
            "Indexing 3 of 10"
        );
        assert_eq!(
            t(Status::Indexing { done: 3, total: 0 }),
            "Indexing, 3 found"
        );
        assert_eq!(
            t(Status::Offline {
                tries: 0,
                retry_at: now + Duration::from_millis(4200)
            }),
            "Offline, retrying in 5s"
        );
        assert_eq!(
            t(Status::Offline {
                tries: 3,
                retry_at: now
            }),
            "Offline, retrying in 0s"
        );
    }

    #[test]
    fn the_filter_cycles_and_resets_when_its_source_is_gone() {
        let ids = [A, B];
        let mut f = SourceFilter::All;
        f = f.next(&ids);
        assert_eq!(f, SourceFilter::One(A));
        f = f.next(&ids).next(&ids);
        assert_eq!(f, SourceFilter::All);
        assert!(SourceFilter::All.admits(B) && !SourceFilter::One(A).admits(B));
        assert_eq!(SourceFilter::One(B).next(&[A]), SourceFilter::All);
        assert_eq!(SourceFilter::All.next(&[]), SourceFilter::All);
    }

    #[test]
    fn first_run_routing() {
        assert_eq!(FirstRun::Local.first(), Step::Folders);
        assert_eq!(FirstRun::Server.first(), Step::Server);
        assert_eq!(FirstRun::Both.first(), Step::Server);
        assert_eq!(FirstRun::Local.after(Step::Folders), Step::Done);
        assert_eq!(FirstRun::Server.after(Step::Server), Step::Done);
        assert_eq!(FirstRun::Both.after(Step::Server), Step::Folders);
        assert_eq!(FirstRun::Both.after(Step::Folders), Step::Done);
    }

    #[test]
    fn answers_merge_as_they_arrive_and_stale_ones_are_dropped() {
        let mut f = Fanout::new(5, vec![A, B]);
        assert_eq!(f.pending(), 2);
        assert!(!f.accept(4, A, Some(vec![lib(A, "old")])), "older request");
        assert!(!f.accept(5, SourceId(9), Some(vec![])), "not asked");
        // B answers first; A is still pending, rows render already.
        assert!(f.accept(5, B, Some(vec![lib(B, "b1")])));
        assert_eq!(f.pending(), 1);
        let prio = [A, B];
        assert_eq!(f.flat(&prio).len(), 1);
        assert!(
            !f.accept(5, B, Some(vec![lib(B, "dup")])),
            "once per source"
        );
        assert!(f.accept(5, A, Some(vec![lib(A, "a1"), lib(A, "a2")])));
        let names: Vec<_> = f.flat(&prio).iter().map(|m| m.id.clone()).collect();
        assert_eq!(
            names,
            ["a1", "a2", "b1"],
            "priority order, not arrival order"
        );
        assert_eq!(f.pending(), 0);
        assert_eq!(f.contributors(), 2);
        assert_eq!(
            f.entries(&prio, &Preferred::default(), &SeriesIds::default())
                .len(),
            3
        );
    }

    #[test]
    fn recent_lists_interleave_across_sources() {
        let mut f = Fanout::new(1, vec![A, B]);
        f.accept(1, A, Some(vec![lib(A, "a1"), lib(A, "a2")]));
        f.accept(1, B, Some(vec![lib(B, "b1"), lib(B, "b2")]));
        let ids: Vec<_> = f
            .interleaved(&[A, B], &Preferred::default(), &SeriesIds::default())
            .iter()
            .map(|e| e.primary.id.clone())
            .collect();
        assert_eq!(ids, ["a1", "b1", "a2", "b2"]);
    }

    #[test]
    fn a_failed_source_is_noted_and_does_not_block_the_rest() {
        let mut f = Fanout::new(1, vec![A, B]);
        assert!(f.accept(1, A, None));
        assert!(f.accept(1, B, Some(vec![lib(B, "b")])));
        assert_eq!(f.failed(), [A]);
        assert_eq!(f.pending(), 0);
        assert_eq!(f.flat(&[A, B]).len(), 1);
        assert!(Fanout::new(2, vec![]).is_empty());
    }
}
