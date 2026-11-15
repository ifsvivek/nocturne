use serde::Deserialize;
use std::collections::HashMap;

/// Only the fields NOCTURNE needs; unknown fields are ignored, missing ones default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
    pub collection_type: Option<String>,
    pub overview: Option<String>,
    pub production_year: Option<u32>,
    pub run_time_ticks: Option<u64>,
    pub community_rating: Option<f32>,
    pub index_number: Option<u32>,
    pub parent_index_number: Option<u32>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    /// Image type name (e.g. "Primary") -> tag, for cache-busting image URLs.
    pub image_tags: HashMap<String, String>,
    /// Backdrop image tags (first one busts the cache for `ImageType::Backdrop`).
    pub backdrop_image_tags: Vec<String>,
    pub genres: Vec<String>,
    pub user_data: Option<UserData>,
}

/// Per-user playback state (read only; NOCTURNE never writes it back).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserData {
    pub playback_position_ticks: u64,
    pub played: bool,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Items {
    pub items: Vec<Item>,
    pub total_record_count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PublicInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
}
