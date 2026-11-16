//! Cell text shared by Home, browse and search: captions, badges and the source mark.
use nocturne_core::catalog::Catalog;
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
        m.unplayed = Some(3);
        assert_eq!(badge(&m).as_deref(), Some("3"));
        assert_eq!(
            mark_for(&Catalog::default(), &m, 2).as_deref(),
            Some("2 SOURCES")
        );
        assert_eq!(mark_for(&Catalog::default(), &m, 1), None);
    }
}
