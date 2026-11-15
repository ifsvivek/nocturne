//! Tolerant models: unknown fields ignored, missing ones default. Types that are written back
//! (`UserPolicy`, `ServerConfig`) keep unknown fields in `extra` so a read-modify-write is lossless.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct SystemInfo {
    pub server_name: String,
    pub version: String,
    pub product_name: String,
    pub id: String,
    pub operating_system: String,
    pub operating_system_display_name: String,
    pub system_architecture: String,
    pub local_address: Option<String>,
    pub program_data_path: String,
    pub web_path: String,
    pub cache_path: String,
    pub log_path: String,
    pub internal_metadata_path: String,
    pub transcoding_temp_path: String,
    pub encoder_location: String,
    pub web_socket_port_number: u16,
    pub has_pending_restart: bool,
    pub is_shutting_down: bool,
    pub can_self_restart: bool,
    pub has_update_available: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ActivityEntry {
    pub id: i64,
    pub name: String,
    pub overview: Option<String>,
    pub short_overview: Option<String>,
    #[serde(rename = "Type")]
    pub kind: String,
    pub item_id: Option<String>,
    pub date: String,
    pub user_id: Option<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ActivityPage {
    pub items: Vec<ActivityEntry>,
    pub total_record_count: u32,
    pub start_index: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Session {
    pub id: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub application_version: String,
    pub remote_end_point: Option<String>,
    pub last_activity_date: String,
    pub is_active: bool,
    pub supports_remote_control: bool,
    pub now_playing_item: Option<NowPlaying>,
    pub play_state: Option<PlayState>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct NowPlaying {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PlayState {
    pub is_paused: bool,
    pub position_ticks: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub app_name: Option<String>,
    pub app_version: Option<String>,
    pub last_user_name: Option<String>,
    pub last_user_id: Option<String>,
    pub date_last_activity: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub key: String,
    pub description: String,
    pub category: String,
    /// `Idle`, `Running`, `Cancelling`.
    pub state: String,
    pub current_progress_percentage: Option<f64>,
    pub is_hidden: bool,
    pub triggers: Vec<TaskTrigger>,
    pub last_execution_result: Option<TaskResult>,
}

/// A scheduled-task trigger; also the shape the server accepts back (see `edit::validate_trigger`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct TaskTrigger {
    /// `DailyTrigger`, `WeeklyTrigger`, `IntervalTrigger` or `StartupTrigger`.
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_of_day_ticks: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_ticks: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_of_week: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_runtime_ticks: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct TaskResult {
    pub start_time_utc: String,
    pub end_time_utc: String,
    pub status: String,
    pub name: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct VirtualFolder {
    pub name: String,
    pub locations: Vec<String>,
    pub collection_type: Option<String>,
    /// Library id; pass to `refresh_library_folder`.
    pub item_id: String,
    /// `Idle`, `Queued`, `Active`.
    pub refresh_status: Option<String>,
    pub refresh_progress: Option<f64>,
    /// Raw library options (display, NFO, trickplay flags); shown read-only.
    pub library_options: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct User {
    pub id: String,
    pub name: String,
    pub has_password: bool,
    pub last_login_date: Option<String>,
    pub last_activity_date: Option<String>,
    pub policy: UserPolicy,
}

/// Full user policy; round-trips every field the server sends (unknown ones live in `extra`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserPolicy {
    pub is_administrator: bool,
    pub is_disabled: bool,
    pub is_hidden: bool,
    pub enable_media_playback: bool,
    pub enable_content_deletion: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub status: String,
    pub can_uninstall: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct LogFile {
    pub name: String,
    pub size: u64,
    pub date_created: String,
    pub date_modified: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Branding {
    pub login_disclaimer: Option<String>,
    pub custom_css: Option<String>,
    pub splashscreen_enabled: bool,
}

/// Server configuration; only a few fields are typed, the rest round-trip through `extra`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ServerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(rename = "UICulture", skip_serializing_if = "Option::is_none")]
    pub ui_culture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_file_retention_days: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_log_retention_days: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_connect_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_resume_pct: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_resume_pct: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_resume_duration_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_client_bitrate_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_metadata_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_country_code: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `GET /Items/Counts`: library totals by type.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ItemCounts {
    pub movie_count: u32,
    pub series_count: u32,
    pub episode_count: u32,
    pub album_count: u32,
    pub song_count: u32,
    pub music_video_count: u32,
    pub box_set_count: u32,
    pub book_count: u32,
    pub trailer_count: u32,
    pub program_count: u32,
}

/// `GET /System/Configuration/network` (read-only view of the common fields).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct NetworkConfig {
    pub base_url: String,
    pub enable_https: bool,
    pub require_https: bool,
    pub internal_http_port: u16,
    pub internal_https_port: u16,
    pub public_http_port: u16,
    pub public_https_port: u16,
    pub auto_discovery: bool,
    #[serde(rename = "EnableUPnP")]
    pub enable_upnp: bool,
    #[serde(rename = "EnableIPv4")]
    pub enable_ipv4: bool,
    #[serde(rename = "EnableIPv6")]
    pub enable_ipv6: bool,
    pub enable_remote_access: bool,
    /// LAN networks (CIDR or address) the server treats as local.
    pub local_network_subnets: Vec<String>,
    /// Addresses the server binds to; empty = all interfaces.
    pub local_network_addresses: Vec<String>,
}

/// Plugin catalog entry (`GET /Packages`; the server uses lowercase keys here).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Package {
    pub name: String,
    pub description: String,
    pub category: String,
    pub owner: String,
    pub guid: String,
    /// Newest first.
    pub versions: Vec<PackageVersion>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PackageVersion {
    pub version: String,
    #[serde(rename = "repositoryUrl")]
    pub repository_url: String,
}

/// API key. `Debug` redacts the token.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiKey {
    pub access_token: String,
    pub app_name: String,
    pub date_created: String,
    pub date_last_activity: String,
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKey")
            .field("app_name", &self.app_name)
            .field("access_token", &"<redacted>")
            .finish()
    }
}

/// Paged `{Items, TotalRecordCount}` envelope.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct Page<T> {
    pub items: Vec<T>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self { items: vec![] }
    }
}
