//! The "Versions and sources" picker's data: every way to play one title, as one flat list. A copy
//! on a server may carry several versions (Jellyfin `MediaSources`: 4K and 1080p files of one
//! movie); duplicates found in other sources (see `merge`) add their own copies. The first entry is
//! the merge's primary and the server's default version. Pure: no I/O, no window.
use crate::backend::PlayTarget;
use crate::detail::InfoBlock;
use crate::merge::{Preferred, SeriesIds};
use crate::source::{Media, SourceId};
use std::path::Path;

/// One playable way to watch the title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub origin: SourceId,
    /// The item inside `origin`.
    pub item_id: String,
    /// Jellyfin media source id; `None` for a local file or a copy with a single version.
    pub source_id: Option<String>,
    /// The version's own name (file name), may be empty.
    pub name: String,
    /// "4K · HEVC · MKV", only what is known.
    pub detail: String,
    /// False for a local copy whose file or folder is gone.
    pub available: bool,
}

/// What a choice is made of: stable across the list being rebuilt (alternates' details arrive late).
pub type Pick = (SourceId, String, Option<String>);

impl Version {
    pub fn pick(&self) -> Pick {
        (self.origin, self.item_id.clone(), self.source_id.clone())
    }
}

fn describe(b: &InfoBlock) -> String {
    let mut v = [&b.quality, &b.codec, &b.container, &b.size, &b.bitrate]
        .into_iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    v.dedup();
    v.join(" \u{b7} ")
}

/// `copies` is the primary first, then the alternates, each with what is known of its files
/// (`Full::info`; empty for a local copy or one not loaded yet). A copy with several files becomes
/// several versions, each naming its media source; with one or none it is a single version.
pub fn build(copies: &[(Media, Vec<InfoBlock>)]) -> Vec<Version> {
    let mut out = vec![];
    for (m, info) in copies {
        if info.len() > 1 {
            out.extend(info.iter().map(|b| Version {
                origin: m.origin,
                item_id: m.id.clone(),
                source_id: Some(b.id.clone()).filter(|s| !s.is_empty()),
                name: b.name.clone(),
                detail: describe(b),
                available: m.available,
            }));
        } else {
            let b = info.first();
            out.push(Version {
                origin: m.origin,
                item_id: m.id.clone(),
                source_id: None,
                name: b.map(|b| b.name.clone()).unwrap_or_default(),
                detail: b.map(describe).unwrap_or_default(),
                available: m.available,
            });
        }
    }
    out
}

/// Index of `pick` in `list`, else the first playable entry, else 0.
pub fn selected(list: &[Version], pick: Option<&Pick>) -> usize {
    pick.and_then(|p| list.iter().position(|v| &v.pick() == p))
        .or_else(|| list.iter().position(|v| v.available))
        .unwrap_or(0)
}

/// The picker only shows when there is more than one way to play.
pub fn worth_showing(list: &[Version]) -> bool {
    list.len() > 1
}

/// Remember `m`'s source as the one to show first for this title (what "Always prefer this source"
/// does) and save the list to `path`. False when `m` has no provider ids: there is no identity to
/// key the choice on.
pub fn prefer(prefs: &mut Preferred, series: &SeriesIds, m: &Media, path: Option<&Path>) -> bool {
    prefs.choose(m, series);
    if prefs.get(m, series) != Some(m.origin) {
        return false;
    }
    if let Some(p) = path {
        let _ = prefs.save_to(p);
    }
    true
}

/// A stream URL for one version (`MediaSourceId=`); a file is its own version.
pub fn with_source(t: PlayTarget, id: &str) -> PlayTarget {
    match t {
        PlayTarget::Url { url, headers } => {
            let enc: String = id
                .bytes()
                .map(|b| match b {
                    b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect();
            let sep = if url.contains('?') { '&' } else { '?' };
            PlayTarget::Url {
                url: format!("{url}{sep}MediaSourceId={enc}"),
                headers,
            }
        }
        f => f,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ProviderIds;

    fn media(origin: u32, id: &str, available: bool) -> Media {
        let mut m = Media::from_item_in(
            SourceId(origin),
            &nocturne_jellyfin::Item {
                id: id.into(),
                name: "Film".into(),
                kind: "Movie".into(),
                ..Default::default()
            },
        );
        m.available = available;
        m.ids = ProviderIds {
            imdb: Some("tt1".into()),
            ..Default::default()
        };
        m
    }

    fn block(id: &str, name: &str, quality: &str) -> InfoBlock {
        InfoBlock {
            id: id.into(),
            name: name.into(),
            quality: quality.into(),
            codec: "HEVC".into(),
            container: "MKV".into(),
            ..Default::default()
        }
    }

    #[test]
    fn server_versions_each_name_their_media_source() {
        let l = build(&[(
            media(1, "a", true),
            vec![
                block("s1", "Film - 4K", "4K"),
                block("s2", "Film - 1080p", "1080p"),
            ],
        )]);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].source_id.as_deref(), Some("s1"));
        assert_eq!(l[1].detail, "1080p \u{b7} HEVC \u{b7} MKV");
        assert_eq!(
            selected(&l, None),
            0,
            "default = the server's first version"
        );
        assert!(worth_showing(&l));
    }

    #[test]
    fn single_copy_needs_no_picker_and_alternates_join_the_list() {
        let one = build(&[(media(1, "a", true), vec![block("s1", "", "1080p")])]);
        assert!(!worth_showing(&one));
        assert_eq!(one[0].source_id, None);
        let two = build(&[
            (media(1, "a", true), vec![block("s1", "", "4K")]),
            (media(2, "b", true), vec![]),
        ]);
        assert!(worth_showing(&two));
        assert_eq!((two[1].origin, two[1].detail.as_str()), (SourceId(2), ""));
    }

    #[test]
    fn selection_survives_a_rebuild_and_skips_unavailable_by_default() {
        let before = build(&[(media(1, "a", true), vec![]), (media(2, "b", true), vec![])]);
        let pick = before[1].pick();
        let after = build(&[
            (
                media(1, "a", true),
                vec![block("x", "", "4K"), block("y", "", "720p")],
            ),
            (media(2, "b", true), vec![]),
        ]);
        assert_eq!(selected(&after, Some(&pick)), 2, "the alternate moved down");
        let gone = build(&[
            (media(1, "a", false), vec![]),
            (media(2, "b", true), vec![]),
        ]);
        assert_eq!(
            selected(&gone, None),
            1,
            "a missing local copy is not the default"
        );
        assert_eq!(selected(&[], None), 0);
    }

    #[test]
    fn preferring_a_source_persists_and_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferred.json");
        let series = SeriesIds::default();
        let mut prefs = Preferred::default();
        let alt = media(2, "b", true);
        assert!(prefer(&mut prefs, &series, &alt, Some(&path)));
        let loaded = Preferred::load_from(&path);
        assert_eq!(loaded.get(&alt, &series), Some(SourceId(2)));
        // The same title seen through the first source resolves to the same choice.
        assert_eq!(loaded.get(&media(1, "a", true), &series), Some(SourceId(2)));
        // A title with no ids has nothing to key on.
        let mut bare = media(2, "c", true);
        bare.ids = ProviderIds::default();
        assert!(!prefer(&mut Preferred::default(), &series, &bare, None));
    }

    #[test]
    fn stream_url_names_the_version() {
        let t = PlayTarget::Url {
            url: "http://h/Videos/x/stream?static=true".into(),
            headers: vec![],
        };
        let PlayTarget::Url { url, .. } = with_source(t, "ab c") else {
            panic!()
        };
        assert!(url.ends_with("&MediaSourceId=ab%20c"));
        let f = PlayTarget::File("/x".into());
        assert_eq!(with_source(f.clone(), "s"), f);
    }
}
