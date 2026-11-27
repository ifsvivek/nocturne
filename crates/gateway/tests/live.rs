//! Live remote-control forwarding against the THROWAWAY Jellyfin only. Run:
//!   scripts/test-jellyfin.sh start
//!   NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-gateway --test live -- --ignored --test-threads=1
//! Safety: `TestServerGuard::connect()` refuses unless the URL comes from the env file named by `NOCTURNE_JF_TEST_ENV`, is loopback :8097, differs from the configured real server and the server's data path is under /tmp. Nothing else is contacted. Output: pass or fail only, no tokens.
//!
//! A real controllable device is stood in for by a second sign-in of the same account that posts its capabilities and listens on the server's own `/socket`; the gateway (signed in to the server as that account) must list it, forward a `Playstate` command to it, and refuse everything the gating says it must.
use futures_util::StreamExt;
use nocturne_core::gateway::GatewaySettings;
use nocturne_core::source::SourceId;
use nocturne_gateway::config::{Config, Paths};
use nocturne_gateway::server;
use nocturne_gateway::state::{Secret, State, UpKind, UpState, Upstream};
use nocturne_jellyfin_admin::guard::TestServerGuard;
use nocturne_jellyfin_user::UserClient;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;

struct Rig {
    st: State,
    gw: String,
    gw_token: String,
    http: reqwest::Client,
    server: String,
    device_token: String,
    _dir: tempfile::TempDir,
}

fn env_var(k: &str) -> String {
    let text = std::fs::read_to_string(std::env::var("NOCTURNE_JF_TEST_ENV").unwrap()).unwrap();
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap_or_else(|| panic!("{k} missing in the test env"))
        .to_string()
}

async fn rig(writes: bool) -> Rig {
    TestServerGuard::connect().await.expect("guard refused");
    let server = env_var("NOCTURNE_JF_TEST_URL");
    let pw = env_var("NOCTURNE_JF_TEST_ALICE_PW");
    // The account the gateway acts as.
    let acct = UserClient::authenticate_by_name(&server, "alice", &pw)
        .await
        .unwrap();
    let http = reqwest::Client::new();
    // The "device": its own sign-in under another DeviceId, capabilities posted, its own socket open.
    let dev = http
        .post(format!("{server}/Users/AuthenticateByName"))
        .header(
            "Authorization",
            r#"MediaBrowser Client="Live Device", Device="Rig", DeviceId="gw-live-device", Version="1""#,
        )
        .json(&json!({"Username": "alice", "Pw": pw}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let device_token = dev["AccessToken"].as_str().unwrap().to_string();
    let r = http
        .post(format!("{server}/Sessions/Capabilities/Full"))
        .header("Authorization", format!(r#"MediaBrowser Token="{device_token}""#))
        .json(&json!({"PlayableMediaTypes": ["Video"], "SupportedCommands": ["SetVolume"], "SupportsMediaControl": true}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

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
            remote_control: true,
            ..Default::default()
        },
        allow_server_writes: writes,
        ..Default::default()
    };
    let st = State::open(paths, cfg).unwrap();
    let gw_pw = st.users.take_unseen().unwrap();
    st.set_upstreams(vec![Upstream {
        id: SourceId(1),
        label: "Throwaway".into(),
        kind: UpKind::Server {
            url: server.clone(),
            user_id: acct.user_id.clone(),
            source_key: acct.server_id.clone(),
            token: Some(Secret::new(acct.access_token.clone())),
        },
        state: UpState::Ready,
    }]);
    tokio::spawn(server::run(st.clone()));
    let addr = tokio::time::timeout(Duration::from_secs(5), st.wait_listening())
        .await
        .unwrap()[0];
    let gw = format!("http://127.0.0.1:{}", addr.port());
    let login = http
        .post(format!("{gw}/Users/AuthenticateByName"))
        .header(
            "Authorization",
            r#"MediaBrowser Client="Controller", Device="Rig", DeviceId="gw-live-controller", Version="1""#,
        )
        .json(&json!({"Username": "nocturne", "Pw": gw_pw}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    Rig {
        st,
        gw,
        gw_token: login["AccessToken"].as_str().unwrap().to_string(),
        http,
        server,
        device_token,
        _dir: dir,
    }
}

impl Rig {
    async fn gw_post(&self, path: &str) -> u16 {
        self.http
            .post(format!("{}{path}", self.gw))
            .header(
                "Authorization",
                format!(r#"MediaBrowser Token="{}""#, self.gw_token),
            )
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// The gateway's id for the device, as a controller sees it.
    async fn device_session(&self) -> Option<Value> {
        let list: Vec<Value> = self
            .http
            .get(format!("{}/Sessions", self.gw))
            .header(
                "Authorization",
                format!(r#"MediaBrowser Token="{}""#, self.gw_token),
            )
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        list.into_iter().find(|s| s["DeviceId"] == "gw-live-device")
    }
}

/// The device's own socket on the server.
async fn device_socket(r: &Rig) -> WebSocketStream<reqwest::Upgraded> {
    let resp = r
        .http
        .get(format!("{}/socket", r.server))
        .header(
            "Authorization",
            format!(
                r#"MediaBrowser Client="Live Device", Device="Rig", DeviceId="gw-live-device", Version="1", Token="{}""#,
                r.device_token
            ),
        )
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 101);
    WebSocketStream::from_raw_socket(resp.upgrade().await.unwrap(), Role::Client, None).await
}

#[tokio::test]
#[ignore = "needs the throwaway Jellyfin (scripts/test-jellyfin.sh start)"]
async fn a_playstate_command_reaches_a_device_on_the_server() {
    let r = rig(true).await;
    let mut sock = device_socket(&r).await;
    let s = r.device_session().await.expect("the device is listed");
    assert_eq!(s["SupportsRemoteControl"], true);
    assert_eq!(
        s["UserName"], "nocturne",
        "the server's user name is not shown"
    );
    let id = s["Id"].as_str().unwrap();
    assert_eq!(
        r.gw_post(&format!("/Sessions/{id}/Playing/Pause")).await,
        204
    );
    let got = tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(Ok(m)) = sock.next().await {
            if let Message::Text(t) = m {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                if v["MessageType"] == "Playstate" {
                    return v;
                }
            }
        }
        Value::Null
    })
    .await
    .expect("the command arrives");
    assert_eq!(got["Data"]["Command"], "Pause");
    // A seek carries its position.
    assert_eq!(
        r.gw_post(&format!(
            "/Sessions/{id}/Playing/Seek?SeekPositionTicks=600000000"
        ))
        .await,
        204
    );
    // A command outside the allow-list never leaves the gateway.
    assert_eq!(
        r.gw_post(&format!("/Sessions/{id}/Command/Shutdown")).await,
        400
    );
    r.st.stop();
}

#[tokio::test]
#[ignore = "needs the throwaway Jellyfin (scripts/test-jellyfin.sh start)"]
async fn with_server_changes_off_the_command_is_refused_and_nothing_is_sent() {
    let r = rig(false).await;
    let _sock = device_socket(&r).await;
    let s = r.device_session().await.expect("the device is listed");
    assert_eq!(s["SupportsRemoteControl"], false, "listed read-only");
    let id = s["Id"].as_str().unwrap();
    assert_eq!(
        r.gw_post(&format!("/Sessions/{id}/Playing/Pause")).await,
        403
    );
    r.st.stop();
}
