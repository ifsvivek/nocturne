//! Pure rules of the player page that need neither a window nor mpv: how the loading overlay's
//! fill follows mpv's cache state, when the overlay leaves, list navigation and menu placement.
use iced::{Point, Rectangle, Size};
use std::time::Duration;

/// Seconds of read-ahead that count as "enough to start" for the opening fill.
pub const READY_AHEAD: f64 = 6.0;
/// A cache stall must last this long before the light overlay shows (short blips stay invisible).
pub const STALL_SHOW_AFTER: Duration = Duration::from_millis(400);

/// Opening fill 0..=1 from the demuxer: nothing until the file is open, then 30 % for "open" and
/// the rest as the read-ahead builds up to [`READY_AHEAD`] seconds.
pub fn open_fill(loaded: bool, buffered_secs: f64) -> f32 {
    if !loaded {
        return 0.0;
    }
    (0.3 + 0.7 * (buffered_secs / READY_AHEAD).clamp(0.0, 1.0)) as f32
}

/// Mid-playback stall fill 0..=1 from mpv's `cache-buffering-state` (percent).
pub fn stall_fill(percent: Option<f64>) -> f32 {
    percent.map_or(0.0, |p| (p / 100.0).clamp(0.0, 1.0) as f32)
}

/// Has playback started, so the opening overlay can leave? The file is open, mpv knows a
/// position and it is not waiting on the cache; a resume prompt also counts (the overlay must
/// not hide the question).
pub fn started(loaded: bool, has_position: bool, cache_wait: bool, prompt: bool) -> bool {
    loaded && (prompt || (has_position && !cache_wait))
}

/// Light stall overlay: waiting on the cache for longer than [`STALL_SHOW_AFTER`].
pub fn stall_visible(since: Option<Duration>) -> bool {
    since.is_some_and(|d| d >= STALL_SHOW_AFTER)
}

/// Up/Down in a list of `len` rows.
pub fn list_nav(key: &str, sel: usize, len: usize) -> usize {
    match key {
        "up" => sel.saturating_sub(1),
        "down" => (sel + 1).min(len.saturating_sub(1)),
        _ => sel,
    }
}

/// Top-left corner for a `w` x `h` menu opened at `at`, kept inside the window.
pub fn menu_origin(at: Point, win: Size, w: f32, h: f32) -> Point {
    Point::new(
        at.x.min(win.width - w - 8.0).max(0.0),
        at.y.min(win.height - h - 8.0).max(0.0),
    )
}

/// Left edge of a tooltip `w` wide centred on `x`, kept inside a window `win_w` wide.
pub fn tip_left(x: f32, w: f32, win_w: f32) -> f32 {
    (x - w / 2.0).clamp(8.0, (win_w - w - 8.0).max(8.0))
}

/// The rectangle the in-window video (a Windows child window the UI cannot draw over) may take:
/// the hole, less the controls band at its bottom while the controls show. `None` while the UI
/// has something to draw over the picture (menus, prompts, the loading overlay): the window is
/// hidden then.
pub fn video_rect(
    hole: Rectangle,
    bar_h: f32,
    bar_shown: bool,
    covered: bool,
) -> Option<Rectangle> {
    let h = hole.height - if bar_shown { bar_h } else { 0.0 };
    (!covered && hole.width >= 2.0 && h >= 2.0).then_some(Rectangle { height: h, ..hole })
}

/// A pointer event forwarded by mpv from the embedded picture: `"move x y"`, `"click x y"`,
/// `"dbl x y"` or `"right x y"` (device pixels inside the picture window).
pub fn parse_input(s: &str) -> Option<(&str, Point)> {
    let mut it = s.split_whitespace();
    let kind = it.next()?;
    let mut n = || it.next().and_then(|v| v.parse::<f32>().ok());
    let (x, y) = (n().unwrap_or(0.0), n().unwrap_or(0.0));
    matches!(kind, "move" | "click" | "dbl" | "right").then_some((kind, Point::new(x, y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_fill_follows_the_cache() {
        assert_eq!(open_fill(false, 99.0), 0.0);
        assert!((open_fill(true, 0.0) - 0.3).abs() < 1e-6);
        assert!(open_fill(true, 3.0) > open_fill(true, 1.0));
        assert_eq!(open_fill(true, 600.0), 1.0);
        assert_eq!(open_fill(true, -4.0), open_fill(true, 0.0));
    }

    #[test]
    fn stall_fill_is_a_clamped_percentage() {
        assert_eq!(stall_fill(None), 0.0);
        assert_eq!(stall_fill(Some(42.0)), 0.42);
        assert_eq!(stall_fill(Some(250.0)), 1.0);
        assert_eq!(stall_fill(Some(-3.0)), 0.0);
    }

    #[test]
    fn overlay_leaves_only_when_playing_or_asking() {
        assert!(!started(false, true, false, false));
        assert!(!started(true, false, false, false));
        assert!(
            !started(true, true, true, false),
            "still waiting on the cache"
        );
        assert!(started(true, true, false, false));
        assert!(
            started(true, false, true, true),
            "a resume prompt must stay visible"
        );
    }

    #[test]
    fn short_stalls_do_not_flash_the_overlay() {
        assert!(!stall_visible(None));
        assert!(!stall_visible(Some(Duration::from_millis(100))));
        assert!(stall_visible(Some(Duration::from_millis(400))));
    }

    #[test]
    fn list_navigation_stops_at_the_ends() {
        assert_eq!(list_nav("up", 0, 5), 0);
        assert_eq!(list_nav("down", 4, 5), 4);
        assert_eq!(list_nav("down", 1, 5), 2);
        assert_eq!(list_nav("x", 3, 5), 3);
        assert_eq!(list_nav("down", 0, 0), 0);
    }

    #[test]
    fn menus_and_tips_stay_inside_the_window() {
        let win = Size::new(800.0, 600.0);
        let p = menu_origin(Point::new(790.0, 590.0), win, 220.0, 300.0);
        assert!(p.x + 220.0 <= 800.0 && p.y + 300.0 <= 600.0);
        assert_eq!(
            menu_origin(Point::new(10.0, 20.0), win, 220.0, 300.0),
            Point::new(10.0, 20.0)
        );
        assert_eq!(tip_left(0.0, 200.0, 800.0), 8.0);
        assert_eq!(tip_left(800.0, 200.0, 800.0), 592.0);
        assert_eq!(tip_left(400.0, 200.0, 800.0), 300.0);
    }

    #[test]
    fn video_rect_leaves_room_for_the_bar_and_hides_under_overlays() {
        let hole = Rectangle::new(Point::new(0.0, 0.0), Size::new(1280.0, 800.0));
        assert_eq!(video_rect(hole, 96.0, false, false), Some(hole));
        assert_eq!(video_rect(hole, 96.0, true, false).unwrap().height, 704.0);
        assert_eq!(video_rect(hole, 96.0, false, true), None);
        let tiny = Rectangle::new(Point::ORIGIN, Size::new(1280.0, 50.0));
        assert_eq!(video_rect(tiny, 96.0, true, false), None);
    }

    #[test]
    fn forwarded_pointer_events_parse() {
        assert_eq!(
            parse_input("click 12 34"),
            Some(("click", Point::new(12.0, 34.0)))
        );
        assert_eq!(parse_input("move"), Some(("move", Point::ORIGIN)));
        assert_eq!(parse_input("key a"), None);
        assert_eq!(parse_input(""), None);
    }
}
