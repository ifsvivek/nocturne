//! The `/socket` WebSocket (plan section 4). One task per socket and nothing running while nobody listens: no `Sessions` polling until a client sends `SessionsStart`, no timer beyond the 30 s housekeeping tick.
//!
//! Frames the gateway sends: `ForceKeepAlive` (Data 60) on connect, `KeepAlive` in answer to the client's, `Sessions` (the merged table, only when it changed) after `SessionsStart`, `Play` / `Playstate` / `GeneralCommand` for commands aimed at this client ([`crate::sessions`]), and `LibraryChanged` / `UserDataChanged` (coalesced over 5 s). Frames it accepts: `KeepAlive`, `SessionsStart`, `SessionsStop`; anything else is ignored.
use crate::auth::{AuthCtx, random_hex, status_json};
use crate::server::{Access, Routes};
use crate::sessions::{self, Event, Hub};
use crate::state::State;
use axum::Extension;
use axum::extract::State as St;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const FORCE_KEEPALIVE_SECS: u64 = 60;
/// A client that says nothing for this long (it should send a KeepAlive every 30 s) is dropped.
const IDLE: Duration = Duration::from_secs(150);
const HOUSEKEEPING: Duration = Duration::from_secs(30);
const SESSIONS_EVERY: Duration = Duration::from_millis(1500);
const COALESCE: Duration = Duration::from_secs(5);
const MAX_FRAME: usize = 16 * 1024;

/// `{"MessageType":..., "MessageId":..., "Data":...}`
pub fn frame(kind: &str, data: Value) -> String {
    json!({"MessageType": kind, "MessageId": random_hex::<16>(), "Data": data}).to_string()
}

/// What a client may send. Everything else is [`Incoming::Other`].
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    KeepAlive,
    SessionsStart,
    SessionsStop,
    Other,
}

pub fn parse_incoming(text: &str) -> Incoming {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Incoming::Other;
    };
    match v["MessageType"].as_str() {
        Some("KeepAlive") => Incoming::KeepAlive,
        Some("SessionsStart") => Incoming::SessionsStart,
        Some("SessionsStop") => Incoming::SessionsStop,
        _ => Incoming::Other,
    }
}

/// Library and user-data events are held for 5 s and sent once, however many arrived.
#[derive(Default)]
pub struct Coalesce {
    library: bool,
    /// By `ItemId`; a later report of the same item replaces the earlier one.
    user_data: BTreeMap<String, Value>,
    due: Option<Instant>,
}

impl Coalesce {
    pub fn note(&mut self, e: Event, now: Instant) {
        match e {
            Event::Library => self.library = true,
            Event::UserData(items) => {
                for d in items {
                    if let Some(id) = d["ItemId"].as_str() {
                        self.user_data.insert(id.to_string(), d);
                    }
                }
            }
        }
        self.due.get_or_insert(now + COALESCE);
    }

    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    /// The frames to send if the hold is over.
    pub fn take(&mut self, now: Instant, user_id: &str) -> Vec<String> {
        if self.due.is_none_or(|d| now < d) {
            return vec![];
        }
        self.due = None;
        let mut out = vec![];
        if std::mem::take(&mut self.library) {
            out.push(frame(
                "LibraryChanged",
                json!({"ItemsAdded": [], "ItemsRemoved": [], "ItemsUpdated": [], "CollectionFolders": [], "IsEmpty": false}),
            ));
        }
        let items = std::mem::take(&mut self.user_data);
        if !items.is_empty() {
            out.push(frame(
                "UserDataChanged",
                json!({"UserId": user_id, "UserDataList": items.into_values().collect::<Vec<_>>()}),
            ));
        }
        out
    }
}

async fn socket(
    St(st): St<State>,
    Extension(ctx): Extension<AuthCtx>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(slot) = st.limits.sockets.acquire(&ctx.token_id) else {
        return status_json(StatusCode::TOO_MANY_REQUESTS, "Too many sockets");
    };
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |s| async move {
            run(st, ctx, s).await;
            drop(slot);
        })
        .into_response()
}

async fn send(s: &mut WebSocket, text: String) -> bool {
    s.send(Message::text(text)).await.is_ok()
}

/// Sleep until `at`, or forever.
async fn until(at: Option<Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t.into()).await,
        None => std::future::pending().await,
    }
}

async fn run(st: State, ctx: AuthCtx, mut s: WebSocket) {
    let hub = st.ext::<Hub>();
    let (conn, mut rx) = hub.attach(&ctx);
    let mut events = hub.events();
    let mut co = Coalesce::default();
    let mut sessions_next: Option<Instant> = None;
    let mut last_sessions: Option<Value> = None;
    let mut last_rx = Instant::now();
    let mut house = tokio::time::interval(HOUSEKEEPING);
    house.tick().await;
    let mut alive = send(&mut s, frame("ForceKeepAlive", json!(FORCE_KEEPALIVE_SECS))).await;
    while alive {
        tokio::select! {
            m = s.recv() => match m {
                Some(Ok(Message::Text(t))) => {
                    last_rx = Instant::now();
                    hub.touch(&ctx.token_id);
                    match parse_incoming(t.as_str()) {
                        Incoming::KeepAlive => alive = send(&mut s, frame("KeepAlive", Value::Null)).await,
                        Incoming::SessionsStart => {
                            last_sessions = None;
                            sessions_next = Some(Instant::now());
                        }
                        Incoming::SessionsStop => sessions_next = None,
                        Incoming::Other => {}
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => alive = false,
                Some(Ok(_)) => last_rx = Instant::now(), // ping, pong, binary: alive, nothing to do
            },
            Some(text) = rx.recv() => alive = send(&mut s, text).await,
            e = events.recv() => match e {
                Ok(e) => co.note(e, Instant::now()),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => co.note(Event::Library, Instant::now()),
                Err(_) => {}
            },
            () = until(sessions_next) => {
                sessions_next = Some(Instant::now() + SESSIONS_EVERY);
                let now = serde_json::to_value(sessions::list(&st, &ctx).await).unwrap_or_default();
                if last_sessions.as_ref() != Some(&now) {
                    alive = send(&mut s, frame("Sessions", now.clone())).await;
                    last_sessions = Some(now);
                }
            }
            () = until(co.due()) => {
                for f in co.take(Instant::now(), &st.users.user_id()) {
                    if !send(&mut s, f).await {
                        alive = false;
                        break;
                    }
                }
            }
            _ = house.tick() => {
                // A revoked token ends its sockets; a silent client is dropped.
                let known = st.tokens.list().iter().any(|t| t.id == ctx.token_id);
                alive = known && last_rx.elapsed() < IDLE;
            }
        }
    }
    hub.detach(&ctx.token_id, conn);
    let _ = s.send(Message::Close(None)).await;
}

pub fn routes(r: Routes) -> Routes {
    // Any signed-in client may open the socket (library news, KeepAlive); what is sent over it is gated where it is produced.
    r.add_stream("/socket", Access::Auth, get(socket))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_frames() {
        assert_eq!(
            parse_incoming(r#"{"MessageType":"KeepAlive"}"#),
            Incoming::KeepAlive
        );
        assert_eq!(
            parse_incoming(r#"{"MessageType":"SessionsStart","Data":"0,1500"}"#),
            Incoming::SessionsStart
        );
        assert_eq!(
            parse_incoming(r#"{"MessageType":"SessionsStop"}"#),
            Incoming::SessionsStop
        );
        for other in [
            r#"{"MessageType":"ScheduledTasksInfoStart","Data":"0,1000"}"#,
            r#"{"MessageType":"Play"}"#,
            "not json",
            "{}",
            "[]",
        ] {
            assert_eq!(parse_incoming(other), Incoming::Other, "{other}");
        }
    }

    #[test]
    fn frame_shape() {
        let v: Value = serde_json::from_str(&frame("ForceKeepAlive", json!(60))).unwrap();
        assert_eq!(v["MessageType"], "ForceKeepAlive");
        assert_eq!(v["Data"], 60);
        assert_eq!(v["MessageId"].as_str().unwrap().len(), 32);
    }

    #[test]
    fn events_are_held_five_seconds_and_sent_once() {
        let t0 = Instant::now();
        let mut c = Coalesce::default();
        assert!(c.due().is_none() && c.take(t0, "u").is_empty());
        c.note(Event::Library, t0);
        c.note(Event::Library, t0 + Duration::from_secs(1));
        c.note(
            Event::UserData(vec![
                json!({"ItemId": "b", "Played": false}),
                json!({"ItemId": "a", "Played": false}),
            ]),
            t0 + Duration::from_secs(2),
        );
        c.note(
            Event::UserData(vec![json!({"ItemId": "a", "Played": true})]),
            t0 + Duration::from_secs(3),
        );
        assert!(
            c.take(t0 + Duration::from_secs(4), "u").is_empty(),
            "still held"
        );
        let out = c.take(t0 + Duration::from_secs(5), "user1");
        assert_eq!(out.len(), 2);
        let lib: Value = serde_json::from_str(&out[0]).unwrap();
        assert_eq!(lib["MessageType"], "LibraryChanged");
        let ud: Value = serde_json::from_str(&out[1]).unwrap();
        assert_eq!(ud["MessageType"], "UserDataChanged");
        assert_eq!(ud["Data"]["UserId"], "user1");
        assert_eq!(
            ud["Data"]["UserDataList"],
            json!([{"ItemId": "a", "Played": true}, {"ItemId": "b", "Played": false}])
        );
        assert!(
            c.due().is_none() && c.take(t0 + Duration::from_secs(60), "u").is_empty(),
            "nothing is resent"
        );
    }
}
