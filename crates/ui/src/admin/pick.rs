//! Which server the admin screen talks about. Admin acts on ONE Jellyfin source at a time; the
//! root offers every source and each one is probed (`system_info`): only those where the signed-in
//! account is an administrator are listed. Pure logic, so it tests without a window.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    Pending,
    Admin,
    /// Not an administrator, unreachable, or signed out.
    No,
}

/// Indices of the sources to offer, in the order the root listed them (priority order).
pub fn admins(p: &[Probe]) -> Vec<usize> {
    p.iter()
        .enumerate()
        .filter(|(_, x)| **x == Probe::Admin)
        .map(|(i, _)| i)
        .collect()
}

/// The default source: the first administrator, but only once every source listed before it has
/// answered (a slow first server must not lose to a quick second one).
pub fn default_choice(p: &[Probe]) -> Option<usize> {
    for (i, x) in p.iter().enumerate() {
        match x {
            Probe::Admin => return Some(i),
            Probe::Pending => return None,
            Probe::No => {}
        }
    }
    None
}

/// Every source has answered.
pub fn settled(p: &[Probe]) -> bool {
    p.iter().all(|x| *x != Probe::Pending)
}

/// The next (or previous) administrator source after `cur`, wrapping; `cur` when it is the only one.
pub fn cycle(p: &[Probe], cur: usize, forward: bool) -> usize {
    let a = admins(p);
    let Some(at) = a.iter().position(|i| *i == cur) else {
        return a.first().copied().unwrap_or(cur);
    };
    let n = a.len();
    a[if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use Probe::*;

    #[test]
    fn default_is_the_first_admin_once_earlier_sources_answered() {
        assert_eq!(default_choice(&[Pending, Pending]), None);
        assert_eq!(
            default_choice(&[Pending, Admin]),
            None,
            "waits for the first source"
        );
        assert_eq!(default_choice(&[No, Admin, Admin]), Some(1));
        assert_eq!(default_choice(&[Admin, Pending]), Some(0));
        assert_eq!(default_choice(&[No, No]), None);
        assert!(settled(&[No, Admin]) && !settled(&[No, Pending]));
        assert!(default_choice(&[]).is_none() && settled(&[]));
    }

    #[test]
    fn only_admin_sources_are_listed_and_cycle_wraps() {
        let p = [Admin, No, Admin, Pending, Admin];
        assert_eq!(admins(&p), [0, 2, 4]);
        assert_eq!(cycle(&p, 0, true), 2);
        assert_eq!(cycle(&p, 4, true), 0);
        assert_eq!(cycle(&p, 0, false), 4);
        assert_eq!(cycle(&[Admin], 0, true), 0);
        assert_eq!(
            cycle(&[No, Admin], 0, true),
            1,
            "a non-admin current falls back to the first"
        );
    }
}
