//! Cell text shared by Home, browse and search: captions, badges and the source mark.
use nocturne_core::catalog::Catalog;
use nocturne_core::customize::EpisodeFormat;
use nocturne_core::source::{Kind, Media};

/// Mono line under a title: kind (or episode / artist) and the year / runtime.
pub fn caption(m: &Media) -> String {
    let second = match m.kind {
        Kind::Episode => m.episode_label(),
        Kind::MusicAlbum | Kind::Audio => m.sub.clone().unwrap_or_default(),
        _ => m.kind_label(),
    };
    [second, m.meta_line()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// The line under the show on a "Coming this week" card: when it airs, then "S1:E2".
pub fn coming_line(m: &Media) -> String {
    let when = m.premiere.as_deref().and_then(|p| {
        nocturne_core::calendar::when(
            p,
            nocturne_core::calendar::today(),
            &nocturne_core::calendar::local_offset,
        )
    });
    [when.unwrap_or_default(), m.short_ep()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// The line under an episode's series title on a landscape card: "S1:E2 · Name" as built in,
/// "S1:E2 - Name" (`short`) or "Season 1, Episode 2 - Name" (`long`) when a theme asks.
pub fn episode_line(m: &Media, fmt: Option<EpisodeFormat>) -> String {
    let (code, sep) = match (fmt, m.season_number, m.number) {
        (Some(EpisodeFormat::Long), Some(s), Some(e)) => {
            (format!("Season {s}, Episode {e}"), " - ")
        }
        (Some(EpisodeFormat::Long), None, Some(e)) => (format!("Episode {e}"), " - "),
        (Some(_), ..) => (m.short_ep(), " - "),
        (None, ..) => (m.short_ep(), " \u{b7} "),
    };
    [code, m.title.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(sep)
}

/// Unwatched count badge for shows and seasons.
pub fn badge(m: &Media) -> Option<String> {
    m.unplayed.filter(|n| *n > 0).map(|n| n.to_string())
}

/// Where an item lives: "2 SOURCES" when several have a copy, else its own source's mark text.
pub fn mark_for(catalog: &Catalog, m: &Media, copies: usize) -> Option<String> {
    if copies > 1 {
        return Some(format!("{copies} SOURCES"));
    }
    catalog.slot(m.origin).map(crate::widget::source::mark_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captions_and_marks() {
        let mut m = Media::from_item(&nocturne_jellyfin::Item::default());
        m.kind = Kind::Movie;
        m.year = Some(2022);
        assert_eq!(caption(&m), "movie \u{b7} 2022");
        assert_eq!(badge(&m), None);
        m.kind = Kind::Episode;
        (m.season_number, m.number, m.title) = (Some(1), Some(2), "Pilot".into());
        assert_eq!(episode_line(&m, None), "S1:E2 \u{b7} Pilot");
        assert_eq!(
            episode_line(&m, Some(EpisodeFormat::Short)),
            "S1:E2 - Pilot"
        );
        assert_eq!(
            episode_line(&m, Some(EpisodeFormat::Long)),
            "Season 1, Episode 2 - Pilot"
        );
        m.kind = Kind::Movie;
        m.unplayed = Some(3);
        assert_eq!(badge(&m).as_deref(), Some("3"));
        assert_eq!(
            mark_for(&Catalog::default(), &m, 2).as_deref(),
            Some("2 SOURCES")
        );
        assert_eq!(mark_for(&Catalog::default(), &m, 1), None);
    }
}
