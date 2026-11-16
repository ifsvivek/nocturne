//! Drop-down menu model for the Settings rows with several choices. Pure state, no UI: the view
//! paints the list, `Root` feeds keys in and applies what comes back.

/// An open menu: which settings row it belongs to, how many entries, which one is highlighted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropdown {
    pub row: usize,
    pub sel: usize,
    n: usize,
}

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Still open (highlight may have moved).
    Stay,
    /// Closed without a change.
    Close,
    /// Closed, entry chosen.
    Pick(usize),
}

impl Dropdown {
    /// Opens on the current entry `cur` (clamped) of `n` entries. `None` when there is nothing to choose.
    pub fn open(row: usize, n: usize, cur: usize) -> Option<Self> {
        (n > 0).then(|| Self {
            row,
            sel: cur.min(n - 1),
            n,
        })
    }

    /// Up / Down / Home / End move (no wrap), Enter / Space choose, Esc / Backspace close. The
    /// menu is modal, so any other key is ignored and leaves it open.
    pub fn key(&mut self, key: &str) -> Outcome {
        match key {
            "up" => self.sel = self.sel.saturating_sub(1),
            "down" => self.sel = (self.sel + 1).min(self.n - 1),
            "home" => self.sel = 0,
            "end" => self.sel = self.n - 1,
            "enter" | "space" => return Outcome::Pick(self.sel),
            "escape" | "backspace" => return Outcome::Close,
            _ => {}
        }
        Outcome::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_the_current_entry_and_never_on_nothing() {
        assert_eq!(Dropdown::open(2, 0, 0), None);
        let d = Dropdown::open(2, 5, 3).unwrap();
        assert_eq!((d.row, d.sel), (2, 3));
        assert_eq!(Dropdown::open(2, 5, 99).unwrap().sel, 4, "clamped");
    }

    #[test]
    fn keys_move_clamp_choose_and_close() {
        let mut d = Dropdown::open(7, 4, 1).unwrap();
        assert_eq!(d.key("down"), Outcome::Stay);
        assert_eq!(d.sel, 2);
        d.key("end");
        d.key("down");
        assert_eq!(d.sel, 3, "no wrap at the end");
        d.key("home");
        d.key("up");
        assert_eq!(d.sel, 0, "no wrap at the start");
        d.key("down");
        assert_eq!(d.key("enter"), Outcome::Pick(1));
        assert_eq!(d.key("space"), Outcome::Pick(1));
        assert_eq!(d.key("escape"), Outcome::Close);
        assert_eq!(d.key("x"), Outcome::Stay, "other keys are swallowed");
        assert_eq!(d.sel, 1);
    }
}
