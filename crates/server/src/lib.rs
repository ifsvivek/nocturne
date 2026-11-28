//! `nocturne-server`: one headless process, one systemd user unit (docs/dev/SERVER_PLAN.md). Independently enabled modules ([`module::Module`]) behind one REST + WebSocket API (docs/api/openapi.yaml) on TCP and on a unix socket; state in one redb file ([`db`]), configuration in `server.toml`, secrets in the keyring. The same binary is the CLI.
pub mod api;
pub mod auth;
pub mod db;
pub mod decide;
pub mod dl;
pub mod fake;
pub mod http;
pub mod import;
pub mod library;
pub mod meta;
pub mod module;
pub mod modules;
pub mod naming;
pub mod real;
pub mod state;
pub mod tls;
pub mod web;

pub use redb;
