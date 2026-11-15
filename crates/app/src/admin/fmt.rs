//! Small pure formatters for admin screens.
use std::time::{SystemTime, UNIX_EPOCH};

/// Epoch seconds of an ISO-8601 UTC stamp ("2026-11-15T10:11:12.5Z"); sub-seconds/zone ignored.
pub fn parse_iso(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    // Days since 1970-01-01 (Howard Hinnant's civil-date algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + hh * 3600 + mm * 60 + ss)
}

pub fn ago(then: i64, now: i64) -> String {
    match now - then {
        s if s < 45 => "just now".into(),
        s if s < 3600 => format!("{}m ago", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// "5m ago" for an ISO stamp, "never" when missing or unparsable.
pub fn when(iso: Option<&str>) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    iso.and_then(parse_iso)
        .map_or("never".into(), |t| ago(t, now))
}

pub fn bytes(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1 << 20 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

pub fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_and_ago() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2000-03-01T00:00:01.5Z"), Some(951_868_801));
        assert_eq!(parse_iso("junk"), None);
        assert_eq!(ago(0, 10), "just now");
        assert_eq!(ago(0, 300), "5m ago");
        assert_eq!(ago(0, 7300), "2h ago");
        assert_eq!(ago(0, 3 * 86_400 + 5), "3d ago");
        assert_eq!(when(None), "never");
        assert_eq!(bytes(1536), "1.5 KB");
    }
}
