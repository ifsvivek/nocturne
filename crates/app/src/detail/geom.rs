//! Sizes of the page's blocks. The page is laid out to these heights (every band has a fixed
//! height), so keyboard focus can scroll a row into view without measuring widgets: the same
//! numbers that size the widgets say where each row is.
use super::Detail;
use super::model::{self, DRow, Sec};
use crate::theme as t;
use nocturne_core::detail::Full;
use nocturne_core::source::{Art, Kind, Media};

/// The header and the breadcrumb strip float over the page; content starts below them.
pub fn top() -> f32 {
    t::header_h() + 29.0
}
pub const FACTS_H: f32 = 52.0;
/// Chip height and the gaps of the chip band (a gap above keeps the rule off the chips).
pub const CHIP_H: f32 = 28.0;
const CHIPS_TOP: f32 = t::S3;

/// Height of the chip band for these labels at window width `w`: chips wrap onto as many lines as
/// they need, and the band grows with them so they never run into the next band.
/// ponytail: text width is estimated per character (13 px font ~ 7.2 px); a measured width would be exact.
pub fn chips_h(labels: &[String], w: f32) -> f32 {
    let avail = (w - 2.0 * t::page_pad()).max(1.0);
    let mut lines = 1;
    let mut x = 0.0;
    for l in labels {
        let cw = l.chars().count() as f32 * 7.2 + 2.0 * t::S3 + 2.0;
        if x > 0.0 && x + t::S2 + cw > avail {
            lines += 1;
            x = cw;
        } else {
            x += if x > 0.0 { t::S2 + cw } else { cw };
        }
    }
    CHIPS_TOP + lines as f32 * CHIP_H + (lines - 1) as f32 * t::S2 + t::S4
}
/// Section heading block (title, one line of text) without its padding.
pub const HEAD_BLOCK: f32 = 70.0;
pub const TABS_H: f32 = t::S4 + 37.0;
pub const ROW_H: f32 = 72.0 + 2.0 * t::S3;
/// Text under an episode card's thumbnail (title, meta, three lines of overview).
pub const CARD_TEXT: f32 = 104.0;
pub const CARD_PX: u32 = 680;
pub const CARD_TARGET: f32 = 340.0;
pub const MSG_H: f32 = 70.0;
pub const PORTRAIT: (f32, f32) = (170.0, 255.0);
pub const LANDSCAPE: (f32, f32) = (300.0, 169.0);
/// Title and caption lines under a poster cell.
const CELL_TEXT: f32 = 12.0 + 22.0 + 18.0;

pub fn hero_art(m: &Media) -> Art {
    if m.backdrop_tag.is_none() && m.kind == Kind::Episode {
        Art::Landscape
    } else {
        Art::Backdrop
    }
}

/// Items with no backdrop of their own show their poster beside the text.
pub fn about_poster(m: &Media) -> bool {
    m.backdrop_tag.is_none()
        && matches!(
            m.kind,
            Kind::Person | Kind::MusicAlbum | Kind::MusicArtist | Kind::Playlist | Kind::BoxSet
        )
}

pub fn dims(s: Sec) -> (f32, f32) {
    if s.landscape() { LANDSCAPE } else { PORTRAIT }
}

pub fn strip_art(s: Sec) -> (Art, u32) {
    match s {
        Sec::Chapters => (Art::Chapter, 600),
        s if s.landscape() => (Art::Landscape, 600),
        _ => (Art::Poster, 340),
    }
}

/// Full-bleed hero height for a window of `win`.
pub fn hero_h(win: iced::Size) -> f32 {
    (win.width * 9.0 / 16.0)
        .min(win.height * 0.55)
        .max(500.0)
        .round()
}

/// Episode-card columns and each card's thumbnail width.
pub fn card_cols(width: f32) -> usize {
    nocturne_core::nav::fit_cols(width - 2.0 * t::page_pad(), CARD_TARGET)
}

pub fn card_w(width: f32) -> f32 {
    let avail = width - 2.0 * t::page_pad();
    (avail / card_cols(width) as f32).floor() - 2.0 * t::S3
}

pub fn card_h(width: f32) -> f32 {
    2.0 * t::S3 + (card_w(width) * 9.0 / 16.0).round() + t::S2 + CARD_TEXT
}

/// Height of one strip's cells.
pub fn strip_h(s: Sec) -> f32 {
    dims(s).1 + 2.0 * t::S3 + CELL_TEXT
}

/// Height of a whole strip block (heading and cells), rule included.
pub fn strip_block(s: Sec) -> f32 {
    2.0 * t::S8 + HEAD_BLOCK + t::S4 + strip_h(s) + 1.0
}

/// "Premiere / Studio / Added ..." rows of the metadata table (rating, year, runtime and genres are shown elsewhere on the page).
pub fn facts(m: &Media, f: &Full) -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = vec![];
    let mut put = |k: &'static str, x: Option<String>| {
        if let Some(x) = x.filter(|x| !x.is_empty()) {
            v.push((k, x));
        }
    };
    put(
        if m.kind == Kind::Person {
            "Born"
        } else {
            "Premiere"
        },
        f.premiere.as_deref().map(|d| d.chars().take(10).collect()),
    );
    put("Place", f.places.first().cloned());
    put("Studio", f.studios.first().map(|s| s.1.clone()));
    put(
        "Added",
        m.added.as_deref().map(|d| d.chars().take(10).collect()),
    );
    // Only on local items whose details came from an online provider.
    put("Details", m.details_from.clone());
    v
}

/// Height of the overview + facts band (text wraps, so the line count is an estimate; the band is
/// laid out to this height and clips).
pub fn about_h(m: &Media, f: &Full, width: f32) -> f32 {
    let text_w = (width - 2.0 * t::page_pad() - 300.0 - t::S8).clamp(280.0, 760.0);
    let per_line = (text_w / 8.0).floor().max(20.0) as usize;
    let chars = m
        .overview
        .as_deref()
        .map_or("No description available.".len(), |s| s.chars().count());
    let lines = chars.div_ceil(per_line).clamp(1, 14) as f32;
    let tag = if f.tagline.is_some() {
        30.0 + t::S3
    } else {
        0.0
    };
    let left = tag + lines * t::BODY_LG * 1.6;
    let right = facts(m, f).len() as f32 * 24.0;
    2.0 * t::S6 + left.max(right).max(60.0)
}

pub struct Geom {
    pub hero_end: f32,
    pub chips: Option<(f32, f32)>,
    pub list_top: f32,
    pub head_h: f32,
    pub list_body: f32,
    pub per_row: f32,
    pub strips: Vec<(Sec, f32)>,
    cols: usize,
}

impl Geom {
    pub fn of(d: &Detail) -> Self {
        let w = d.win.width;
        let empty = Full::default();
        let full = d.full.as_ref().unwrap_or(&empty);
        let mut y = top() + hero_h(d.win) + 1.0;
        let hero_end = y;
        y += FACTS_H + 1.0;
        let labels: Vec<String> = d.chips().iter().map(model::Chip::label).collect();
        let chips = (!labels.is_empty()).then(|| {
            let c = (y, y + chips_h(&labels, w) + 1.0);
            y = c.1;
            c
        });
        y += about_h(&d.media, full, w) + 1.0;
        let list_top = y;
        let kind = d.media.kind;
        let (mut head_h, mut body, mut per_row) = (0.0, 0.0, ROW_H);
        if model::has_list(kind) {
            head_h =
                2.0 * t::S8 + HEAD_BLOCK + if kind == Kind::Series { TABS_H } else { 0.0 } + 1.0;
            let n = d.episodes.items().len();
            if n == 0 {
                body = MSG_H + 1.0;
            } else if model::card_grid(kind) {
                per_row = card_h(w);
                body = n.div_ceil(card_cols(w)) as f32 * per_row;
            } else {
                body = n as f32 * (ROW_H + 1.0);
                per_row = ROW_H + 1.0;
            }
            y += head_h + body;
        }
        let mut strips = vec![];
        for s in Sec::ALL {
            if !d.sec_items(s).is_empty() {
                strips.push((s, y));
                y += strip_block(s);
            }
        }
        Self {
            hero_end,
            chips,
            list_top,
            head_h,
            list_body: body,
            per_row,
            strips,
            cols: if model::card_grid(kind) {
                card_cols(w)
            } else {
                1
            },
        }
    }

    /// Top and bottom of row `r` in page coordinates.
    pub fn span(&self, d: &Detail, r: DRow) -> (f32, f32) {
        match r {
            DRow::Actions => (0.0, self.hero_end),
            DRow::Chips => self.chips.unwrap_or((0.0, 0.0)),
            DRow::Seasons => (
                self.list_top + self.head_h - TABS_H - 1.0,
                self.list_top + self.head_h,
            ),
            DRow::List => {
                let top =
                    self.list_top + self.head_h + (d.focus.ep / self.cols) as f32 * self.per_row;
                (top, top + self.per_row)
            }
            DRow::Strip(s) => {
                let y = self
                    .strips
                    .iter()
                    .find(|(x, _)| *x == s)
                    .map_or(0.0, |x| x.1);
                (y, y + strip_block(s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_band_keeps_a_gap_and_grows_when_chips_wrap() {
        let few: Vec<String> = ["Animation", "Drama", "Mystery"].map(String::from).to_vec();
        let one_line = chips_h(&few, 1280.0);
        assert_eq!(
            one_line,
            t::S3 + CHIP_H + t::S4,
            "one line plus the gap above and below"
        );
        let many: Vec<String> = (0..30).map(|i| format!("Genre number {i}")).collect();
        let wrapped = chips_h(&many, 900.0);
        assert!(
            wrapped >= one_line + 2.0 * (CHIP_H + t::S2),
            "several lines: {wrapped}"
        );
        assert!(
            chips_h(&many, 2400.0) < wrapped,
            "a wider window needs fewer lines"
        );
    }

    #[test]
    fn hero_has_a_floor_and_follows_the_window() {
        let w = |x, y| iced::Size::new(x, y);
        assert_eq!(hero_h(w(1280.0, 800.0)), 500.0);
        assert_eq!(hero_h(w(2560.0, 1440.0)), 792.0);
    }

    #[test]
    fn cards_fill_the_width() {
        assert_eq!(card_cols(1280.0), 3);
        let (c, w) = (card_cols(1280.0), card_w(1280.0));
        assert!(c as f32 * (w + 2.0 * t::S3) <= 1280.0 - 2.0 * t::page_pad());
    }

    #[test]
    fn about_grows_with_the_text_but_is_capped() {
        let mut m = Media::from_item(&nocturne_jellyfin::Item::default());
        let f = Full::default();
        m.overview = Some("x".repeat(40));
        let short = about_h(&m, &f, 1280.0);
        m.overview = Some("x".repeat(100_000));
        let long = about_h(&m, &f, 1280.0);
        assert!(long > short);
        assert_eq!(long, 2.0 * t::S6 + 14.0 * t::BODY_LG * 1.6);
    }
}
