//! Loading state of one screen region (a home row, a grid, the hero), so the UI can show skeleton
//! placeholders while it loads. `Tracker` owns the request epoch: a late answer to an older
//! request never overwrites the region the user has since moved on from.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// A request is in flight and nothing is shown yet: draw skeletons.
    Loading,
    Ready,
    /// The request succeeded with nothing in it.
    Empty,
    Error,
}

impl Region {
    /// From the plain facts a screen already holds. `failed` wins over `loading` (a retry keeps
    /// showing the error until it succeeds); anything shown beats a loading refresh.
    pub fn of(loading: bool, failed: bool, shown: usize) -> Self {
        match (shown, loading, failed) {
            (n, _, _) if n > 0 => Self::Ready,
            (_, _, true) => Self::Error,
            (_, true, _) => Self::Loading,
            _ => Self::Empty,
        }
    }

    pub fn skeleton(self) -> bool {
        self == Self::Loading
    }
}

/// How many placeholder cells fill a row of `width` px (at least one, at most `max`).
pub fn skeleton_cells(width: f32, cell: f32, gap: f32, max: usize) -> usize {
    (((width + gap) / (cell + gap).max(1.0)).floor() as usize).clamp(1, max.max(1))
}

#[derive(Clone, Debug)]
pub struct Tracker {
    epoch: u64,
    loading: bool,
    failed: bool,
    shown: usize,
}

impl Default for Tracker {
    fn default() -> Self {
        Self {
            epoch: 0,
            loading: true,
            failed: false,
            shown: 0,
        }
    }
}

impl Tracker {
    /// Start a request; the returned epoch travels with it. Content already shown stays.
    pub fn begin(&mut self) -> u64 {
        self.epoch += 1;
        (self.loading, self.failed) = (true, false);
        self.epoch
    }

    /// Record an answer (`Ok(count)` or `Err`); false and ignored when `epoch` is stale.
    pub fn finish(&mut self, epoch: u64, result: Result<usize, ()>) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.loading = false;
        match result {
            Ok(n) => (self.failed, self.shown) = (false, n),
            Err(()) => self.failed = true,
        }
        true
    }

    /// The user left: answers to everything asked so far are dropped.
    pub fn cancel(&mut self) {
        self.epoch += 1;
        self.loading = false;
    }

    pub fn region(&self) -> Region {
        Region::of(self.loading, self.failed, self.shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_from_facts() {
        assert_eq!(Region::of(true, false, 0), Region::Loading);
        assert_eq!(
            Region::of(true, false, 5),
            Region::Ready,
            "refresh keeps content"
        );
        assert_eq!(Region::of(false, false, 0), Region::Empty);
        assert_eq!(Region::of(false, true, 0), Region::Error);
        assert_eq!(Region::of(true, true, 0), Region::Error);
        assert!(Region::Loading.skeleton() && !Region::Empty.skeleton());
    }

    #[test]
    fn tracker_drops_stale_answers() {
        let mut t = Tracker::default();
        assert_eq!(
            t.region(),
            Region::Loading,
            "skeleton before the first request"
        );
        let a = t.begin();
        let b = t.begin();
        assert!(!t.finish(a, Ok(9)), "older request is ignored");
        assert_eq!(t.region(), Region::Loading);
        assert!(t.finish(b, Ok(0)));
        assert_eq!(t.region(), Region::Empty);
        let c = t.begin();
        assert!(t.finish(c, Err(())));
        assert_eq!(t.region(), Region::Error);
        let d = t.begin();
        assert!(t.finish(d, Ok(4)));
        assert_eq!(t.region(), Region::Ready);
        let e = t.begin();
        assert_eq!(t.region(), Region::Ready, "refreshing keeps what is shown");
        t.cancel();
        assert!(!t.finish(e, Ok(0)));
    }

    #[test]
    fn skeleton_count_fits_the_row() {
        assert_eq!(skeleton_cells(1000.0, 150.0, 16.0, 12), 6);
        assert_eq!(skeleton_cells(10.0, 150.0, 16.0, 12), 1);
        assert_eq!(skeleton_cells(5000.0, 150.0, 16.0, 8), 8);
    }
}
