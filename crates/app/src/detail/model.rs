//! What a detail page is made of, without any widgets: its focusable rows, the buttons a kind of
//! item offers, the More menu, keyboard focus movement, playlist edits. Port of the pure parts of
//! the GPUI `ui_detail.rs` / `ui_playlists.rs`, so they test without a window.
use nocturne_core::detail::Full;
use nocturne_core::nav::{self, Dir, Move};
use nocturne_core::source::{Kind, Media};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sec {
    Next,
    Cast,
    Chapters,
    Similar,
    Extras,
    Collections,
    Kids,
    Kids2,
    Seasons,
}

impl Sec {
    /// Display order, top to bottom.
    pub const ALL: [Sec; 9] = [
        Sec::Next,
        Sec::Seasons,
        Sec::Kids,
        Sec::Kids2,
        Sec::Chapters,
        Sec::Cast,
        Sec::Extras,
        Sec::Collections,
        Sec::Similar,
    ];

    pub fn idx(self) -> usize {
        match self {
            Sec::Cast => 0,
            Sec::Chapters => 1,
            Sec::Similar => 2,
            Sec::Extras => 3,
            Sec::Kids => 4,
            Sec::Kids2 => 5,
            Sec::Next => 6,
            Sec::Seasons => 7,
            Sec::Collections => 8,
        }
    }

    pub fn title(self, kind: Kind) -> (&'static str, &'static str) {
        match (self, kind) {
            (Sec::Next, _) => ("Next up", "Pick up this show where you left off."),
            (Sec::Seasons, _) => ("Seasons", "Open a season on its own."),
            (Sec::Cast, _) => ("Cast & crew", "Select a person for their filmography."),
            (Sec::Chapters, _) => ("Chapters", "Jump straight to a scene."),
            (Sec::Similar, _) => (
                "More like this",
                "Chosen by the server from what this resembles.",
            ),
            (Sec::Collections, _) => ("Included in", "Collections this belongs to."),
            (Sec::Extras, _) => ("Extras", "Trailers and special features."),
            (Sec::Kids, Kind::Person) => ("Movies", "Films with this person."),
            (Sec::Kids2, Kind::Person) => ("Shows", "Series with this person."),
            _ => ("Related", ""),
        }
    }

    /// Wide 16:9 cells (stills) instead of posters.
    pub fn landscape(self) -> bool {
        matches!(self, Sec::Chapters | Sec::Extras | Sec::Next)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DRow {
    Actions,
    Chips,
    Seasons,
    List,
    Strip(Sec),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DAct {
    Play,
    PlayAll,
    Shuffle,
    Watched,
    Fav,
    Prev,
    Next,
    Series,
    More,
    View,
    /// Opens the versions and sources picker (offered only when there is a choice).
    Versions,
}

/// Entries of the More menu on a detail page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoreAct {
    FromStart,
    Watched,
    Fav,
    Info,
    AddToPlaylist,
    RenamePlaylist,
    DeletePlaylist,
    PlayNext,
    AddToQueue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Chip {
    Genre(String),
    Studio(String, String),
    Link(String, String),
}

impl Chip {
    /// The text on the chip (links carry an arrow).
    pub fn label(&self) -> String {
        match self {
            Chip::Genre(x) => x.clone(),
            Chip::Studio(_, n) => n.clone(),
            Chip::Link(n, _) => format!("{n} \u{2197}"),
        }
    }
}

/// Buttons a page offers, by kind.
pub fn actions(m: &Media, has_prev: bool, has_next: bool) -> Vec<DAct> {
    use DAct::*;
    let mut v = match m.kind {
        Kind::Movie | Kind::Video | Kind::Audio => vec![Play, Watched, Fav, More],
        Kind::Episode => {
            let mut v = vec![Play, Watched, Fav];
            if has_prev {
                v.push(Prev);
            }
            if has_next {
                v.push(Next);
            }
            if m.series_id.is_some() {
                v.push(Series);
            }
            v.push(More);
            v
        }
        Kind::Series => vec![Play, Watched, Fav],
        Kind::Season => {
            let mut v = vec![Play, Watched];
            if m.series_id.is_some() {
                v.push(Series);
            }
            v
        }
        Kind::BoxSet | Kind::Playlist | Kind::MusicAlbum => vec![PlayAll, Shuffle, Fav, More],
        Kind::Photo => vec![View, Fav],
        _ => vec![Fav],
    };
    if matches!(m.kind, Kind::Series | Kind::Season) {
        // Watched toggles on a whole show/season are not offered: the server flags episodes.
        v.retain(|a| *a != Watched);
    }
    v
}

/// (label, primary) of a button for `m`.
pub fn spec(a: DAct, m: &Media) -> (&'static str, bool) {
    match a {
        DAct::Play if m.progress().is_some() => ("Resume", true),
        DAct::Play => ("Play", true),
        DAct::PlayAll => ("Play all", true),
        DAct::Shuffle => ("Shuffle", false),
        DAct::Watched if m.played => ("Watched", false),
        DAct::Watched => ("Mark watched", false),
        DAct::Fav if m.favorite => ("\u{2665} Favourite", false),
        DAct::Fav => ("\u{2661} Favourite", false),
        DAct::Prev => ("\u{2039} Previous", false),
        DAct::Next => ("Next \u{203A}", false),
        DAct::Series => ("Go to series", false),
        DAct::More => ("More \u{2026}", false),
        DAct::View => ("View", true),
        DAct::Versions => ("Versions", false),
    }
}

/// Is a change to the server's (or the local library's) state; dimmed with writes off.
pub fn is_write(a: DAct) -> bool {
    matches!(a, DAct::Watched | DAct::Fav)
}

/// Order for shuffling a queue; xorshift is plenty for picking tracks.
pub fn shuffled(mut v: Vec<Media>) -> Vec<Media> {
    let mut s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
        | 1;
    for i in (1..v.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        v.swap(i, (s % (i as u64 + 1)) as usize);
    }
    v
}

/// Genre, studio and link chips of an item.
pub fn chips(m: &Media, f: Option<&Full>) -> Vec<Chip> {
    let mut v: Vec<Chip> = m.genres.iter().cloned().map(Chip::Genre).collect();
    if let Some(f) = f {
        v.extend(
            f.studios
                .iter()
                .map(|(i, n)| Chip::Studio(i.clone(), n.clone())),
        );
        v.extend(
            f.links
                .iter()
                .map(|(n, u)| Chip::Link(n.clone(), u.clone())),
        );
    }
    v
}

/// Kinds whose main content is a list (episodes, tracks, members).
pub fn has_list(k: Kind) -> bool {
    matches!(
        k,
        Kind::Series
            | Kind::Season
            | Kind::BoxSet
            | Kind::Playlist
            | Kind::MusicAlbum
            | Kind::MusicArtist
    )
}

/// Episodes and collection members are cards; tracks and playlist entries are rows.
pub fn card_grid(k: Kind) -> bool {
    matches!(k, Kind::Series | Kind::Season | Kind::BoxSet)
}

/// What focus moves over: row kinds in order, how many entries each holds.
#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub rows: Vec<DRow>,
    /// Entries per row (actions, chips, seasons, list items, strip cells), parallel to `rows`.
    pub lens: Vec<usize>,
    /// Columns of the card grid (1 for a plain list).
    pub cols: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    pub row: usize,
    pub col: usize,
    /// Selected entry of the list / card grid.
    pub ep: usize,
    pub season: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Same,
    Moved,
    /// The season tabs moved: load this season's episodes.
    Season(usize),
}

impl Shape {
    fn len_of(&self, r: usize) -> usize {
        self.lens.get(r).copied().unwrap_or(0)
    }

    pub fn current(&self, f: &Focus) -> DRow {
        self.rows.get(f.row).copied().unwrap_or(DRow::Actions)
    }
}

/// One arrow press. Inside the list the selection moves in two dimensions and only leaves at its
/// top or bottom edge; the season tabs and every other row move sideways.
pub fn step(f: &mut Focus, d: Dir, s: &Shape) -> Step {
    if s.rows.is_empty() {
        return Step::Same;
    }
    let cur = s.current(f);
    let cols = s.cols.max(1);
    let n = s.len_of(f.row);
    if cur == DRow::List
        && let Move::Inside(i) = nav::grid_move(f.ep, cols, n, d)
    {
        return if i != f.ep {
            f.ep = i;
            Step::Moved
        } else {
            Step::Same
        };
    }
    if matches!(d, Dir::Up | Dir::Down) {
        let before = f.row;
        f.row = match d {
            Dir::Up => f.row.saturating_sub(1),
            _ => (f.row + 1).min(s.rows.len() - 1),
        };
        if f.row == before {
            return Step::Same;
        }
        let len = s.len_of(f.row);
        f.col = f.col.min(len.saturating_sub(1));
        if s.rows[f.row] == DRow::List {
            f.ep = if d == Dir::Up {
                (len.saturating_sub(1) / cols * cols + f.ep % cols).min(len.saturating_sub(1))
            } else {
                f.ep.min(cols.min(len.max(1)).saturating_sub(1))
            };
        }
        return Step::Moved;
    }
    match cur {
        DRow::Seasons => {
            let to = nav::step(f.season, n.max(1), n, d);
            if to == f.season {
                return Step::Same;
            }
            f.season = to;
            Step::Season(to)
        }
        DRow::List => Step::Same,
        _ => {
            let to = nav::step(f.col, n.max(1), n, d);
            if to == f.col {
                return Step::Same;
            }
            f.col = to;
            Step::Moved
        }
    }
}

/// What a keypress does to the selected entry of an owned playlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Remove,
    Up,
    Down,
}

impl Edit {
    pub fn of(key: &str) -> Option<Self> {
        match key {
            "x" | "delete" => Some(Self::Remove),
            "[" => Some(Self::Up),
            "]" => Some(Self::Down),
            _ => None,
        }
    }

    /// The server call for entry `idx` of `len`: `Some(None)` = remove, `Some(Some(n))` = move to
    /// `n`, `None` at an edge (nothing to do).
    pub fn plan(self, idx: usize, len: usize) -> Option<Option<usize>> {
        if idx >= len {
            return None;
        }
        match self {
            Self::Remove => Some(None),
            Self::Up => idx.checked_sub(1).map(Some),
            Self::Down => (idx + 1 < len).then_some(Some(idx + 1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_jellyfin as jf;

    fn m(kind: &str) -> Media {
        Media::from_item(&jf::Item {
            id: "x".into(),
            kind: kind.into(),
            series_id: Some("s".into()),
            ..Default::default()
        })
    }

    #[test]
    fn actions_by_kind() {
        use DAct::*;
        assert_eq!(
            actions(&m("Movie"), false, false),
            [Play, Watched, Fav, More]
        );
        assert_eq!(
            actions(&m("Episode"), true, true),
            [Play, Watched, Fav, Prev, Next, Series, More]
        );
        assert_eq!(
            actions(&m("Episode"), false, false),
            [Play, Watched, Fav, Series, More]
        );
        assert_eq!(actions(&m("Series"), false, false), [Play, Fav]);
        assert_eq!(actions(&m("Season"), false, false), [Play, Series]);
        assert_eq!(
            actions(&m("Playlist"), false, false),
            [PlayAll, Shuffle, Fav, More]
        );
        assert_eq!(actions(&m("Person"), false, false), [Fav]);
        assert_eq!(actions(&m("Photo"), false, false), [View, Fav]);
        assert_eq!(actions(&m("Book"), false, false), [Fav]);
    }

    #[test]
    fn shuffle_keeps_every_item() {
        let v: Vec<Media> = (0..50)
            .map(|i| {
                let mut x = m("Audio");
                x.id = i.to_string();
                x
            })
            .collect();
        let mut ids: Vec<usize> = shuffled(v).iter().map(|x| x.id.parse().unwrap()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn playlist_key_edits_follow_the_list_edges() {
        assert_eq!(Edit::of("x"), Some(Edit::Remove));
        assert_eq!(Edit::of("]"), Some(Edit::Down));
        assert_eq!(Edit::of("q"), None);
        assert_eq!(Edit::Remove.plan(0, 1), Some(None));
        assert_eq!(Edit::Up.plan(0, 3), None, "first entry cannot go up");
        assert_eq!(Edit::Up.plan(2, 3), Some(Some(1)));
        assert_eq!(Edit::Down.plan(2, 3), None, "last entry cannot go down");
        assert_eq!(Edit::Down.plan(0, 3), Some(Some(1)));
        assert_eq!(Edit::Remove.plan(5, 3), None, "stale selection");
    }

    /// Actions(3), Seasons(2), List(5 cards in 2 columns), one strip of 4.
    fn shape() -> Shape {
        Shape {
            rows: vec![
                DRow::Actions,
                DRow::Seasons,
                DRow::List,
                DRow::Strip(Sec::Cast),
            ],
            lens: vec![3, 2, 5, 4],
            cols: 2,
        }
    }

    #[test]
    fn focus_walks_rows_and_columns() {
        let s = shape();
        let mut f = Focus::default();
        assert_eq!(step(&mut f, Dir::Right, &s), Step::Moved);
        assert_eq!(f.col, 1);
        assert_eq!(step(&mut f, Dir::Left, &s), Step::Moved);
        assert_eq!(step(&mut f, Dir::Left, &s), Step::Same, "no wrap");
        assert_eq!(step(&mut f, Dir::Up, &s), Step::Same, "top row stays");
        assert_eq!(step(&mut f, Dir::Down, &s), Step::Moved);
        assert_eq!(s.current(&f), DRow::Seasons);
    }

    #[test]
    fn season_tabs_ask_for_their_episodes() {
        let s = shape();
        let mut f = Focus {
            row: 1,
            ..Default::default()
        };
        assert_eq!(step(&mut f, Dir::Right, &s), Step::Season(1));
        assert_eq!(step(&mut f, Dir::Right, &s), Step::Same, "last tab");
        assert_eq!(f.season, 1);
    }

    #[test]
    fn list_moves_in_two_dimensions_and_leaves_at_its_edges() {
        let s = shape();
        let mut f = Focus {
            row: 1,
            ..Default::default()
        };
        // Down from the tabs enters the grid on its first row, same column clamp.
        assert_eq!(step(&mut f, Dir::Down, &s), Step::Moved);
        assert_eq!((s.current(&f), f.ep), (DRow::List, 0));
        assert_eq!(step(&mut f, Dir::Right, &s), Step::Moved);
        assert_eq!(step(&mut f, Dir::Down, &s), Step::Moved);
        assert_eq!(f.ep, 3, "one grid row down");
        assert_eq!(step(&mut f, Dir::Down, &s), Step::Moved);
        assert_eq!(f.ep, 4, "short last row lands on the last card");
        assert_eq!(step(&mut f, Dir::Right, &s), Step::Same);
        assert_eq!(step(&mut f, Dir::Down, &s), Step::Moved);
        assert_eq!(
            s.current(&f),
            DRow::Strip(Sec::Cast),
            "leaves at the bottom"
        );
        assert_eq!(step(&mut f, Dir::Up, &s), Step::Moved);
        assert_eq!(s.current(&f), DRow::List);
        assert_eq!(f.ep, 4, "re-enters on the last grid row");
    }

    #[test]
    fn strip_stops_at_its_ends() {
        let s = shape();
        let mut f = Focus {
            row: 3,
            col: 3,
            ..Default::default()
        };
        assert_eq!(step(&mut f, Dir::Right, &s), Step::Same);
        assert_eq!(step(&mut f, Dir::Down, &s), Step::Same, "last row stays");
    }
}
