//! Cross-source ("unified") views: which kinds exist, the header's link model, the progressive
//! full load of a merged list and its client-side ordering. Pure: no UI, no I/O.
use crate::libview::{Scope, SortKey};
use crate::merge::{Entry, Preferred, SeriesIds, merge};
use crate::source::{Media, SourceId};
use nocturne_jellyfin::BrowseQuery;
use nocturne_local::fold;

/// Items per request while a merged list loads (bigger than a browse page: fewer round trips).
pub const MERGED_PAGE: usize = 300;
/// Most items one source contributes to a merged list.
// ponytail: merged lists load fully, up to this cap per source; a bigger library needs a paged merge.
pub const MERGED_CAP: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UKind {
    Movies,
    Shows,
    Music,
    Photos,
}

impl UKind {
    pub const ALL: [UKind; 4] = [Self::Movies, Self::Shows, Self::Music, Self::Photos];

    pub fn label(self) -> &'static str {
        match self {
            Self::Movies => "Movies",
            Self::Shows => "Shows",
            Self::Music => "Music",
            Self::Photos => "Photos",
        }
    }

    pub fn collection(self) -> &'static str {
        match self {
            Self::Movies => "movies",
            Self::Shows => "tvshows",
            Self::Music => "music",
            Self::Photos => "photos",
        }
    }

    /// Everything of this kind in a source (not one library of it).
    pub fn scope(self) -> Scope {
        let t = match self {
            Self::Movies => "Movie",
            Self::Shows => "Series",
            Self::Music => "MusicAlbum",
            Self::Photos => "Photo",
        };
        Scope {
            key: format!("all-{}", self.collection()),
            title: self.label().into(),
            subtitle: "Everything of this kind, from every source.".into(),
            collection: Some(self.collection().into()),
            base: BrowseQuery {
                include_types: vec![t.into()],
                recursive: true,
                ..Default::default()
            },
        }
    }
}

/// Unified kinds that at least one library offers (a mixed local folder offers movies and shows).
pub fn kinds(libs: &[Media], is_local: impl Fn(SourceId) -> bool) -> Vec<UKind> {
    UKind::ALL
        .into_iter()
        .filter(|k| {
            libs.iter().any(|l| match l.collection.as_deref() {
                Some("homevideos") => *k == UKind::Photos,
                Some(c) => c == k.collection(),
                None => is_local(l.origin) && matches!(k, UKind::Movies | UKind::Shows),
            })
        })
        .collect()
}

/// One entry of the header's link row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Home,
    Kind(UKind),
    /// Index into the page's flat library list (a single source's libraries, inline).
    Lib(usize),
    /// The menu of every source's libraries (several sources only).
    Libraries,
    Global(usize),
    /// The smart collections menu (added by the UI, never by [`links`]).
    Smart,
    /// The Calendar page (added by the UI, never by [`links`]).
    Calendar,
    Admin,
    /// The Seerr Requests page (added by the UI, after the other links, only when Seerr is signed in).
    Requests,
}

/// The header links. One source: Home, its libraries, the globals (as always). Several: Home, the
/// unified kinds, one Libraries menu, the globals.
pub fn links(
    libs: &[Media],
    several: bool,
    is_local: impl Fn(SourceId) -> bool,
    globals: usize,
    admin: bool,
) -> Vec<Link> {
    let mut v = vec![Link::Home];
    if several {
        v.extend(kinds(libs, is_local).into_iter().map(Link::Kind));
        if !libs.is_empty() {
            v.push(Link::Libraries);
        }
    } else {
        v.extend((0..libs.len()).map(Link::Lib));
    }
    v.extend((0..globals).map(Link::Global));
    if admin {
        v.push(Link::Admin);
    }
    v
}

/// Library indexes grouped by source, in first-seen order (the libraries menu's sections).
pub fn group_libraries(libs: &[Media]) -> Vec<(SourceId, Vec<usize>)> {
    let mut out: Vec<(SourceId, Vec<usize>)> = vec![];
    for (i, l) in libs.iter().enumerate() {
        match out.iter_mut().find(|(o, _)| *o == l.origin) {
            Some((_, v)) => v.push(i),
            None => out.push((l.origin, vec![i])),
        }
    }
    out
}

/// Merge order the user asked for. Names, dates, scores and runtimes compare across sources
/// (each source sorted only its own part); random is a stable shuffle.
pub fn sort_entries(v: &mut [Entry], key: SortKey, desc: bool) {
    let name = |e: &Entry| fold(&e.primary.title);
    match key {
        SortKey::Name => v.sort_by_cached_key(name),
        SortKey::DateAdded => v.sort_by_cached_key(|e| (e.primary.added.clone(), name(e))),
        SortKey::Premiere => v.sort_by_cached_key(|e| {
            (
                e.primary
                    .premiere
                    .clone()
                    .or_else(|| e.primary.year.map(|y| y.to_string())),
                name(e),
            )
        }),
        SortKey::Rating => {
            v.sort_by_cached_key(|e| (e.primary.rating.map_or(0, |r| (r * 100.0) as i64), name(e)))
        }
        SortKey::Runtime => v.sort_by_cached_key(|e| (e.primary.runtime_min.unwrap_or(0), name(e))),
        SortKey::Random => {
            return v.sort_by_cached_key(|e| {
                e.primary.id.bytes().fold(0x9e37_79b9_u64, |h, b| {
                    h.wrapping_mul(0x100_0000_01b3) ^ u64::from(b)
                })
            });
        }
    }
    if desc {
        v.reverse();
    }
}

#[derive(Clone, Debug)]
struct Part {
    origin: SourceId,
    items: Vec<Media>,
    /// Server-reported total of the query (0 until the first page answers).
    total: usize,
    done: bool,
    failed: bool,
}

/// A merged list loading page by page from every source at once. `accept` tells the caller which
/// page to ask next for that source; answers of an older load (`epoch`) are dropped.
#[derive(Clone, Debug, Default)]
pub struct Merged {
    pub epoch: u64,
    parts: Vec<Part>,
}

impl Merged {
    pub fn new(epoch: u64, sources: &[SourceId]) -> Self {
        Self {
            epoch,
            parts: sources
                .iter()
                .map(|o| Part {
                    origin: *o,
                    items: vec![],
                    total: 0,
                    done: false,
                    failed: false,
                })
                .collect(),
        }
    }

    /// One page arrived (`None` = the source failed). Returns the start index of the next page to
    /// request for that source, if there is more and the cap is not reached.
    pub fn accept(
        &mut self,
        epoch: u64,
        origin: SourceId,
        start: usize,
        r: Option<(Vec<Media>, usize)>,
    ) -> Option<usize> {
        if epoch != self.epoch {
            return None;
        }
        let p = self
            .parts
            .iter_mut()
            .find(|p| p.origin == origin && !p.done && p.items.len() == start)?;
        let Some((items, total)) = r else {
            (p.failed, p.done) = (true, true);
            return None;
        };
        p.total = total;
        let got = items.len();
        p.items.extend(items);
        if got == 0 || p.items.len() >= total.min(MERGED_CAP) {
            p.done = true;
            return None;
        }
        Some(p.items.len())
    }

    /// Optimistic edits (played / favourite) reach the loaded items.
    pub fn for_each_mut(&mut self, mut f: impl FnMut(&mut Media)) {
        self.parts
            .iter_mut()
            .flat_map(|p| p.items.iter_mut())
            .for_each(&mut f);
    }

    /// Some source still has pages to come.
    pub fn loading(&self) -> bool {
        self.parts.iter().any(|p| !p.done)
    }

    pub fn failed(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(|p| p.failed)
    }

    /// Items known so far / items the sources report in all.
    pub fn counts(&self) -> (usize, usize) {
        (
            self.parts.iter().map(|p| p.items.len()).sum(),
            self.parts.iter().map(|p| p.total).sum(),
        )
    }

    /// What arrived so far as one ordered list, duplicates collapsed.
    pub fn entries(
        &self,
        priority: &[SourceId],
        prefs: &Preferred,
        series: &SeriesIds,
        key: SortKey,
        desc: bool,
    ) -> Vec<Entry> {
        let mut v = merge(
            self.parts
                .iter()
                .map(|p| (p.origin, p.items.clone()))
                .collect(),
            priority,
            prefs,
            series,
        );
        sort_entries(&mut v, key, desc);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{Kind, ProviderIds};

    const A: SourceId = SourceId(1);
    const B: SourceId = SourceId(2);

    fn m(o: SourceId, id: &str, title: &str, imdb: Option<&str>) -> Media {
        Media {
            origin: o,
            id: id.into(),
            title: title.into(),
            kind: Kind::Movie,
            ids: ProviderIds {
                imdb: imdb.map(Into::into),
                ..Default::default()
            },
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        }
    }

    fn lib(o: SourceId, id: &str, coll: Option<&str>) -> Media {
        Media {
            origin: o,
            id: id.into(),
            title: id.into(),
            kind: Kind::Library,
            collection: coll.map(Into::into),
            ..Media::from_item(&nocturne_jellyfin::Item::default())
        }
    }

    #[test]
    fn kinds_follow_the_libraries_and_a_mixed_local_folder_offers_movies_and_shows() {
        let libs = [
            lib(A, "m", Some("movies")),
            lib(B, "mix", None),
            lib(A, "p", Some("homevideos")),
        ];
        let k = kinds(&libs, |o| o == B);
        assert_eq!(k, [UKind::Movies, UKind::Shows, UKind::Photos]);
        let only_music = [lib(A, "mu", Some("music"))];
        assert_eq!(kinds(&only_music, |_| false), [UKind::Music]);
        // A mixed folder on a server says nothing about its kinds.
        assert!(kinds(&[lib(A, "mix", None)], |_| false).is_empty());
    }

    #[test]
    fn one_source_lists_its_libraries_several_get_kinds_and_a_libraries_menu() {
        let libs = [lib(A, "m", Some("movies")), lib(B, "s", Some("tvshows"))];
        let one = links(&libs, false, |_| false, 3, true);
        assert_eq!(
            one,
            [
                Link::Home,
                Link::Lib(0),
                Link::Lib(1),
                Link::Global(0),
                Link::Global(1),
                Link::Global(2),
                Link::Admin
            ]
        );
        let many = links(&libs, true, |_| false, 3, false);
        assert_eq!(
            many,
            [
                Link::Home,
                Link::Kind(UKind::Movies),
                Link::Kind(UKind::Shows),
                Link::Libraries,
                Link::Global(0),
                Link::Global(1),
                Link::Global(2),
            ]
        );
        // No libraries yet: no empty menu.
        assert!(!links(&[], true, |_| false, 0, false).contains(&Link::Libraries));
    }

    #[test]
    fn the_libraries_menu_groups_by_source_in_first_seen_order() {
        let libs = [lib(B, "b1", None), lib(A, "a1", None), lib(B, "b2", None)];
        assert_eq!(group_libraries(&libs), [(B, vec![0, 2]), (A, vec![1])]);
    }

    #[test]
    fn a_merged_load_chains_pages_drops_stale_answers_and_collapses_duplicates() {
        let mut g = Merged::new(3, &[A, B]);
        assert!(g.loading());
        assert_eq!(g.accept(2, A, 0, Some((vec![], 9))), None, "older load");
        // A has 3 in all, B 1; A's first page holds 2.
        let a1 = vec![m(A, "a1", "Beta", Some("tt1")), m(A, "a2", "Alpha", None)];
        assert_eq!(g.accept(3, A, 0, Some((a1, 3))), Some(2));
        assert_eq!(
            g.accept(3, A, 0, Some((vec![], 3))),
            None,
            "a page that is not the next one"
        );
        let b1 = vec![m(B, "b1", "Beta copy", Some("tt1"))];
        assert_eq!(g.accept(3, B, 0, Some((b1, 1))), None);
        assert!(g.loading(), "A has one more page");
        assert_eq!(
            g.accept(3, A, 2, Some((vec![m(A, "a3", "Gamma", None)], 3))),
            None
        );
        assert!(!g.loading());
        assert_eq!(g.counts(), (4, 4));
        let e = g.entries(
            &[A, B],
            &Preferred::default(),
            &SeriesIds::default(),
            SortKey::Name,
            false,
        );
        let names: Vec<_> = e.iter().map(|e| e.primary.title.as_str()).collect();
        assert_eq!(names, ["Alpha", "Beta", "Gamma"], "sorted across sources");
        assert_eq!(
            e[1].alternates.len(),
            1,
            "the duplicate is one cell, two copies"
        );
    }

    #[test]
    fn a_failed_source_ends_its_part_only() {
        let mut g = Merged::new(1, &[A, B]);
        assert_eq!(g.accept(1, A, 0, None), None);
        assert!(!g.failed(), "B has not answered yet");
        assert_eq!(
            g.accept(1, B, 0, Some((vec![m(B, "b", "B", None)], 1))),
            None
        );
        assert!(!g.loading() && !g.failed());
        assert!(
            Merged::new(1, &[A]).accept(1, A, 0, None).is_none(),
            "failure is not a next page"
        );
    }

    #[test]
    fn ordering_by_other_keys_and_direction() {
        let mk = |t: &str, y: u32, r: f32| {
            let mut x = m(A, t, t, None);
            (x.year, x.rating) = (Some(y), Some(r));
            Entry {
                primary: x,
                alternates: vec![],
            }
        };
        let mut v = vec![mk("b", 2000, 9.0), mk("a", 2010, 5.0), mk("c", 1990, 7.0)];
        sort_entries(&mut v, SortKey::Premiere, true);
        assert_eq!(v[0].primary.title, "a");
        sort_entries(&mut v, SortKey::Rating, true);
        assert_eq!(v[0].primary.title, "b");
        sort_entries(&mut v, SortKey::Name, false);
        assert_eq!(v[0].primary.title, "a");
        let (mut s1, mut s2) = (v.clone(), v.clone());
        sort_entries(&mut s1, SortKey::Random, false);
        sort_entries(&mut s2, SortKey::Random, false);
        assert_eq!(
            s1.iter().map(|e| &e.primary.id).collect::<Vec<_>>(),
            s2.iter().map(|e| &e.primary.id).collect::<Vec<_>>(),
            "random is stable between reloads of the same items"
        );
    }
}
