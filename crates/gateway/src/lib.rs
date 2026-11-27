//! `nocturne-gateway`: one Jellyfin-compatible address over every NOCTURNE source (docs/dev/GATEWAY_PLAN.md).
//! An optional companion process: the app starts it, talks to it over the control socket, and nothing in the app links this crate.
//!
//! # For the workers adding routes
//!
//! * **State.** Every handler takes `axum::extract::State<State>` ([`state::State`], cheap to clone, derefs to [`state::Inner`]):
//!   `state.cfg()` settings snapshot (`gw`, `allow_server_writes`, `sync_progress`, `sync_watched`, [`config::Config::forward_progress`]),
//!   `state.ids` the id map ([`idmap::IdMap`]: `register_single`, `register`, `resolve_str`; build keys with `idmap::key_*`),
//!   `state.upstreams()` the sources in priority order with `Secret` server tokens (only ever into an `Authorization` header; never into a response, a log or a file),
//!   `state.mark_signed_out(id)` after a 401, `state.limits` (`command` 20/s per token, `sockets.acquire(token_id)` 4 per token),
//!   `state.users` / `state.tokens` (gateway user and devices) and `state.paths.data` for your own 0600 files (`config::write_private`).
//!   Per-module state without editing `state.rs`: `state.ext::<T>()` returns one shared `Arc<T>` per type (`T: Default`).
//! * **Registration.** Each of `items`, `images`, `stream`, `playback`, `userdata`, `sessions`, `ws` has `pub fn routes(r: Routes) -> Routes`,
//!   called from [`server::build`]. Add routes with `r.add(path, Access::Auth, get(handler).post(..))`; use `add_stream` for media and the WebSocket
//!   (no 60 s timeout). Helper modules `lists`, `rewrite`, `local_dto` are empty files for worker B. To add a module, add one `mod` line below and one line in `server::build`.
//! * **Access.** `Public` (no token, only pre-login routes), `Auth` (gateway token), `Remote` (token plus `gateway_remote_control`; use it for the session and command routes).
//!   There is no way to register a route without one; the `routes_are_gated` test in `tests/gateway.rs` calls every registered route without a token.
//! * **The caller.** `Extension<auth::AuthCtx>` on `Auth`/`Remote` routes (token id, client, device); `Extension<auth::ClientInfo>` on all (its `token` is always cleared, handlers never see the client token);
//!   `Extension<server::ClientIp>` for the effective address.
//! * **Path parameters** use axum 0.8 syntax (`{itemId}`); matchit 0.8 accepts different names at the same position (tested), and a static segment wins over a parameter (`/Users/Me` over `/Users/{userId}`).
//! * **Never**: write to `crates/jellyfin` (GET only), forward a client header or token to a server, put a server token or URL into a response, log through anything but [`glog!`], mint `Confirmed` / `UserConfirmed`.
#![recursion_limit = "256"] // the big json! literals of the sign-in result
pub mod auth;
pub mod config;
pub mod control;
pub mod discovery;
pub mod idmap;
pub mod images;
pub mod items;
pub mod jsonbody;
pub mod limits;
pub mod lists;
pub mod local_dto;
pub mod log;
pub mod misc;
pub mod playback;
pub mod rewrite;
pub mod server;
pub mod sessions;
pub mod state;
pub mod stream;
pub mod tls;
pub mod userdata;
pub mod web;
pub mod ws;
