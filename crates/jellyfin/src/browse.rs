//! Browse reads (all GET): filtered/sorted/paged item queries, filter option lists, similar,
//! extras, playlists, search hints, chapter images. Kept apart from `client.rs` so the
//! player-side reads and these never edit the same lines.
use crate::{Client, Item, Items, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// Extra item fields beyond the list basics. Flattened into [`Item`]; every field defaults.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ItemExtra {
    pub official_rating: Option<String>,
    pub critic_rating: Option<f32>,
    pub premiere_date: Option<String>,
    pub end_date: Option<String>,
    pub date_created: Option<String>,
    pub child_count: Option<u32>,
    /// Whether this user may delete the item (for playlists: owns it).
    pub can_delete: Option<bool>,
    pub recursive_item_count: Option<u32>,
    pub taglines: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artists: Vec<String>,
    pub media_type: Option<String>,
    pub is_folder: bool,
    pub parent_id: Option<String>,
    pub local_trailer_count: Option<u32>,
    pub special_feature_count: Option<u32>,
    pub status: Option<String>,
    /// Image type -> tag -> blurhash.
    pub image_blur_hashes: HashMap<String, HashMap<String, String>>,
    pub studios: Vec<NameId>,
    pub people: Vec<Person>,
    pub chapters: Vec<Chapter>,
    pub external_urls: Vec<ExternalUrl>,
    pub media_sources: Vec<MediaSource>,
    pub production_locations: Vec<String>,
    pub tags: Vec<String>,
    /// "Imdb" / "Tmdb" / "Tvdb" -> id (key case varies by server version).
    pub provider_ids: HashMap<String, Option<String>>,
    /// Series: the show's own art (used for episodes).
    pub series_primary_image_tag: Option<String>,
    /// Seasons/episodes borrow the show's logo and backdrop.
    pub parent_logo_item_id: Option<String>,
    pub parent_logo_image_tag: Option<String>,
    pub parent_backdrop_item_id: Option<String>,
    pub parent_backdrop_image_tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct NameId {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    #[serde(rename = "Type")]
    pub kind: String,
    pub primary_image_tag: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Chapter {
    pub start_position_ticks: u64,
    pub name: Option<String>,
    pub image_tag: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ExternalUrl {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaSource {
    pub container: Option<String>,
    pub size: Option<u64>,
    pub bitrate: Option<u64>,
    pub name: Option<String>,
    pub media_streams: Vec<Stream>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Stream {
    /// "Video", "Audio", "Subtitle", ...
    #[serde(rename = "Type")]
    pub kind: String,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub display_title: Option<String>,
    pub title: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_rate: Option<u64>,
    pub video_range_type: Option<String>,
    pub video_range: Option<String>,
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
    pub is_default: bool,
    pub is_external: bool,
    pub is_forced: bool,
    pub bit_depth: Option<u32>,
    pub average_frame_rate: Option<f32>,
    pub sample_rate: Option<u32>,
    pub profile: Option<String>,
}

/// `GET /Items` with everything the library views need. Parameter names are Jellyfin's.
#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    pub parent_id: Option<String>,
    pub include_types: Vec<String>,
    /// Comma-joined into `sortBy` (e.g. `["CommunityRating", "SortName"]`).
    pub sort_by: Vec<String>,
    pub descending: bool,
    /// `IsPlayed`, `IsUnplayed`, `IsFavorite`.
    pub filters: Vec<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub years: Vec<u32>,
    pub official_ratings: Vec<String>,
    pub studio_ids: Vec<String>,
    pub person_ids: Vec<String>,
    pub album_artist_ids: Vec<String>,
    pub has_subtitles: bool,
    pub has_trailer: bool,
    pub is_4k: bool,
    pub is_hd: bool,
    /// Items whose name starts with this (one character).
    pub name_starts_with: Option<String>,
    /// Items sorting before this (used for the `#` bucket).
    pub name_less_than: Option<String>,
    pub search_term: Option<String>,
    pub start_index: Option<u32>,
    pub limit: Option<u32>,
    pub recursive: bool,
}

/// Extra fields a list page needs beyond the server defaults.
const LIST_FIELDS: &str =
    "DateCreated,ChildCount,Genres,PrimaryImageAspectRatio,CanDelete,ProviderIds";

impl BrowseQuery {
    pub fn pairs(&self, user_id: &str) -> Vec<(&'static str, String)> {
        let mut p = vec![("userId", user_id.to_string())];
        let mut add = |k: &'static str, v: String| p.push((k, v));
        if let Some(v) = &self.parent_id {
            add("parentId", v.clone());
        }
        if !self.include_types.is_empty() {
            add("includeItemTypes", self.include_types.join(","));
        }
        if !self.sort_by.is_empty() {
            add("sortBy", self.sort_by.join(","));
            let o = if self.descending {
                "Descending"
            } else {
                "Ascending"
            };
            add("sortOrder", o.into());
        }
        if !self.filters.is_empty() {
            add("filters", self.filters.join(","));
        }
        // Multi-value name lists are pipe-delimited (names may contain commas).
        if !self.genres.is_empty() {
            add("genres", self.genres.join("|"));
        }
        if !self.tags.is_empty() {
            add("tags", self.tags.join("|"));
        }
        if !self.official_ratings.is_empty() {
            add("officialRatings", self.official_ratings.join("|"));
        }
        if !self.studio_ids.is_empty() {
            add("studioIds", self.studio_ids.join("|"));
        }
        if !self.years.is_empty() {
            let y: Vec<String> = self.years.iter().map(u32::to_string).collect();
            add("years", y.join(","));
        }
        if !self.person_ids.is_empty() {
            add("personIds", self.person_ids.join(","));
        }
        if !self.album_artist_ids.is_empty() {
            add("albumArtistIds", self.album_artist_ids.join(","));
        }
        for (on, k) in [
            (self.has_subtitles, "hasSubtitles"),
            (self.has_trailer, "hasTrailer"),
            (self.is_4k, "is4K"),
            (self.is_hd, "isHd"),
        ] {
            if on {
                add(k, "true".into());
            }
        }
        if let Some(v) = &self.name_starts_with {
            add("nameStartsWith", v.clone());
        }
        if let Some(v) = &self.name_less_than {
            add("nameLessThan", v.clone());
        }
        if let Some(v) = &self.search_term {
            add("searchTerm", v.clone());
        }
        if let Some(v) = self.start_index {
            add("startIndex", v.to_string());
        }
        if let Some(v) = self.limit {
            add("limit", v.to_string());
        }
        if self.recursive {
            add("recursive", "true".into());
        }
        add("fields", LIST_FIELDS.into());
        p
    }
}

/// Values the filter panel and the Genres/Tags/Years tabs offer for a library.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct FilterOptions {
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub official_ratings: Vec<String>,
    pub years: Vec<u32>,
}

/// One `GET /Search/Hints` entry.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct SearchHint {
    pub item_id: String,
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
    pub production_year: Option<u32>,
    pub primary_image_tag: Option<String>,
    pub series: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub run_time_ticks: Option<u64>,
}

impl SearchHint {
    /// Servers before 10.9 only send `Id`.
    pub fn item_id(&self) -> &str {
        if self.item_id.is_empty() {
            &self.id
        } else {
            &self.item_id
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct Hints {
    search_hints: Vec<SearchHint>,
}

impl Client {
    /// Filtered, sorted, paged items.
    pub async fn browse(&self, user_id: &str, q: &BrowseQuery) -> Result<Items> {
        self.get(&["Items"], &q.pairs(user_id)).await
    }

    /// Genres, tags, official ratings and years present under a parent.
    pub async fn filter_options(
        &self,
        user_id: &str,
        parent_id: Option<&str>,
        include_types: &[String],
    ) -> Result<FilterOptions> {
        let mut p = vec![("userId", user_id.to_string())];
        if let Some(v) = parent_id {
            p.push(("parentId", v.to_string()));
        }
        if !include_types.is_empty() {
            p.push(("includeItemTypes", include_types.join(",")));
        }
        self.get(&["Items", "Filters"], &p).await
    }

    /// `GET /Studios` under a parent (ids are needed to filter by studio).
    pub async fn studios(&self, user_id: &str, parent_id: Option<&str>) -> Result<Items> {
        let mut p = vec![
            ("userId", user_id.to_string()),
            ("sortBy", "SortName".into()),
        ];
        if let Some(v) = parent_id {
            p.push(("parentId", v.to_string()));
        }
        self.get(&["Studios"], &p).await
    }

    /// `GET /Items/{id}/Similar`.
    pub async fn similar(&self, user_id: &str, id: &str, limit: u32) -> Result<Items> {
        self.get(
            &["Items", id, "Similar"],
            &[
                ("userId", user_id.to_string()),
                ("limit", limit.to_string()),
                ("fields", "ProviderIds".into()),
            ],
        )
        .await
    }

    /// Special features and local trailers of an item (both answer a bare JSON array).
    pub async fn extras(&self, user_id: &str, id: &str) -> Result<Vec<Item>> {
        let q = [("userId", user_id.to_string())];
        let mut all: Vec<Item> = self
            .get(&["Items", id, "LocalTrailers"], &q)
            .await
            .unwrap_or_default();
        all.extend(
            self.get::<Vec<Item>>(&["Items", id, "SpecialFeatures"], &q)
                .await
                .unwrap_or_default(),
        );
        Ok(all)
    }

    /// `GET /Playlists/{id}/Items` in playlist order.
    pub async fn playlist_items(&self, user_id: &str, id: &str) -> Result<Items> {
        self.get(
            &["Playlists", id, "Items"],
            &[
                ("userId", user_id.to_string()),
                ("fields", "ProviderIds".into()),
            ],
        )
        .await
    }

    /// `GET /Search/Hints`: one ranked mixed list; callers group it by type.
    pub async fn search_hints(
        &self,
        user_id: &str,
        term: &str,
        limit: u32,
    ) -> Result<Vec<SearchHint>> {
        let h: Hints = self
            .get(
                &["Search", "Hints"],
                &[
                    ("userId", user_id.to_string()),
                    ("searchTerm", term.to_string()),
                    ("limit", limit.to_string()),
                ],
            )
            .await?;
        Ok(h.search_hints)
    }

    /// Chapter thumbnail (GET).
    pub async fn chapter_image(
        &self,
        id: &str,
        index: u32,
        max_width: u32,
        tag: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut p = vec![("maxWidth", max_width.to_string())];
        if let Some(t) = tag {
            p.push(("tag", t.to_string()));
        }
        let url = self.url(&["Items", id, "Images", "Chapter", &index.to_string()], &p);
        self.fetch(url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(q: &BrowseQuery) -> Vec<String> {
        q.pairs("u")
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    #[test]
    fn minimal_query_is_user_and_fields() {
        assert_eq!(
            s(&BrowseQuery::default()),
            ["userId=u", &format!("fields={LIST_FIELDS}")]
        );
    }

    #[test]
    fn sort_and_filters_exact_parameters() {
        let q = BrowseQuery {
            parent_id: Some("lib".into()),
            include_types: vec!["Movie".into(), "Series".into()],
            sort_by: vec!["CommunityRating".into(), "SortName".into()],
            descending: true,
            filters: vec!["IsUnplayed".into(), "IsFavorite".into()],
            genres: vec!["Sci-Fi".into(), "Drama, Crime".into()],
            tags: vec!["t1".into()],
            years: vec![2010, 2011],
            official_ratings: vec!["PG-13".into(), "R".into()],
            studio_ids: vec!["s1".into(), "s2".into()],
            has_subtitles: true,
            has_trailer: true,
            is_4k: true,
            is_hd: true,
            name_starts_with: Some("B".into()),
            start_index: Some(100),
            limit: Some(50),
            recursive: true,
            ..Default::default()
        };
        assert_eq!(
            s(&q),
            [
                "userId=u",
                "parentId=lib",
                "includeItemTypes=Movie,Series",
                "sortBy=CommunityRating,SortName",
                "sortOrder=Descending",
                "filters=IsUnplayed,IsFavorite",
                "genres=Sci-Fi|Drama, Crime",
                "tags=t1",
                "officialRatings=PG-13|R",
                "studioIds=s1|s2",
                "years=2010,2011",
                "hasSubtitles=true",
                "hasTrailer=true",
                "is4K=true",
                "isHd=true",
                "nameStartsWith=B",
                "startIndex=100",
                "limit=50",
                "recursive=true",
                &format!("fields={LIST_FIELDS}"),
            ]
        );
    }

    #[test]
    fn person_artist_and_name_bucket() {
        let q = BrowseQuery {
            person_ids: vec!["p".into()],
            album_artist_ids: vec!["a".into()],
            name_less_than: Some("A".into()),
            ..Default::default()
        };
        let v = s(&q);
        assert!(v.contains(&"personIds=p".into()));
        assert!(v.contains(&"albumArtistIds=a".into()));
        assert!(v.contains(&"nameLessThan=A".into()));
    }

    #[test]
    fn detail_fixture_parses() {
        let j = r#"{"Id":"1","Name":"Invented","Type":"Movie","OfficialRating":"PG","CriticRating":71.0,
            "Taglines":["A line"],"Studios":[{"Name":"Studio One","Id":"s"}],
            "People":[{"Id":"p","Name":"Ann","Role":"Lead","Type":"Actor","PrimaryImageTag":"t"}],
            "Chapters":[{"StartPositionTicks":600000000,"Name":"One","ImageTag":"c"}],
            "ExternalUrls":[{"Name":"IMDb","Url":"https://example.test/x"}],
            "ImageBlurHashes":{"Primary":{"t":"LEHV6nWB2yk8pyo0adR*.7kCMdnj"}},
            "MediaSources":[{"Container":"mkv","Bitrate":8000000,"MediaStreams":[
              {"Type":"Video","Codec":"hevc","Width":3840,"Height":2160,"VideoRangeType":"HDR10"},
              {"Type":"Audio","Codec":"eac3","Channels":6,"Language":"eng","IsDefault":true}]}],
            "UserData":{"UnplayedItemCount":3,"Played":false}}"#;
        let i: Item = serde_json::from_str(j).unwrap();
        let x = &i.x;
        assert_eq!(x.official_rating.as_deref(), Some("PG"));
        assert_eq!(x.people[0].role.as_deref(), Some("Lead"));
        assert_eq!(x.chapters[0].start_position_ticks, 600_000_000);
        assert_eq!(x.media_sources[0].media_streams.len(), 2);
        assert_eq!(x.media_sources[0].media_streams[0].height, Some(2160));
        assert_eq!(x.image_blur_hashes["Primary"]["t"].len(), 28);
        assert_eq!(i.user_data.unwrap().unplayed_item_count, Some(3));
        let h: Hints = serde_json::from_str(
            r#"{"SearchHints":[{"ItemId":"i","Name":"N","Type":"Movie"},{"Id":"o","Name":"M","Type":"Person"}]}"#,
        )
        .unwrap();
        assert_eq!(h.search_hints[0].item_id(), "i");
        assert_eq!(h.search_hints[1].item_id(), "o", "pre-10.9 servers send Id");
    }
}
