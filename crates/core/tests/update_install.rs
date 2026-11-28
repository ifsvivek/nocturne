//! Updater phase 2 end to end on loopback: a manifest and a signed artifact made by the release scripts with a
//! temporary key, downloaded, verified, swapped into a temp "per-user" prefix, and the new binary started.
//! No network beyond 127.0.0.1, no real key. Linux x86_64 only (the artifact name carries the platform).
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use nocturne_core::update::{self, Action, InstallKind, Manager, UpdateState};
use nocturne_core::update_install as ui;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn repo() -> PathBuf {
    Path::new(std::env::var("CARGO_MANIFEST_DIR").unwrap().as_str()).join("../..")
}

fn run(cmd: &mut Command) -> String {
    let o = cmd.output().unwrap();
    assert!(
        o.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

/// A throwaway ed25519 key (openssl) and its raw public half in base64, the format of the compiled-in key.
fn make_key(dir: &Path) -> (PathBuf, String) {
    let pem = dir.join("test-key.pem");
    run(Command::new("openssl")
        .args(["genpkey", "-algorithm", "ed25519", "-out"])
        .arg(&pem));
    let der = Command::new("openssl")
        .args(["pkey", "-pubout", "-outform", "DER", "-in"])
        .arg(&pem)
        .output()
        .unwrap()
        .stdout;
    // SPKI DER of ed25519: 12 bytes of header, then the 32 raw key bytes.
    let raw = &der[der.len() - 32..];
    use base64::Engine;
    (pem, base64::engine::general_purpose::STANDARD.encode(raw))
}

/// Serves fixed bodies by path on 127.0.0.1; `hits` counts requests. `no_len` omits Content-Length.
struct Server {
    port: u16,
    hits: Arc<AtomicU64>,
}

async fn serve(routes: Vec<(String, Vec<u8>)>, no_len: bool) -> Server {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(AtomicU64::new(0));
    let h = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let (routes, h) = (routes.clone(), h.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                h.fetch_add(1, Ordering::Relaxed);
                match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let head = if no_len {
                            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
                        } else {
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                        };
                        let _ = s.write_all(head.as_bytes()).await;
                        let _ = s.write_all(body).await;
                    }
                    None => {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    }
                }
                let _ = s.shutdown().await;
            });
        }
    });
    Server { port, hits }
}

struct Release {
    dir: tempfile::TempDir,
    pubkey: String,
    tarball: PathBuf,
}

/// A fake release: a tarball like scripts/package.sh makes (a directory holding a `nocturne` script that prints
/// its version), signed and described by the real release scripts.
fn release(version: &str) -> Release {
    let dir = tempfile::tempdir().unwrap();
    let (pem, pubkey) = make_key(dir.path());
    let name = format!("nocturne-{version}-x86_64-linux");
    let top = dir.path().join("src").join(&name);
    std::fs::create_dir_all(&top).unwrap();
    std::fs::write(
        top.join("nocturne"),
        format!("#!/bin/sh\necho started {version} \"$@\"\n"),
    )
    .unwrap();
    std::fs::write(top.join("install.sh"), "#!/bin/sh\n").unwrap();
    let tarball = dir.path().join(format!("{name}.tar.gz"));
    run(Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(dir.path().join("src"))
        .arg(&name));
    run(Command::new(repo().join("scripts/sign-update.sh"))
        .env("UPDATE_KEY", &pem)
        .arg(&tarball));
    Release {
        dir,
        pubkey,
        tarball,
    }
}

fn manifest(r: &Release, base: &str, version: &str) -> Vec<u8> {
    let out = r.dir.path().join("stable.json");
    run(Command::new(repo().join("scripts/make-update-manifest.sh"))
        .env("OUT", &out)
        .env("RELEASE_BASE", base)
        .arg(version)
        .arg("https://example.org/notes")
        .arg(&r.tarball));
    std::fs::read(out).unwrap()
}

fn old_install(root: &Path) -> PathBuf {
    let bin = root.join("prefix/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("nocturne");
    std::fs::write(&exe, "#!/bin/sh\necho started 0.0.1\n").unwrap();
    std::fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    exe
}

fn artifact(m: &[u8]) -> update::Artifact {
    update::parse_manifest(m).unwrap().platforms["linux-x86_64"].clone()
}

#[tokio::test]
async fn download_verify_swap_restart() {
    let r = release("9.9.9");
    let tar_bytes = std::fs::read(&r.tarball).unwrap();
    let name = r.tarball.file_name().unwrap().to_string_lossy().to_string();
    let srv = serve(vec![(format!("/{name}"), tar_bytes.clone())], false).await;
    let base = format!("http://127.0.0.1:{}", srv.port);
    let m = manifest(&r, &base, "9.9.9");
    let parsed = update::parse_manifest(&m).unwrap();
    assert_eq!(artifact(&m).size, tar_bytes.len() as u64);

    let root = tempfile::tempdir().unwrap();
    let exe = old_install(root.path());
    // the banner's decision for a per-user install in a writable prefix
    assert!(ui::can_self_update(InstallKind::PerUser, &exe));
    let UpdateState::Available {
        action: Action::Install(art),
        ..
    } = update::decide("0.0.1", &parsed, InstallKind::PerUser, true)
    else {
        panic!("expected an in-app install")
    };

    let staging = root.path().join("staging");
    let progress = Arc::new(AtomicU64::new(0));
    let file = ui::fetch_verified(
        InstallKind::PerUser,
        &art,
        &[&r.pubkey],
        &staging,
        progress.clone(),
    )
    .await
    .unwrap();
    assert_eq!(progress.load(Ordering::Relaxed), tar_bytes.len() as u64);
    assert_eq!(std::fs::read(&file).unwrap(), tar_bytes);

    let again = ui::install(InstallKind::PerUser, &file, &exe).unwrap();
    assert_eq!(again, exe);
    // what `restart_if_requested` execs: the same path, now the new version, with the original arguments
    assert_eq!(
        run(Command::new(&again).arg("--x")).trim(),
        "started 9.9.9 --x"
    );
    let old = exe.with_file_name("nocturne.old");
    assert_eq!(run(&mut Command::new(&old)).trim(), "started 0.0.1");

    // next start: the old version and the staging directory go
    ui::cleanup(&exe);
    assert!(!old.exists());
    assert_eq!(run(&mut Command::new(&exe)).trim(), "started 9.9.9");
}

fn art_with(a: &update::Artifact, f: impl FnOnce(&mut update::Artifact)) -> update::Artifact {
    let mut a = a.clone();
    f(&mut a);
    a
}

#[tokio::test]
async fn bad_downloads_are_rejected_and_leave_nothing() {
    let r = release("9.9.9");
    let good = std::fs::read(&r.tarball).unwrap();
    let name = r.tarball.file_name().unwrap().to_string_lossy().to_string();
    let path = format!("/{name}");
    let srv = serve(vec![(path.clone(), good.clone())], false).await;
    let base = format!("http://127.0.0.1:{}", srv.port);
    let art = artifact(&manifest(&r, &base, "9.9.9"));
    let root = tempfile::tempdir().unwrap();
    let staging = root.path().join("staging");
    let go = |a: update::Artifact, keys: Vec<String>| {
        let staging = staging.clone();
        async move {
            let k: Vec<&str> = keys.iter().map(String::as_str).collect();
            ui::fetch_verified(InstallKind::PerUser, &a, &k, &staging, Arc::default()).await
        }
    };
    let k = vec![r.pubkey.clone()];

    // sha256 mismatch
    let e = go(art_with(&art, |a| a.sha256 = "0".repeat(64)), k.clone())
        .await
        .unwrap_err();
    assert!(e.contains("checksum"), "{e}");
    assert!(!staging.exists(), "staging cleaned after a failure");
    // announced size smaller than the body (Content-Length) and larger than the body
    let e = go(art_with(&art, |a| a.size -= 1), k.clone())
        .await
        .unwrap_err();
    assert!(e.contains("larger"), "{e}");
    let e = go(art_with(&art, |a| a.size += 1), k.clone())
        .await
        .unwrap_err();
    assert!(e.contains("shorter"), "{e}");
    // wrong key
    let (_, other) = make_key(root.path());
    let e = go(art.clone(), vec![other, String::new()])
        .await
        .unwrap_err();
    assert!(e.contains("signature"), "{e}");
    // signature of a different file
    let e = go(
        art_with(&art, |a| a.signature = "A".repeat(86) + "=="),
        k.clone(),
    )
    .await
    .unwrap_err();
    assert!(e.contains("signature"), "{e}");
    // not https, not loopback
    let e = go(
        art_with(&art, |a| a.url = "http://example.org/a".into()),
        k.clone(),
    )
    .await
    .unwrap_err();
    assert!(e.contains("https"), "{e}");
    // 404
    assert!(
        go(
            art_with(&art, |a| a.url = format!("{base}/nope")),
            k.clone()
        )
        .await
        .is_err()
    );
    assert!(!staging.exists());

    // a tampered artifact whose size and sha256 were updated to match: only the signature catches it
    let mut bad = good.clone();
    let n = bad.len() / 2;
    bad[n] ^= 1;
    let srv2 = serve(vec![(path, bad.clone())], false).await;
    let forged = art_with(&art, |a| {
        a.url = format!("http://127.0.0.1:{}/{name}", srv2.port);
        a.size = bad.len() as u64;
        a.sha256 = Sha256::digest(&bad)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
    });
    let e = go(forged, k.clone()).await.unwrap_err();
    assert!(e.contains("signature"), "{e}");
    assert!(!staging.exists());

    // a body that outgrows its announced size with no Content-Length: stopped by the running cap
    let srv3 = serve(vec![(format!("/{name}"), good.clone())], true).await;
    let capped = art_with(&art, |a| {
        a.url = format!("http://127.0.0.1:{}/{name}", srv3.port);
        a.size -= 10;
    });
    let e = go(capped, k.clone()).await.unwrap_err();
    assert!(e.contains("larger"), "{e}");

    // and the good one still works after all of that
    assert!(go(art, k).await.is_ok());
}

#[tokio::test]
async fn system_and_dev_installs_never_download() {
    let r = release("9.9.9");
    let srv = serve(vec![], false).await;
    let art = artifact(&manifest(
        &r,
        &format!("http://127.0.0.1:{}", srv.port),
        "9.9.9",
    ));
    let dir = tempfile::tempdir().unwrap();
    for k in [
        InstallKind::Dev,
        InstallKind::SystemPackage(Manager::Pacman),
        InstallKind::SystemPackage(Manager::Apt),
        InstallKind::SystemPackage(Manager::Dnf),
    ] {
        let e =
            ui::fetch_verified(k, &art, &[&r.pubkey], &dir.path().join("s"), Arc::default()).await;
        assert!(e.is_err());
    }
    assert_eq!(srv.hits.load(Ordering::Relaxed), 0, "no request was made");
    assert!(!dir.path().join("s").exists());
}

#[test]
fn failed_install_leaves_the_current_one_running() {
    let r = release("9.9.9");
    let root = tempfile::tempdir().unwrap();
    let exe = old_install(root.path());
    let junk = root.path().join("artifact");
    std::fs::write(&junk, "not a tarball").unwrap();
    assert!(ui::install(InstallKind::PerUser, &junk, &exe).is_err());
    assert_eq!(run(&mut Command::new(&exe)).trim(), "started 0.0.1");
    assert!(!exe.with_file_name("nocturne.old").exists());
    drop(r);
}
