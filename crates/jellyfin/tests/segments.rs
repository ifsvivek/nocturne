//! Media segments and trickplay reads against a tiny local HTTP fixture (the throwaway server has no
//! segment provider). GET only; the fixture also checks the auth header is sent and no token is in the URL.
use nocturne_jellyfin::{Client, Config, SegmentKind};
use std::io::{Read, Write};
use std::net::TcpListener;

fn serve(routes: Vec<(&'static str, u16, &'static str)>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
            let line = req.lines().next().unwrap_or("").to_string();
            let (code, body) = routes
                .iter()
                .find(|r| line.contains(&r.0.to_lowercase()))
                .map_or((404, ""), |r| (r.1, r.2));
            let ok = req.contains("authorization: mediabrowser") && !line.contains("tok");
            let (code, body) = if ok { (code, body) } else { (401, "") };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://{addr}")
}

fn client(url: String) -> Client {
    Client::new(Config {
        base_url: url,
        token: "tok".into(),
    })
    .unwrap()
}

#[tokio::test]
async fn segments_parse_and_tolerate_missing_api() {
    let url = serve(vec![(
        "/mediasegments/has",
        200,
        r#"{"Items":[{"Type":"Intro","StartTicks":300000000,"EndTicks":900000000},{"Type":"Outro","StartTicks":27000000000,"EndTicks":28000000000}]}"#,
    )]);
    let c = client(url);
    let s = c.media_segments("has").await;
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].kind, SegmentKind::Intro);
    assert_eq!(s[0].start_ticks, 300_000_000);
    assert_eq!(s[1].kind, SegmentKind::Outro);
    assert!(
        c.media_segments("none").await.is_empty(),
        "404 means no segments, not an error"
    );
}

#[tokio::test]
async fn play_item_and_trickplay_tile() {
    let url = serve(vec![
        (
            "/items/x",
            200,
            r#"{"MediaSources":[{"Id":"s","MediaStreams":[{"Index":0,"Type":"Video"}]}],"Trickplay":{"s":{"320":{"Width":320,"Height":180,"TileWidth":10,"TileHeight":10,"ThumbnailCount":5,"Interval":10000}}}}"#,
        ),
        ("/trickplay/320/0.jpg", 200, "JPEG"),
    ]);
    let c = client(url);
    let p = c.play_item("u", "x").await.unwrap();
    assert_eq!(p.trickplay_for(400).unwrap().1.thumbnail_count, 5);
    assert_eq!(c.trickplay_tile("x", 320, 0, "s").await.unwrap(), b"JPEG");
    assert!(c.trickplay_tile("x", 320, 9, "s").await.is_err());
}
