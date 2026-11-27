//! Remote control and the /socket WebSocket, in process: the real server on an ephemeral loopback port, two native clients talking to each other over real sockets, and a mock server standing in for an upstream Jellyfin. No keyring, no real server.
use axum::Router;
use axum::extract::{Request, State as AxState};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use nocturne_core::gateway::GatewaySettings;
use nocturne_core::source::SourceId;
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::server;
use nocturne_gateway::state::{Secret, State, UpKind, UpState, Upstream};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;

type Ws = WebSocketStream<reqwest::Upgraded>;

struct Gw {
    st: State,
    base: String,
    http: reqwest::Client,
    pw: String,
    _dir: tempfile::TempDir,
}

async fn start(remote: bool, writes: bool) -> Gw {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        data: dir.path().join("data"),
        runtime: None,
        settings: Some(dir.path().join("settings.toml")),
        sources: None,
    };
    let cfg = Config {
        gw: GatewaySettings {
            port: 0,
            remote_control: remote,
            ..Default::default()
        },
        allow_server_writes: writes,
        ..Default::default()
    };
    let st = State::open(paths, cfg).unwrap();
    let pw = st.users.take_unseen().unwrap();
    tokio::spawn(server::run(st.clone()));
    let addrs = tokio::time::timeout(Duration::from_secs(5), st.wait_listening())
        .await
        .unwrap();
    Gw {
        st,
        base: format!("http://127.0.0.1:{}", addrs[0].port()),
        http: reqwest::Client::new(),
        pw,
        _dir: dir,
    }
}

fn auth(token: &str) -> String {
    format!(r#"MediaBrowser Token="{token}""#)
}

impl Gw {
    fn url(&self, p: &str) -> String {
        format!("{}{p}", self.base)
    }

    async fn login(&self, device_id: &str, client: &str) -> String {
        let r = self
            .http
            .post(self.url("/Users/AuthenticateByName"))
            .header(
                "Authorization",
                format!(r#"MediaBrowser Client="{client}", Device="{device_id}", DeviceId="{device_id}", Version="1""#),
            )
            .json(&json!({"Username": "nocturne", "Pw": self.pw}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.json::<Value>().await.unwrap()["AccessToken"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn post(&self, token: &str, path: &str, body: Option<Value>) -> StatusCode {
        let mut r = self
            .http
            .post(self.url(path))
            .header("Authorization", auth(token));
        if let Some(b) = body {
            r = r.json(&b);
        }
        r.send().await.unwrap().status()
    }

    async fn sessions(&self, token: &str) -> Vec<Value> {
        let r = self
            .http
            .get(self.url("/Sessions"))
            .header("Authorization", auth(token))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.json().await.unwrap()
    }

    async fn socket(&self, token: &str) -> Ws {
        let r = self
            .http
            .get(self.url("/socket"))
            .header("Authorization", auth(token))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::SWITCHING_PROTOCOLS);
        let up = r.upgrade().await.unwrap();
        WebSocketStream::from_raw_socket(up, Role::Client, None).await
    }
}

/// The next text frame, parsed.
async fn next(ws: &mut Ws) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(6), ws.next())
            .await
            .expect("a frame within 6 s")
            .expect("socket open")
            .expect("frame");
        if let Message::Text(t) = m {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

const CAPS: fn() -> Value = || json!({"PlayableMediaTypes": ["Video", "Audio"], "SupportedCommands": ["SetVolume", "ToggleMute", "DisplayMessage"], "SupportsMediaControl": true});

#[tokio::test]
async fn one_native_client_drives_another_over_the_socket() {
    let g = start(true, false).await;
    let (ta, tb) = (
        g.login("dev-a", "Client A").await,
        g.login("dev-b", "Client B").await,
    );
    let (mut wa, mut wb) = (g.socket(&ta).await, g.socket(&tb).await);
    for w in [&mut wa, &mut wb] {
        let f = next(w).await;
        assert_eq!(
            (f["MessageType"].as_str(), f["Data"].as_i64()),
            (Some("ForceKeepAlive"), Some(60))
        );
    }
    assert_eq!(
        g.post(&ta, "/Sessions/Capabilities/Full", Some(CAPS()))
            .await,
        204
    );
    assert_eq!(
        g.post(&tb, "/Sessions/Capabilities/Full", Some(CAPS()))
            .await,
        204
    );

    // A sees B, controllable, with gateway ids and the gateway user.
    let list = g.sessions(&ta).await;
    assert_eq!(list.len(), 2);
    let b = list.iter().find(|s| s["DeviceId"] == "dev-b").unwrap();
    assert_eq!(
        (b["Client"].as_str(), b["SupportsRemoteControl"].as_bool()),
        (Some("Client B"), Some(true))
    );
    assert_eq!(b["UserName"], "nocturne");
    assert_eq!(
        b["Capabilities"]["SupportedCommands"],
        json!(["SetVolume", "ToggleMute", "DisplayMessage"])
    );
    let bid = b["Id"].as_str().unwrap().to_string();

    // Playstate
    assert_eq!(
        g.post(&ta, &format!("/Sessions/{bid}/Playing/Pause"), None)
            .await,
        204
    );
    let f = next(&mut wb).await;
    assert_eq!(f["MessageType"], "Playstate");
    assert_eq!(f["Data"]["Command"], "Pause");
    assert_eq!(
        g.post(
            &ta,
            &format!("/Sessions/{bid}/Playing/Seek?SeekPositionTicks=600000000"),
            None
        )
        .await,
        204
    );
    let f = next(&mut wb).await;
    assert_eq!(
        (
            f["Data"]["Command"].as_str(),
            f["Data"]["SeekPositionTicks"].as_i64()
        ),
        (Some("Seek"), Some(600_000_000))
    );

    // Play: a gateway item id, any item (the client resolves it through the gateway); an id never handed out is a 404.
    let item = g.st.ids.register_single("srvX", "item1").to_string();
    let url =
        format!("/Sessions/{bid}/Playing?PlayCommand=PlayNow&itemIds={item}&StartPositionTicks=5");
    assert_eq!(g.post(&ta, &url, None).await, 204);
    let f = next(&mut wb).await;
    assert_eq!(f["MessageType"], "Play");
    assert_eq!(f["Data"]["ItemIds"], json!([item]));
    assert_eq!(f["Data"]["PlayCommand"], "PlayNow");
    assert_eq!(f["Data"]["StartPositionTicks"], 5);
    let unknown = format!(
        "/Sessions/{bid}/Playing?PlayCommand=PlayNow&ItemIds={}",
        "ab".repeat(16)
    );
    assert_eq!(g.post(&ta, &unknown, None).await, 404);

    // General commands: allowed ones arrive, the rest are refused before anything is sent.
    assert_eq!(
        g.post(&ta, &format!("/Sessions/{bid}/Command/ToggleMute"), None)
            .await,
        204
    );
    let f = next(&mut wb).await;
    assert_eq!(
        (f["MessageType"].as_str(), f["Data"]["Name"].as_str()),
        (Some("GeneralCommand"), Some("ToggleMute"))
    );
    assert_eq!(
        g.post(
            &ta,
            &format!("/Sessions/{bid}/Command"),
            Some(json!({"Name": "SetVolume", "Arguments": {"Volume": "40"}}))
        )
        .await,
        204
    );
    let f = next(&mut wb).await;
    assert_eq!(f["Data"]["Arguments"]["Volume"], "40");
    assert_eq!(
        g.post(
            &ta,
            &format!("/Sessions/{bid}/Message"),
            Some(json!({"Header": "Hi", "Text": "Dinner", "TimeoutMs": 3000}))
        )
        .await,
        204
    );
    let f = next(&mut wb).await;
    assert_eq!(
        (
            f["Data"]["Name"].as_str(),
            f["Data"]["Arguments"]["Text"].as_str()
        ),
        (Some("DisplayMessage"), Some("Dinner"))
    );
    assert_eq!(
        g.post(
            &ta,
            &format!("/Sessions/{bid}/Command"),
            Some(json!({"Name": "Shutdown"}))
        )
        .await,
        400
    );
    assert_eq!(
        g.post(&ta, &format!("/Sessions/{bid}/Command/RestartApp"), None)
            .await,
        400
    );
    assert_eq!(
        g.post(
            &ta,
            "/Sessions/00000000000000000000000000000000/Playing/Pause",
            None
        )
        .await,
        404
    );

    // SessionsStart: a push now, then only when the table changes (B leaves).
    wa.send(Message::text(
        r#"{"MessageType":"SessionsStart","Data":"0,1500"}"#,
    ))
    .await
    .unwrap();
    let f = next(&mut wa).await;
    assert_eq!(f["MessageType"], "Sessions");
    assert_eq!(f["Data"].as_array().unwrap().len(), 2);
    drop(wb);
    let f = loop {
        let f = next(&mut wa).await;
        if f["MessageType"] == "Sessions" {
            break f;
        }
    };
    assert_eq!(
        f["Data"].as_array().unwrap().len(),
        1,
        "B's socket is gone, so is its session"
    );
    // KeepAlive is answered; SessionsStop ends the pushes.
    wa.send(Message::text(r#"{"MessageType":"SessionsStop"}"#))
        .await
        .unwrap();
    wa.send(Message::text(r#"{"MessageType":"KeepAlive"}"#))
        .await
        .unwrap();
    assert_eq!(next(&mut wa).await["MessageType"], "KeepAlive");
    g.st.stop();
}

#[tokio::test]
async fn library_and_user_data_news_arrive_coalesced() {
    let g = start(true, false).await;
    let t = g.login("dev-a", "Client A").await;
    let mut w = g.socket(&t).await;
    assert_eq!(next(&mut w).await["MessageType"], "ForceKeepAlive");
    let hub = g.st.ext::<nocturne_gateway::sessions::Hub>();
    hub.library_changed();
    hub.library_changed();
    hub.user_data_changed(vec![
        json!({"ItemId": "i1", "Played": false, "PlayCount": 0}),
    ]);
    hub.user_data_changed(vec![
        json!({"ItemId": "i2", "Played": true}),
        json!({"ItemId": "i1", "Played": true, "PlayCount": 1}),
    ]);
    let (a, b) = (next(&mut w).await, next(&mut w).await);
    assert_eq!(
        (a["MessageType"].as_str(), b["MessageType"].as_str()),
        (Some("LibraryChanged"), Some("UserDataChanged"))
    );
    assert_eq!(
        b["Data"]["UserDataList"].as_array().unwrap().len(),
        2,
        "one frame for everything that happened in the window"
    );
    g.st.stop();
}

#[tokio::test]
async fn with_remote_control_off_only_the_own_session_shows_and_nothing_can_be_sent() {
    let g = start(false, true).await;
    let (ta, tb) = (g.login("dev-a", "A").await, g.login("dev-b", "B").await);
    let _wb = g.socket(&tb).await;
    assert_eq!(
        g.post(&ta, "/Sessions/Capabilities/Full", Some(CAPS()))
            .await,
        204
    );
    let list = g.sessions(&ta).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["DeviceId"], "dev-a");
    assert_eq!(
        g.post(&ta, "/Sessions/anything/Playing/Pause", None).await,
        403
    );
    assert_eq!(
        g.post(&ta, "/Sessions/anything/Command/Mute", None).await,
        403
    );
    assert_eq!(
        g.post(
            &ta,
            "/Sessions/anything/Message",
            Some(json!({"Text": "x"}))
        )
        .await,
        403
    );
    g.st.stop();
}

#[tokio::test]
async fn commands_are_limited_per_token() {
    let g = start(true, false).await;
    let t = g.login("dev-a", "A").await;
    let mut codes = vec![];
    for _ in 0..40 {
        codes.push(
            g.post(
                &t,
                "/Sessions/00000000000000000000000000000000/Playing/Pause",
                None,
            )
            .await,
        );
    }
    assert!(
        codes[..15].iter().all(|c| *c == 404),
        "under the limit the session is simply unknown"
    );
    assert!(
        codes.contains(&StatusCode::TOO_MANY_REQUESTS),
        "20 per second per token: {codes:?}"
    );
    g.st.stop();
}

// ---------------------------------------------------------------- a mock upstream server

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(String, String, bool)>>>);

async fn mock_any(AxState(seen): AxState<Seen>, req: Request) -> axum::response::Response {
    let uri = req.uri().to_string();
    let method = req.method().to_string();
    let with_token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains(r#"Token="UPTOKEN""#));
    seen.0
        .lock()
        .unwrap()
        .push((method.clone(), uri.clone(), with_token));
    if method == "GET" && uri.starts_with("/Sessions") {
        return axum::Json(json!([{
            "Id": "realsess1", "DeviceId": "mpv-1", "Client": "Jellyfin MPV Shim", "DeviceName": "Den",
            "UserId": "acct", "UserName": "real", "SupportsRemoteControl": true, "RemoteEndPoint": "10.1.2.3",
            "LastActivityDate": "2026-11-27T10:00:00.0000000Z",
            "NowPlayingItem": {"Id": "realitem0", "Path": "/srv/media/x.mkv", "MediaSources": [{"TranscodingUrl": "/a?api_key=UPTOKEN"}]},
        }]))
        .into_response();
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn mock_server() -> (String, Seen) {
    let seen = Seen::default();
    let app = Router::new().fallback(mock_any).with_state(seen.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (url, seen)
}

fn upstream(url: String) -> Upstream {
    Upstream {
        id: SourceId(1),
        label: "Server A".into(),
        kind: UpKind::Server {
            url,
            user_id: "acct".into(),
            source_key: "srvA".into(),
            token: Some(Secret::new("UPTOKEN".into())),
        },
        state: UpState::Ready,
    }
}

#[tokio::test]
async fn commands_for_a_device_on_a_server_are_forwarded_with_that_servers_token() {
    let (url, seen) = mock_server().await;
    let g = start(true, true).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev-a", "A").await;

    let r = g
        .http
        .get(g.url("/Sessions"))
        .header("Authorization", auth(&t))
        .send()
        .await
        .unwrap();
    let text = r.text().await.unwrap();
    for leak in [
        "UPTOKEN",
        "/srv/media",
        "10.1.2.3",
        "Path",
        "TranscodingUrl",
        "realitem0",
        "\"real\"",
    ] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
    let list: Vec<Value> = serde_json::from_str(&text).unwrap();
    let den = list.iter().find(|s| s["DeviceId"] == "mpv-1").unwrap();
    assert_eq!(den["SupportsRemoteControl"], true);
    assert_eq!(den["UserName"], "nocturne");
    let id = den["Id"].as_str().unwrap().to_string();
    assert_ne!(id, "realsess1");
    {
        // The listing asked the server as the stored account, narrowed to what it may control.
        let s = seen.0.lock().unwrap();
        assert!(s[0].1.contains("ControllableByUserId=acct") && s[0].2);
    }

    // Playstate goes to the member session of that server.
    assert_eq!(
        g.post(&t, &format!("/Sessions/{id}/Playing/Pause"), None)
            .await,
        204
    );
    // Play: an item of that server is passed on under the server's own id, never the gateway's.
    let mine = g.st.ids.register_single("srvA", "realitem1").to_string();
    assert_eq!(
        g.post(
            &t,
            &format!("/Sessions/{id}/Playing?PlayCommand=PlayNow&ItemIds={mine}"),
            None
        )
        .await,
        204
    );
    // An item that only exists on another server, or only in a local folder: a clear 400, and nothing was sent.
    let foreign = g.st.ids.register_single("srvB", "other1").to_string();
    let before = seen.0.lock().unwrap().len();
    assert_eq!(
        g.post(
            &t,
            &format!("/Sessions/{id}/Playing?PlayCommand=PlayNow&ItemIds={foreign}"),
            None
        )
        .await,
        400
    );
    assert_eq!(
        g.post(
            &t,
            &format!("/Sessions/{id}/Command"),
            Some(json!({"Name": "Shutdown"}))
        )
        .await,
        400
    );
    assert_eq!(seen.0.lock().unwrap().len(), before);

    let s = seen.0.lock().unwrap();
    let posts: Vec<&(String, String, bool)> = s.iter().filter(|r| r.0 == "POST").collect();
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert!(
        posts.iter().all(|p| p.2),
        "the server's own token, in the header"
    );
    assert!(posts[0].1.starts_with("/Sessions/realsess1/Playing/Pause"));
    assert!(
        posts[1].1.starts_with("/Sessions/realsess1/Playing?")
            && posts[1].1.contains("ItemIds=realitem1")
    );
    assert!(
        !posts[1].1.contains(&mine),
        "gateway ids never go to a server"
    );
    drop(s);
    g.st.stop();
}

#[tokio::test]
async fn with_server_changes_off_a_server_session_is_listed_but_not_controllable() {
    let (url, seen) = mock_server().await;
    let g = start(true, false).await;
    g.st.set_upstreams(vec![upstream(url)]);
    let t = g.login("dev-a", "A").await;
    let list = g.sessions(&t).await;
    let den = list.iter().find(|s| s["DeviceId"] == "mpv-1").unwrap();
    assert_eq!(den["SupportsRemoteControl"], false);
    let id = den["Id"].as_str().unwrap();
    assert_eq!(
        g.post(&t, &format!("/Sessions/{id}/Playing/Pause"), None)
            .await,
        403
    );
    assert_eq!(
        g.post(&t, &format!("/Sessions/{id}/Command/Mute"), None)
            .await,
        403
    );
    assert!(
        seen.0.lock().unwrap().iter().all(|r| r.0 == "GET"),
        "nothing was written to the server"
    );
    g.st.stop();
}

#[tokio::test]
async fn json_bodies_bind_in_any_casing() {
    let g = start(true, false).await;
    // jellyfin-apiclient-python's casing: lower-case Username, Pw
    let r = g
        .http
        .post(g.url("/Users/AuthenticateByName"))
        .header(
            "Authorization",
            r#"MediaBrowser Client="Py", Device="d", DeviceId="py-1", Version="1""#,
        )
        .json(&json!({"username": "nocturne", "Pw": g.pw}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let t = r.json::<Value>().await.unwrap()["AccessToken"]
        .as_str()
        .unwrap()
        .to_string();
    let tb = g.login("dev-b", "B").await;
    let _wb = g.socket(&tb).await;
    let caps = json!({"playablemediatypes": ["Video"], "supportedcommands": ["SetVolume"], "supportsmediacontrol": true});
    assert_eq!(
        g.post(&tb, "/Sessions/Capabilities/Full", Some(caps)).await,
        204
    );
    let list = g.sessions(&t).await;
    let b = list.iter().find(|s| s["DeviceId"] == "dev-b").unwrap();
    assert_eq!(b["SupportsRemoteControl"], true);
    assert_eq!(b["Capabilities"]["SupportedCommands"], json!(["SetVolume"]));
    let id = b["Id"].as_str().unwrap();
    let cmd = format!("/Sessions/{id}/Command");
    assert_eq!(
        g.post(
            &t,
            &cmd,
            Some(json!({"name": "SetVolume", "arguments": {"Volume": "3"}}))
        )
        .await,
        204
    );
    assert_eq!(
        g.post(&t, &cmd, Some(json!({"NAME": "Shutdown"}))).await,
        400
    );
    assert_eq!(
        g.post(
            &t,
            &format!("/Sessions/{id}/Message"),
            Some(json!({"text": "hi"}))
        )
        .await,
        204
    );
    g.st.stop();
}
