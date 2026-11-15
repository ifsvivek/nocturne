//! SyncPlay ("watch together") for the user-scope client: wire types, clock sync, the pure state
//! machine, REST calls and the WebSocket. See docs/syncplay.md for what was verified and how the
//! official web client behaves.
mod engine;
mod model;
mod rest;
mod time;
pub mod ws;

pub use engine::*;
pub use model::*;
pub use rest::NewQueue;
pub use time::{Sample, TRACKED, TimeSync};
