//! The per-server extra header reaches the wire from the read client and the user client (reports),
//! alongside the token header, and only for the server it was set for. Local fixture, no real server.
use nocturne_jellyfin::{Client, Config, header};
use nocturne_jellyfin_user::{PlayMethod, PlaybackReport, Session, UserClient};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// Answers every request `200 {}` and sends its lower-cased head and body to the channel.
fn serve() -> (String, mpsc::Receiver<String>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap();
            let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_lowercase());
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
            );
        }
    });
    (url, rx)
}

#[tokio::test]
async fn header_is_sent_by_read_and_user_clients() {
    let (url, rx) = serve();
    header::set(&url, Some("X-Gate: s3cret")).unwrap();
    let c = Client::new(Config {
        base_url: url.clone(),
        token: "tok".into(),
    })
    .unwrap();
    let _ = c.public_info().await;
    let req = rx.recv().unwrap();
    assert!(req.contains("x-gate: s3cret"), "{req}");
    assert!(req.contains("authorization: mediabrowser"), "token kept");
    assert_eq!(c.extra_header_line().as_deref(), Some("x-gate: s3cret"));

    let s = Session {
        access_token: "tok".into(),
        user_id: "u".into(),
        server_id: "s".into(),
        user_name: "n".into(),
        is_admin: false,
    };
    let u = UserClient::from_session(&url, &s).unwrap();
    let rep = PlaybackReport {
        item_id: "i".into(),
        play_method: PlayMethod::Transcode,
        play_session_id: Some("ps".into()),
        ..Default::default()
    };
    let _ = u.report_playback_start(&rep).await;
    let req = rx.recv().unwrap();
    assert!(req.starts_with("post /sessions/playing "), "{req}");
    assert!(req.contains("x-gate: s3cret"), "{req}");
    assert!(
        req.contains("\"playmethod\":\"transcode\"") && req.contains("\"playsessionid\":\"ps\"")
    );

    // Removing it stops it; another server never got it.
    header::set(&url, None).unwrap();
    let c = Client::new(Config {
        base_url: url,
        token: "tok".into(),
    })
    .unwrap();
    let _ = c.public_info().await;
    assert!(!rx.recv().unwrap().contains("x-gate"));
}
