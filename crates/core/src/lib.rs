//! UI-framework-free logic shared by the front ends: settings, navigation, list/detail state,
//! playback logic, the Jellyfin source layer, caches, and admin/account state machines.
pub mod account;
pub mod admin;
pub mod blurhash;
pub mod bridge;
pub mod cache;
pub mod detail;
pub mod dropdown;
pub mod help;
pub mod imgcache;
pub mod libview;
pub mod nav;
pub mod player_logic;
pub mod player_mpris;
pub mod profile;
pub mod search;
pub mod settings;
pub mod source;
pub mod sync;
pub mod syncplay;
pub mod video;
