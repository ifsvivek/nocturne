//! Transfers against a loopback HTTP server that speaks just enough of the Download endpoint.

use super::*;
use crate::download::{Meta, State};
use crate::source::SourceId;
use nocturne_jellyfin::Config;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    IgnoreRange,
    Deny,
    /// The first answer stops after this many body bytes.
    CutAt(usize),
}

struct Mock {
    url: String,
    ranges: Arc<Mutex<Vec<Option<u64>>>>,
}

async fn serve(data: Vec<u8>, mode: Mode) -> Mock {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let ranges = Arc::new(Mutex::new(vec![]));
    let seen = ranges.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (data, seen) = (data.clone(), seen.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                if !req.starts_with("GET /Items/abc/Download") {
                    let _ = s
                        .write_all(b"HTTP/1.1 404 No\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    return;
                }
                let from = req
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .map(String::from)
                    })
                    .and_then(|r| r.trim_end_matches('-').parse::<u64>().ok());
                let first = {
                    let mut g = seen.lock().unwrap();
                    g.push(from);
                    g.len() == 1
                };
                if let Mode::Deny = mode {
                    let _ = s
                        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    return;
                }
                let start = match (mode, from) {
                    (Mode::IgnoreRange, _) | (_, None) => 0,
                    (_, Some(f)) => f as usize,
                };
                let body = &data[start..];
                let status = if start > 0 {
                    "206 Partial Content"
                } else {
                    "200 OK"
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"Heat.mkv\"\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let send = match mode {
                    Mode::CutAt(n) if first => &body[..n.min(body.len())],
                    _ => body,
                };
                let _ = s.write_all(send).await;
            });
        }
    });
    Mock { url, ranges }
}

fn client(url: &str) -> Client {
    Client::new(Config {
        base_url: url.into(),
        token: "t".into(),
    })
    .unwrap()
}

fn job(root: &Path) -> Job {
    Job {
        id: 1,
        origin: SourceId(2),
        item: "abc".into(),
        meta: Meta {
            title: "Heat".into(),
            year: Some(1995),
            ..Default::default()
        },
        root: root.into(),
        into: SourceId(1),
        dest: None,
        done: 0,
        total: 0,
        state: State::Queued,
    }
}

fn bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn a_download_lands_under_its_name_with_an_nfo_and_no_part() {
    let t = tempfile::tempdir().unwrap();
    let data = bytes(300_000);
    let m = serve(data.clone(), Mode::Normal).await;
    let sh = Shared::default();
    let r = run(&client(&m.url), "u", &job(t.path()), &sh).await;
    assert_eq!(r, Ok(End::Done));
    let dest = t.path().join("Heat (1995)/Heat (1995).mkv");
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert!(!naming::part_of(&dest).exists());
    assert!(
        std::fs::read_to_string(dest.with_extension("nfo"))
            .unwrap()
            .contains("<title>Heat</title>")
    );
    assert_eq!(sh.done.load(Ordering::Relaxed), 300_000);
    assert_eq!(sh.dest.lock().unwrap().as_deref(), Some(dest.as_path()));
}

#[tokio::test]
async fn an_interrupted_download_resumes_with_a_range_request() {
    let t = tempfile::tempdir().unwrap();
    let data = bytes(300_000);
    let m = serve(data.clone(), Mode::CutAt(100_000)).await;
    let c = client(&m.url);
    let mut j = job(t.path());
    let sh = Shared::default();
    assert_eq!(run(&c, "u", &j, &sh).await, Err(Why::Network));
    let dest = sh.dest.lock().unwrap().clone().unwrap();
    let part = naming::part_of(&dest);
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 100_000);
    assert!(!dest.exists(), "nothing is published until it is whole");
    j.dest = Some(dest.clone());
    let sh = Shared::default();
    assert_eq!(run(&c, "u", &j, &sh).await, Ok(End::Done));
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert_eq!(*m.ranges.lock().unwrap(), vec![None, Some(100_000)]);
}

#[tokio::test]
async fn a_server_that_ignores_range_restarts_the_file() {
    let t = tempfile::tempdir().unwrap();
    let data = bytes(50_000);
    let m = serve(data.clone(), Mode::IgnoreRange).await;
    let mut j = job(t.path());
    let dest = t.path().join("Heat (1995)/Heat (1995).mkv");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(naming::part_of(&dest), b"stale bytes").unwrap();
    j.dest = Some(dest.clone());
    assert_eq!(
        run(&client(&m.url), "u", &j, &Shared::default()).await,
        Ok(End::Done)
    );
    assert_eq!(std::fs::read(&dest).unwrap(), data, "no stale prefix");
}

#[tokio::test]
async fn no_download_permission_is_reported_as_denied() {
    let t = tempfile::tempdir().unwrap();
    let m = serve(vec![], Mode::Deny).await;
    let r = run(&client(&m.url), "u", &job(t.path()), &Shared::default()).await;
    assert_eq!(r, Err(Why::Denied));
    assert!(
        std::fs::read_dir(t.path()).unwrap().next().is_none(),
        "nothing was created"
    );
    let st = |status| Error::Status {
        status,
        path: String::new(),
    };
    assert_eq!(why(&st(404)), Why::Missing);
    assert_eq!(why(&st(401)), Why::Auth);
}

#[tokio::test]
async fn an_existing_file_is_never_overwritten() {
    let t = tempfile::tempdir().unwrap();
    let existing = t.path().join("Heat (1995)/Heat (1995).mkv");
    std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
    std::fs::write(&existing, b"mine").unwrap();
    let m = serve(bytes(1000), Mode::Normal).await;
    let sh = Shared::default();
    assert_eq!(
        run(&client(&m.url), "u", &job(t.path()), &sh).await,
        Ok(End::Done)
    );
    assert_eq!(std::fs::read(&existing).unwrap(), b"mine");
    let dest = sh.dest.lock().unwrap().clone().unwrap();
    assert_eq!(dest, t.path().join("Heat (1995)/Heat (1995) - 2.mkv"));
    assert_eq!(std::fs::read(dest).unwrap().len(), 1000);
}

#[test]
fn publish_skips_a_name_taken_in_the_meantime() {
    let t = tempfile::tempdir().unwrap();
    let (part, dest) = (t.path().join("a.mkv.part"), t.path().join("a.mkv"));
    std::fs::write(&part, b"new").unwrap();
    std::fs::write(&dest, b"old").unwrap();
    let to = publish(&part, &dest).unwrap();
    assert_eq!(to, t.path().join("a - 2.mkv"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    assert!(!part.exists());
}

#[tokio::test]
async fn pause_keeps_the_part_and_cancel_removes_only_it() {
    let t = tempfile::tempdir().unwrap();
    let m = serve(bytes(1000), Mode::Normal).await;
    let sh = Shared::default();
    sh.ctl.store(PAUSE, Ordering::Relaxed);
    assert_eq!(
        run(&client(&m.url), "u", &job(t.path()), &sh).await,
        Ok(End::Paused)
    );
    let dest = sh.dest.lock().unwrap().clone().unwrap();
    assert!(naming::part_of(&dest).exists());
    let mut j = job(t.path());
    j.dest = Some(dest.clone());
    sh.ctl.store(CANCEL, Ordering::Relaxed);
    assert_eq!(run(&client(&m.url), "u", &j, &sh).await, Ok(End::Cancelled));
    assert!(!naming::part_of(&dest).exists());
    assert!(dest.parent().unwrap().exists(), "the folder is left alone");
}
