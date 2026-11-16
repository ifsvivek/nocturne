//! Library browsing state without the window: sort, filters, tabs, A-Z, paging, and the
//! translation to Jellyfin query parameters. Everything here is pure and unit-tested.
use crate::source::Media;
use nocturne_jellyfin::{BrowseQuery, FilterOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Range;

/// Items fetched per request.
pub const PAGE: usize = 96;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    DateAdded,
    Premiere,
    Rating,
    Runtime,
    Random,
}

impl SortKey {
    pub const ALL: [SortKey; 6] = [
        Self::Name,
        Self::DateAdded,
        Self::Premiere,
        Self::Rating,
        Self::Runtime,
        Self::Random,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::DateAdded => "Date added",
            Self::Premiere => "Premiere date",
            Self::Rating => "Community rating",
            Self::Runtime => "Runtime",
            Self::Random => "Random",
        }
    }

    /// Jellyfin `sortBy` values (name is the tie-breaker everywhere).
    pub fn params(self) -> Vec<String> {
        let p: &[&str] = match self {
            Self::Name => &["SortName"],
            Self::DateAdded => &["DateCreated", "SortName"],
            Self::Premiere => &["PremiereDate", "SortName"],
            Self::Rating => &["CommunityRating", "SortName"],
            Self::Runtime => &["Runtime", "SortName"],
            Self::Random => &["Random"],
        };
        p.iter().map(|s| s.to_string()).collect()
    }

    /// Newest/highest first is what people expect for everything except names and runtime.
    pub fn default_desc(self) -> bool {
        matches!(self, Self::DateAdded | Self::Premiere | Self::Rating)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    #[default]
    Poster,
    Landscape,
    List,
}

impl ViewKind {
    pub const ALL: [ViewKind; 3] = [Self::Poster, Self::Landscape, Self::List];

    pub fn label(self) -> &'static str {
        match self {
            Self::Poster => "Poster",
            Self::Landscape => "Thumb",
            Self::List => "List",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum Played {
    #[default]
    Any,
    Played,
    Unplayed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Res {
    Uhd4k,
    Hd,
}

/// Filters. The flag ones are remembered per library; value lists are chosen per visit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filters {
    pub played: Played,
    pub favourite: bool,
    pub subtitles: bool,
    pub trailer: bool,
    pub res: Option<Res>,
    #[serde(skip)]
    pub genres: Vec<String>,
    #[serde(skip)]
    pub years: Vec<u32>,
    #[serde(skip)]
    pub ratings: Vec<String>,
    #[serde(skip)]
    pub tags: Vec<String>,
    #[serde(skip)]
    pub studios: Vec<String>,
}

impl Filters {
    pub fn apply(&self, q: &mut BrowseQuery) {
        let mut on = |f: &str| {
            if !q.filters.iter().any(|x| x == f) {
                q.filters.push(f.into());
            }
        };
        match self.played {
            Played::Any => {}
            Played::Played => on("IsPlayed"),
            Played::Unplayed => on("IsUnplayed"),
        }
        if self.favourite {
            on("IsFavorite");
        }
        q.has_subtitles |= self.subtitles;
        q.has_trailer |= self.trailer;
        q.is_4k |= self.res == Some(Res::Uhd4k);
        q.is_hd |= self.res == Some(Res::Hd);
        q.genres.extend(self.genres.iter().cloned());
        q.years.extend(&self.years);
        q.official_ratings.extend(self.ratings.iter().cloned());
        q.tags.extend(self.tags.iter().cloned());
        q.studio_ids.extend(self.studios.iter().cloned());
    }

    /// How many filters are on (shown on the Filters button).
    pub fn active(&self) -> usize {
        (self.played != Played::Any) as usize
            + self.favourite as usize
            + self.subtitles as usize
            + self.trailer as usize
            + self.res.is_some() as usize
            + self.genres.len()
            + self.years.len().min(1)
            + self.ratings.len()
            + self.tags.len()
            + self.studios.len()
    }
}

/// What is remembered per library in settings.toml.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibPrefs {
    pub sort: SortKey,
    pub desc: bool,
    pub view: ViewKind,
    pub filters: Filters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Library,
    Favourites,
    Recent,
    Random,
    Collections,
    Artists,
    Genres,
    Studios,
    Tags,
    Years,
}

impl Tab {
    pub fn label(self, collection: Option<&str>) -> &'static str {
        match self {
            Self::Library if collection == Some("music") => "Albums",
            Self::Library => "Library",
            Self::Favourites => "Favourites",
            Self::Recent => "Recently played",
            Self::Random => "Random",
            Self::Collections => "Collections",
            Self::Artists => "Artists",
            Self::Genres => "Genres",
            Self::Studios => "Studios",
            Self::Tags => "Tags",
            Self::Years => "Years",
        }
    }

    /// Tabs that list names (picking one filters the library) instead of items.
    pub fn is_names(self) -> bool {
        matches!(
            self,
            Self::Genres | Self::Studios | Self::Tags | Self::Years
        )
    }
}

/// Tabs a library offers, by its collection type.
pub fn tabs_for(collection: Option<&str>) -> Vec<Tab> {
    use Tab::*;
    match collection {
        Some("movies") => vec![
            Library,
            Favourites,
            Collections,
            Genres,
            Studios,
            Years,
            Tags,
            Recent,
            Random,
        ],
        Some("tvshows") => vec![Library, Favourites, Genres, Studios, Years, Recent, Random],
        Some("music") => vec![Library, Artists, Favourites, Genres],
        _ => vec![Library, Favourites],
    }
}

/// Item types a library's main tab lists; `None` = the folder's direct children.
pub fn types_for(collection: Option<&str>) -> Option<Vec<&'static str>> {
    Some(match collection {
        Some("movies") => vec!["Movie"],
        Some("tvshows") => vec!["Series"],
        Some("music") => vec!["MusicAlbum"],
        Some("musicvideos") => vec!["MusicVideo"],
        Some("boxsets") => vec!["BoxSet"],
        Some("playlists") => vec!["Playlist"],
        Some("books") => vec!["Book", "AudioBook"],
        Some("homevideos" | "photos") => return None,
        _ => vec!["Movie", "Series"],
    })
}

/// The `#` bucket plus A-Z.
pub fn letters() -> Vec<char> {
    std::iter::once('#').chain('A'..='Z').collect()
}

/// `(nameStartsWith, nameLessThan)` for a jump-bar letter; `#` is everything before "A".
pub fn letter_params(c: char) -> (Option<String>, Option<String>) {
    if c == '#' {
        (None, Some("A".into()))
    } else {
        (Some(c.to_string()), None)
    }
}

/// Next/previous letter; from "no letter" forward lands on `#`, back on `Z`.
pub fn step_letter(cur: Option<char>, dir: i32) -> Option<char> {
    let l = letters();
    let i = cur.and_then(|c| l.iter().position(|x| *x == c));
    match (i, dir > 0) {
        (None, true) => l.first().copied(),
        (None, false) => l.last().copied(),
        (Some(i), true) => l.get(i + 1).copied(),
        (Some(i), false) => i.checked_sub(1).and_then(|j| l.get(j).copied()),
    }
}

/// Years grouped into decades, newest decade first: `(2010, [2010, 2013, ...])`.
pub fn decades(years: &[u32]) -> Vec<(u32, Vec<u32>)> {
    let mut out: Vec<(u32, Vec<u32>)> = vec![];
    let mut ys = years.to_vec();
    ys.sort_unstable_by(|a, b| b.cmp(a));
    ys.dedup();
    for y in ys {
        let d = y / 10 * 10;
        match out.last_mut() {
            Some((k, v)) if *k == d => v.push(y),
            _ => out.push((d, vec![y])),
        }
    }
    out
}

/// Page index holding item `i`.
pub fn page_of(i: usize) -> usize {
    i / PAGE
}

/// Pages that must be fetched to fill `visible` (item indexes), skipping loaded and in-flight ones.
pub fn missing_pages(
    slots: &[Option<Media>],
    pending: &HashSet<usize>,
    visible: Range<usize>,
) -> Vec<usize> {
    let mut v: Vec<usize> = vec![];
    let end = visible.end.min(slots.len());
    for (i, slot) in slots.iter().enumerate().take(end).skip(visible.start) {
        let p = page_of(i);
        if slot.is_none() && !pending.contains(&p) && !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

/// Put a fetched page into the slots (ignores anything past the known total).
pub fn fill(slots: &mut [Option<Media>], start: usize, items: Vec<Media>) {
    for (k, m) in items.into_iter().enumerate() {
        if let Some(s) = slots.get_mut(start + k) {
            *s = Some(m);
        }
    }
}

/// Rows needed for `n` items in `cols` columns.
pub fn rows(n: usize, cols: usize) -> usize {
    n.div_ceil(cols.max(1))
}

/// "1,234" for mono counters.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What a browse page lists before the toolbar's choices are applied.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    /// Settings key for remembered sort/filters/view (opaque id or a fixed name).
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub collection: Option<String>,
    pub base: BrowseQuery,
}

/// One entry of a Genres/Studios/Tags/Years tab.
#[derive(Clone, Debug, PartialEq)]
pub struct NameEntry {
    pub label: String,
    pub pick: Pick,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Genre(String),
    Studio(String),
    Tag(String),
    Years(Vec<u32>),
}

impl Pick {
    pub fn apply(&self, f: &mut Filters) {
        match self {
            Self::Genre(g) => f.genres = vec![g.clone()],
            Self::Studio(id) => f.studios = vec![id.clone()],
            Self::Tag(t) => f.tags = vec![t.clone()],
            Self::Years(y) => f.years = y.clone(),
        }
    }
}

/// Entries for a name tab from the server's filter options (studios come separately).
pub fn name_entries(tab: Tab, o: &FilterOptions, studios: &[(String, String)]) -> Vec<NameEntry> {
    let e = |label: &str, pick| NameEntry {
        label: label.to_string(),
        pick,
    };
    match tab {
        Tab::Genres => o
            .genres
            .iter()
            .map(|g| e(g, Pick::Genre(g.clone())))
            .collect(),
        Tab::Tags => o.tags.iter().map(|g| e(g, Pick::Tag(g.clone()))).collect(),
        Tab::Studios => studios
            .iter()
            .map(|(id, n)| e(n, Pick::Studio(id.clone())))
            .collect(),
        Tab::Years => decades(&o.years)
            .into_iter()
            .map(|(d, ys)| e(&format!("{d}s"), Pick::Years(ys)))
            .collect(),
        _ => vec![],
    }
}

/// One toggle in the Filters panel.
#[derive(Clone, Debug, PartialEq)]
pub enum FAct {
    Clear,
    Played(Played),
    Favourite,
    Subtitles,
    Trailer,
    Res(Res),
    Genre(String),
    Years(Vec<u32>),
    Rating(String),
}

/// A panel row: `act == None` is a section heading (not selectable).
#[derive(Clone, Debug, PartialEq)]
pub struct FRow {
    pub label: String,
    pub on: bool,
    pub act: Option<FAct>,
}

/// Rows of the Filters panel for the current filters and the server's options.
pub fn filter_rows(f: &Filters, o: Option<&FilterOptions>) -> Vec<FRow> {
    let head = |l: &str| FRow {
        label: l.to_string(),
        on: false,
        act: None,
    };
    let row = |l: &str, on, act| FRow {
        label: l.to_string(),
        on,
        act: Some(act),
    };
    let mut v = vec![
        row("Clear all filters", false, FAct::Clear),
        head("Status"),
        row(
            "Played",
            f.played == Played::Played,
            FAct::Played(Played::Played),
        ),
        row(
            "Unplayed",
            f.played == Played::Unplayed,
            FAct::Played(Played::Unplayed),
        ),
        row("Favourites", f.favourite, FAct::Favourite),
        head("Features"),
        row("Has subtitles", f.subtitles, FAct::Subtitles),
        row("Has trailer", f.trailer, FAct::Trailer),
        row("4K", f.res == Some(Res::Uhd4k), FAct::Res(Res::Uhd4k)),
        row("HD", f.res == Some(Res::Hd), FAct::Res(Res::Hd)),
    ];
    let Some(o) = o else { return v };
    if !o.genres.is_empty() {
        v.push(head("Genres"));
        v.extend(
            o.genres
                .iter()
                .map(|g| row(g, f.genres.contains(g), FAct::Genre(g.clone()))),
        );
    }
    if !o.official_ratings.is_empty() {
        v.push(head("Rating"));
        v.extend(
            o.official_ratings
                .iter()
                .map(|r| row(r, f.ratings.contains(r), FAct::Rating(r.clone()))),
        );
    }
    if !o.years.is_empty() {
        v.push(head("Decade"));
        v.extend(decades(&o.years).into_iter().map(|(d, ys)| {
            let on = ys.iter().all(|y| f.years.contains(y));
            row(&format!("{d}s"), on, FAct::Years(ys))
        }));
    }
    v
}

fn flip<T: PartialEq + Clone>(v: &mut Vec<T>, x: &T) {
    match v.iter().position(|y| y == x) {
        Some(i) => {
            v.remove(i);
        }
        None => v.push(x.clone()),
    }
}

/// Apply one panel toggle. Genres, ratings and decades are multi-select; played and
/// resolution are single-choice (picking the active one again clears it).
pub fn toggle(f: &mut Filters, a: &FAct) {
    match a {
        FAct::Clear => *f = Filters::default(),
        FAct::Played(p) => f.played = if f.played == *p { Played::Any } else { *p },
        FAct::Favourite => f.favourite ^= true,
        FAct::Subtitles => f.subtitles ^= true,
        FAct::Trailer => f.trailer ^= true,
        FAct::Res(r) => f.res = if f.res == Some(*r) { None } else { Some(*r) },
        FAct::Genre(g) => flip(&mut f.genres, g),
        FAct::Rating(r) => flip(&mut f.ratings, r),
        FAct::Years(ys) => {
            if ys.iter().all(|y| f.years.contains(y)) {
                f.years.retain(|y| !ys.contains(y));
            } else {
                for y in ys {
                    if !f.years.contains(y) {
                        f.years.push(*y);
                    }
                }
            }
        }
    }
}

/// The full query for one page of a browse view.
pub fn build_query(
    scope: &Scope,
    tab: Tab,
    prefs: &LibPrefs,
    letter: Option<char>,
    start: usize,
) -> BrowseQuery {
    let mut q = scope.base.clone();
    match tab {
        Tab::Collections => q.include_types = vec!["BoxSet".into()],
        Tab::Artists => q.include_types = vec!["MusicArtist".into()],
        _ => {}
    }
    let (key, desc) = match tab {
        Tab::Random => (SortKey::Random.params(), false),
        Tab::Recent => (vec!["DatePlayed".to_string()], true),
        _ => (prefs.sort.params(), prefs.desc),
    };
    q.sort_by = key;
    q.descending = desc;
    prefs.filters.apply(&mut q);
    match tab {
        Tab::Favourites if !q.filters.iter().any(|f| f == "IsFavorite") => {
            q.filters.push("IsFavorite".into())
        }
        Tab::Recent if !q.filters.iter().any(|f| f == "IsPlayed") => {
            q.filters.push("IsPlayed".into())
        }
        _ => {}
    }
    if let Some(c) = letter {
        (q.name_starts_with, q.name_less_than) = letter_params(c);
    }
    q.start_index = Some(start as u32);
    q.limit = Some(PAGE as u32);
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_jellyfin as jf;

    fn lib() -> Scope {
        Scope {
            key: "L".into(),
            collection: Some("movies".into()),
            base: BrowseQuery {
                parent_id: Some("lib".into()),
                include_types: vec!["Movie".into()],
                recursive: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn kv(q: &BrowseQuery) -> Vec<String> {
        q.pairs("u")
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    #[test]
    fn default_name_sort_first_page() {
        let q = build_query(&lib(), Tab::Library, &LibPrefs::default(), None, 0);
        let v = kv(&q);
        assert!(v.contains(&"sortBy=SortName".into()));
        assert!(v.contains(&"sortOrder=Ascending".into()));
        assert!(v.contains(&"startIndex=0".into()) && v.contains(&format!("limit={PAGE}")));
        assert!(
            v.contains(&"includeItemTypes=Movie".into()) && v.contains(&"recursive=true".into())
        );
    }

    #[test]
    fn every_sort_key_maps_and_directions() {
        let want = [
            (SortKey::Name, "SortName", false),
            (SortKey::DateAdded, "DateCreated,SortName", true),
            (SortKey::Premiere, "PremiereDate,SortName", true),
            (SortKey::Rating, "CommunityRating,SortName", true),
            (SortKey::Runtime, "Runtime,SortName", false),
            (SortKey::Random, "Random", false),
        ];
        for (k, by, desc) in want {
            assert_eq!(k.params().join(","), by);
            assert_eq!(k.default_desc(), desc, "{k:?}");
        }
        let p = LibPrefs {
            sort: SortKey::Rating,
            desc: true,
            ..Default::default()
        };
        let v = kv(&build_query(&lib(), Tab::Library, &p, None, 96));
        assert!(v.contains(&"sortBy=CommunityRating,SortName".into()));
        assert!(v.contains(&"sortOrder=Descending".into()));
        assert!(v.contains(&"startIndex=96".into()));
    }

    #[test]
    fn filter_combination_exact_parameters() {
        let p = LibPrefs {
            filters: Filters {
                played: Played::Unplayed,
                favourite: true,
                subtitles: true,
                trailer: true,
                res: Some(Res::Uhd4k),
                genres: vec!["Drama".into(), "Sci-Fi".into()],
                years: vec![2011, 2012],
                ratings: vec!["R".into()],
                studios: vec!["st1".into()],
                tags: vec![],
            },
            ..Default::default()
        };
        let v = kv(&build_query(&lib(), Tab::Library, &p, None, 0));
        for want in [
            "filters=IsUnplayed,IsFavorite",
            "genres=Drama|Sci-Fi",
            "years=2011,2012",
            "officialRatings=R",
            "studioIds=st1",
            "hasSubtitles=true",
            "hasTrailer=true",
            "is4K=true",
        ] {
            assert!(v.contains(&want.to_string()), "{want}");
        }
        assert!(!v.iter().any(|s| s.starts_with("isHd")));
        assert_eq!(p.filters.active(), 10);
    }

    #[test]
    fn tabs_adjust_the_query() {
        let p = LibPrefs::default();
        let v = kv(&build_query(&lib(), Tab::Favourites, &p, None, 0));
        assert!(v.contains(&"filters=IsFavorite".into()));
        let v = kv(&build_query(&lib(), Tab::Recent, &p, None, 0));
        assert!(
            v.contains(&"sortBy=DatePlayed".into()) && v.contains(&"sortOrder=Descending".into())
        );
        assert!(v.contains(&"filters=IsPlayed".into()));
        let v = kv(&build_query(&lib(), Tab::Random, &p, None, 0));
        assert!(v.contains(&"sortBy=Random".into()));
        let v = kv(&build_query(&lib(), Tab::Collections, &p, None, 0));
        assert!(v.contains(&"includeItemTypes=BoxSet".into()));
        // Favourites with the favourite filter already on does not duplicate it.
        let mut f = p.clone();
        f.filters.favourite = true;
        let v = kv(&build_query(&lib(), Tab::Favourites, &f, None, 0));
        assert!(v.contains(&"filters=IsFavorite".into()));
    }

    #[test]
    fn az_mapping_and_stepping() {
        let l = letters();
        assert_eq!((l.len(), l[0], l[1], l[26]), (27, '#', 'A', 'Z'));
        assert_eq!(letter_params('#'), (None, Some("A".into())));
        assert_eq!(letter_params('Q'), (Some("Q".into()), None));
        let v = kv(&build_query(
            &lib(),
            Tab::Library,
            &LibPrefs::default(),
            Some('#'),
            0,
        ));
        assert!(
            v.contains(&"nameLessThan=A".into()) && !v.iter().any(|s| s.starts_with("nameStarts"))
        );
        let v = kv(&build_query(
            &lib(),
            Tab::Library,
            &LibPrefs::default(),
            Some('M'),
            0,
        ));
        assert!(v.contains(&"nameStartsWith=M".into()));
        assert_eq!(step_letter(None, 1), Some('#'));
        assert_eq!(step_letter(None, -1), Some('Z'));
        assert_eq!(step_letter(Some('A'), 1), Some('B'));
        assert_eq!(step_letter(Some('A'), -1), Some('#'));
        assert_eq!(step_letter(Some('#'), -1), None, "back from # clears");
        assert_eq!(step_letter(Some('Z'), 1), None, "forward from Z clears");
    }

    fn media(n: usize) -> Vec<Media> {
        (0..n)
            .map(|i| {
                Media::from_item(&jf::Item {
                    id: i.to_string(),
                    ..Default::default()
                })
            })
            .collect()
    }

    #[test]
    fn paging_math_and_slot_fill() {
        assert_eq!(
            (page_of(0), page_of(95), page_of(96), page_of(250)),
            (0, 0, 1, 2)
        );
        assert_eq!(rows(0, 5), 0);
        assert_eq!(rows(11, 5), 3);
        assert_eq!(rows(3, 0), 3, "zero columns acts as one");
        let mut slots: Vec<Option<Media>> = vec![None; 300];
        let mut pend = HashSet::new();
        // Visible items 90..110 straddle pages 0 and 1.
        assert_eq!(missing_pages(&slots, &pend, 90..110), [0, 1]);
        pend.insert(1);
        assert_eq!(missing_pages(&slots, &pend, 90..110), [0]);
        fill(&mut slots, 0, media(PAGE));
        assert_eq!(missing_pages(&slots, &pend, 0..110), [] as [usize; 0]);
        assert!(slots[95].is_some() && slots[96].is_none());
        // Short last page and out-of-range visible window are safe.
        fill(&mut slots, 288, media(40));
        assert_eq!(slots.iter().flatten().count(), PAGE + 12);
        assert_eq!(
            missing_pages(&slots, &HashSet::new(), 290..999),
            [] as [usize; 0]
        );
    }

    #[test]
    fn decades_group_newest_first() {
        assert_eq!(
            decades(&[1999, 2011, 2003, 2011, 2010, 1994]),
            vec![
                (2010, vec![2011, 2010]),
                (2000, vec![2003]),
                (1990, vec![1999, 1994])
            ]
        );
        assert!(decades(&[]).is_empty());
    }

    #[test]
    fn name_tabs_and_picks() {
        let o = FilterOptions {
            genres: vec!["Drama".into()],
            years: vec![2001, 2015],
            tags: vec!["t".into()],
            ..Default::default()
        };
        let g = name_entries(Tab::Genres, &o, &[]);
        assert_eq!(g[0].label, "Drama");
        let y = name_entries(Tab::Years, &o, &[]);
        assert_eq!(y[0].label, "2010s");
        let s = name_entries(Tab::Studios, &o, &[("id1".into(), "Studio".into())]);
        let mut f = Filters::default();
        s[0].pick.apply(&mut f);
        y[0].pick.apply(&mut f);
        assert_eq!((f.studios, f.years), (vec!["id1".to_string()], vec![2015]));
    }

    #[test]
    fn filter_panel_toggles() {
        let o = FilterOptions {
            genres: vec!["Drama".into(), "Noir".into()],
            official_ratings: vec!["R".into()],
            years: vec![2011, 2015, 1999],
            ..Default::default()
        };
        let mut f = Filters::default();
        toggle(&mut f, &FAct::Genre("Drama".into()));
        toggle(&mut f, &FAct::Genre("Noir".into()));
        assert_eq!(f.genres, ["Drama", "Noir"], "multi-select");
        toggle(&mut f, &FAct::Genre("Drama".into()));
        assert_eq!(f.genres, ["Noir"]);
        toggle(&mut f, &FAct::Played(Played::Unplayed));
        toggle(&mut f, &FAct::Played(Played::Played));
        assert_eq!(f.played, Played::Played, "single choice");
        toggle(&mut f, &FAct::Played(Played::Played));
        assert_eq!(f.played, Played::Any);
        toggle(&mut f, &FAct::Years(vec![2015, 2011]));
        let rows = filter_rows(&f, Some(&o));
        let dec = rows.iter().find(|r| r.label == "2010s").unwrap();
        assert!(dec.on);
        toggle(&mut f, &FAct::Years(vec![2015, 2011]));
        assert!(f.years.is_empty());
        toggle(&mut f, &FAct::Res(Res::Hd));
        toggle(&mut f, &FAct::Res(Res::Uhd4k));
        assert_eq!(f.res, Some(Res::Uhd4k));
        toggle(&mut f, &FAct::Clear);
        assert_eq!(f, Filters::default());
        // Without server options only the fixed rows exist; headings carry no action.
        let base = filter_rows(&f, None);
        assert_eq!(base.iter().filter(|r| r.act.is_none()).count(), 2);
        assert!(filter_rows(&f, Some(&o)).len() > base.len());
    }

    #[test]
    fn tabs_and_types_by_library_kind() {
        assert!(tabs_for(Some("movies")).contains(&Tab::Collections));
        assert!(!tabs_for(Some("tvshows")).contains(&Tab::Collections));
        assert_eq!(tabs_for(Some("photos")), [Tab::Library, Tab::Favourites]);
        assert_eq!(types_for(Some("tvshows")), Some(vec!["Series"]));
        assert_eq!(types_for(Some("photos")), None);
        assert_eq!(Tab::Library.label(Some("music")), "Albums");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }

    #[test]
    fn prefs_round_trip_without_transient_filters() {
        let p = LibPrefs {
            sort: SortKey::Rating,
            desc: true,
            view: ViewKind::List,
            filters: Filters {
                played: Played::Unplayed,
                genres: vec!["Drama".into()],
                ..Default::default()
            },
        };
        let t = toml::to_string(&p).unwrap();
        assert!(t.contains("sort = \"rating\"") && !t.contains("Drama"));
        let back: LibPrefs = toml::from_str(&t).unwrap();
        assert_eq!(back.filters.played, Played::Unplayed);
        assert!(back.filters.genres.is_empty());
        assert_eq!(toml::from_str::<LibPrefs>("").unwrap(), LibPrefs::default());
    }
}
