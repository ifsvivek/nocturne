//! Updater phase 2 (docs/dev/UPDATER_PLAN.md): download, verify, swap, restart, for per-user installs only.
//! Order is fixed: size-capped streaming download into a staging file, sha256, ed25519 signature, and only then the
//! install directory is touched. Any failure leaves the running install as it was.
use crate::update::{Artifact, InstallKind};
use aws_lc_rs::signature::{ED25519, UnparsedPublicKey};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

/// Compiled-in update public keys (base64 of the raw 32 bytes). The second slot is for a rotation: ship the next key
/// in a normal release, sign with it later. Empty slots are skipped.
pub const KEYS: [&str; 2] = [
    include_str!("../../../packaging/update/nocturne-update-key.pub"),
    "",
];

/// `<os>-<arch>` as used in the manifest (`linux-x86_64`, `windows-x86_64`, `macos-aarch64`).
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn b64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .ok()
}

/// Raw ed25519 over the artifact bytes, base64 signature; accepted when any non-empty key in `keys` verifies it.
pub fn verify(keys: &[&str], data: &[u8], sig_b64: &str) -> Result<(), String> {
    let sig = b64(sig_b64)
        .filter(|s| s.len() == 64)
        .ok_or("malformed signature")?;
    let ok = keys
        .iter()
        .filter_map(|k| b64(k).filter(|k| k.len() == 32))
        .any(|k| {
            UnparsedPublicKey::new(&ED25519, k)
                .verify(data, &sig)
                .is_ok()
        });
    if ok {
        Ok(())
    } else {
        Err("signature check failed: the download is not an official NOCTURNE build".into())
    }
}

/// Where downloads wait: under the per-user data dir, wiped at every start.
pub fn staging_dir() -> Option<PathBuf> {
    crate::catalog::data_dir().map(|d| d.join("update-staging"))
}

/// `https`, or plain http to the loopback (tests, local mirrors): the signature is what makes the file trustworthy.
fn url_ok(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
}

async fn stream(art: &Artifact, part: &Path, progress: &AtomicU64) -> Result<(), String> {
    let mut r = reqwest::Client::builder()
        .user_agent(format!("NOCTURNE/{}", crate::update::CURRENT))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())?
        .get(&art.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("download failed: {e}"))?;
    if r.content_length().is_some_and(|n| n > art.size) {
        return Err("download is larger than announced".into());
    }
    let mut f = tokio::fs::File::create(part)
        .await
        .map_err(|e| e.to_string())?;
    let (mut hash, mut total) = (Sha256::new(), 0u64);
    while let Some(c) = r
        .chunk()
        .await
        .map_err(|e| format!("download failed: {e}"))?
    {
        total += c.len() as u64;
        if total > art.size {
            return Err("download is larger than announced".into());
        }
        hash.update(&c);
        f.write_all(&c).await.map_err(|e| e.to_string())?;
        progress.store(total, Ordering::Relaxed);
    }
    f.flush().await.map_err(|e| e.to_string())?;
    if total != art.size {
        return Err("download is shorter than announced".into());
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !hex.eq_ignore_ascii_case(art.sha256.trim()) {
        return Err("checksum mismatch".into());
    }
    Ok(())
}

/// Download `art` into `dir`, check size, sha256 and signature, return the verified file. The staging dir is deleted
/// on any failure. Only per-user installs get here; nothing is fetched for the other kinds. Run through
/// `bridge::spawn`; `progress` carries the bytes received so far.
pub async fn fetch_verified(
    kind: InstallKind,
    art: &Artifact,
    keys: &[&str],
    dir: &Path,
    progress: Arc<AtomicU64>,
) -> Result<PathBuf, String> {
    if kind != InstallKind::PerUser {
        return Err("this install is updated by its package manager".into());
    }
    if !url_ok(&art.url) {
        return Err("update url must be https".into());
    }
    let _ = tokio::fs::remove_dir_all(dir).await;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| e.to_string())?;
    let (part, done) = (dir.join("artifact.part"), dir.join("artifact"));
    let res = async {
        stream(art, &part, &progress).await?;
        let data = tokio::fs::read(&part).await.map_err(|e| e.to_string())?;
        verify(keys, &data, &art.signature)?;
        tokio::fs::rename(&part, &done)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_dir_all(dir).await;
        return Err(e);
    }
    Ok(done)
}

/// macOS replaces the whole `.app` bundle containing the executable, everything else the file itself.
fn swap_target(exe: &Path) -> PathBuf {
    if cfg!(target_os = "macos")
        && let Some(app) = exe
            .ancestors()
            .find(|a| a.extension().is_some_and(|e| e == "app"))
    {
        return app.to_path_buf();
    }
    exe.to_path_buf()
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

fn remove_any(p: &Path) {
    let _ = if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    };
}

fn work_dir(target: &Path) -> Option<PathBuf> {
    Some(target.parent()?.join(".nocturne-update"))
}

#[cfg(unix)]
fn writable(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(dir.as_os_str().as_bytes())
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
}
#[cfg(not(unix))]
fn writable(dir: &Path) -> bool {
    std::fs::metadata(dir).is_ok_and(|m| !m.permissions().readonly())
}

/// True when this exe may replace itself: a per-user install whose directory the user can write (a root-owned
/// `/usr/local` tarball install or `/Applications` bundle is not: those show Download).
pub fn can_self_update(kind: InstallKind, exe: &Path) -> bool {
    kind == InstallKind::PerUser && swap_target(exe).parent().is_some_and(writable)
}

/// Pull the new executable (or bundle) out of the verified artifact into `work`, same filesystem as the target.
/// Linux: tar.gz holding `<name>/nocturne`. Windows: zip with `nocturne.exe`. macOS: tar.gz with `NOCTURNE.app`.
fn extract(artifact: &Path, work: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        return extract_app(artifact, work);
    }
    let mut tar = std::process::Command::new("tar");
    if cfg!(windows) {
        tar.arg("-xf")
            .arg(artifact)
            .arg("-C")
            .arg(work)
            .arg("nocturne.exe");
    } else {
        tar.arg("-xzf").arg(artifact).arg("-C").arg(work).args([
            "--strip-components=1",
            "--wildcards",
            "*/nocturne",
        ]);
    }
    let ok = tar
        .output()
        .map_err(|e| format!("tar: {e}"))?
        .status
        .success();
    let new = if cfg!(windows) {
        work.join("nocturne.exe")
    } else {
        work.join("nocturne")
    };
    if !ok || !new.exists() {
        return Err("the update archive has an unexpected layout".into());
    }
    #[cfg(unix)]
    if new.is_file() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(new)
}

/// The optional gateway helper next to the app binary (Linux tarballs ship `<name>/nocturne-gateway`). A second `tar` run because
/// a pattern that matches nothing is an error for tar; an archive without it (older releases, other platforms) is fine.
const GATEWAY: &str = "nocturne-gateway";

fn extract_gateway(artifact: &Path, work: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let dir = work.join("gateway");
    std::fs::create_dir_all(&dir).ok()?;
    let ok = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(artifact)
        .arg("-C")
        .arg(&dir)
        .args(["--strip-components=1", "--wildcards", "*/nocturne-gateway"])
        .output()
        .ok()?
        .status
        .success();
    let new = dir.join(GATEWAY);
    if !ok || !new.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).ok()?;
    }
    Some(new)
}

/// Where the gateway helper lives for an app binary at `target`.
fn gateway_target(target: &Path) -> PathBuf {
    target.with_file_name(GATEWAY)
}

/// Like [`swap`], but a missing target is simply filled.
fn place(target: &Path, new: &Path) -> Result<(), String> {
    if target.exists() {
        swap(target, new)
    } else {
        std::fs::rename(new, target).map_err(|e| e.to_string())
    }
}

/// The macOS artifact: a tar.gz whose top level is `NOCTURNE.app` (scripts/package-macos.sh).
fn extract_app(artifact: &Path, work: &Path) -> Result<PathBuf, String> {
    let ok = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(artifact)
        .arg("-C")
        .arg(work)
        .output()
        .map_err(|e| format!("tar: {e}"))?
        .status
        .success();
    std::fs::read_dir(work)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.extension().is_some_and(|e| e == "app"))
        .filter(|_| ok)
        .ok_or_else(|| "the update archive has an unexpected layout".into())
}

/// Put `new` at `target`, keeping what was there as `<target>.old` until the next successful start.
/// Unix files: hard-link the old one aside, then `rename` over the target (atomic; the running process keeps its
/// inode). Windows files and directories (macOS bundle): rename the old one away, move the new one in, undo on error.
fn swap(target: &Path, new: &Path) -> Result<(), String> {
    let old = with_suffix(target, ".old");
    remove_any(&old);
    let e = |e: std::io::Error| e.to_string();
    if cfg!(unix) && !target.is_dir() {
        std::fs::hard_link(target, &old)
            .or_else(|_| std::fs::copy(target, &old).map(drop))
            .map_err(e)?;
        return std::fs::rename(new, target).map_err(e);
    }
    std::fs::rename(target, &old).map_err(e)?;
    if let Err(err) = std::fs::rename(new, target) {
        let _ = std::fs::rename(&old, target);
        return Err(err.to_string());
    }
    Ok(())
}

/// Unpack the verified artifact and swap it in. Returns the path to start again. Failure leaves the install alone.
pub fn install(kind: InstallKind, artifact: &Path, exe: &Path) -> Result<PathBuf, String> {
    if !can_self_update(kind, exe) {
        return Err("this install cannot update itself".into());
    }
    let target = swap_target(exe);
    let work = work_dir(&target).ok_or("no install directory")?;
    let res = extract(artifact, &work).and_then(|new| {
        // The helper first and best effort: a version bump of it is harmless, and a failure here must not stop the app's own update.
        if let Some(g) = extract_gateway(artifact, &work) {
            let _ = place(&gateway_target(&target), &g);
        }
        swap(&target, &new)
    });
    let _ = std::fs::remove_dir_all(&work);
    res.map(|()| exe.to_path_buf())
}

/// The whole flow for the app: download, verify, then unpack and swap on a blocking thread.
pub async fn update(
    kind: InstallKind,
    art: &Artifact,
    keys: &[&str],
    dir: &Path,
    exe: &Path,
    progress: Arc<AtomicU64>,
) -> Result<PathBuf, String> {
    let file = fetch_verified(kind, art, keys, dir, progress).await?;
    let exe = exe.to_path_buf();
    let res = tokio::task::spawn_blocking(move || install(kind, &file, &exe))
        .await
        .map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_dir_all(dir).await;
    res
}

/// Every start: drop the previous version (the new one is running, so it started), half-done work and old downloads.
pub fn cleanup(exe: &Path) {
    let target = swap_target(exe);
    remove_any(&with_suffix(&target, ".old"));
    remove_any(&with_suffix(&gateway_target(&target), ".old"));
    if let Some(w) = work_dir(&target) {
        let _ = std::fs::remove_dir_all(w);
    }
    if let Some(s) = staging_dir() {
        let _ = std::fs::remove_dir_all(s);
    }
}

static RESTART: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Ask for a restart into `exe` once the app has shut down cleanly (the player is stopped by the normal quit path).
pub fn request_restart(exe: PathBuf) {
    *RESTART.lock().unwrap() = Some(exe);
}

/// Called by `main` after the UI loop returned: start the new version in place of this process (unix `exec`) or
/// next to it (Windows spawn, then this process exits). Returns only if starting failed.
pub fn restart_if_requested() -> Option<std::io::Error> {
    let exe = RESTART.lock().unwrap().take()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Some(cmd.exec())
    }
    #[cfg(not(unix))]
    cmd.spawn().err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};

    fn keypair() -> (Ed25519KeyPair, String) {
        let kp = Ed25519KeyPair::generate().unwrap();
        let pubkey = base64::engine::general_purpose::STANDARD.encode(kp.public_key().as_ref());
        (kp, pubkey)
    }
    fn sign(kp: &Ed25519KeyPair, d: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(kp.sign(d).as_ref())
    }

    #[test]
    fn compiled_key_is_a_valid_32_byte_key() {
        assert_eq!(b64(KEYS[0]).unwrap().len(), 32);
    }

    #[test]
    fn signature_accept_and_reject() {
        let (kp, pubkey) = keypair();
        let (_, other) = keypair();
        let data = b"release bytes".to_vec();
        let sig = sign(&kp, &data);
        assert!(verify(&[&pubkey], &data, &sig).is_ok());
        // rotation slot: the second key verifies, an empty slot is skipped
        assert!(verify(&["", &other, &pubkey], &data, &sig).is_ok());
        assert!(verify(&[&other], &data, &sig).is_err(), "wrong key");
        assert!(verify(&["", ""], &data, &sig).is_err(), "no keys");
        let mut bad = data.clone();
        bad[3] ^= 1;
        assert!(verify(&[&pubkey], &bad, &sig).is_err(), "tampered byte");
        assert!(
            verify(&[&pubkey], &data[..5], &sig).is_err(),
            "truncated data"
        );
        let raw = base64::engine::general_purpose::STANDARD
            .decode(&sig)
            .unwrap();
        let short = base64::engine::general_purpose::STANDARD.encode(&raw[..63]);
        assert!(
            verify(&[&pubkey], &data, &short).is_err(),
            "truncated signature"
        );
        assert!(verify(&[&pubkey], &data, "!!!").is_err());
    }

    #[test]
    fn swap_keeps_old_and_cleanup_removes_it() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("nocturne");
        std::fs::write(&exe, "v1").unwrap();
        let new = d.path().join("new");
        std::fs::write(&new, "v2").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v2");
        assert_eq!(
            std::fs::read_to_string(with_suffix(&exe, ".old")).unwrap(),
            "v1"
        );
        cleanup(&exe);
        assert!(!with_suffix(&exe, ".old").exists());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v2");
    }

    #[test]
    fn swap_failure_leaves_target_untouched() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("nocturne");
        std::fs::write(&exe, "v1").unwrap();
        assert!(swap(&exe, &d.path().join("missing")).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v1");
    }

    #[test]
    fn directory_swap_is_the_macos_and_windows_path() {
        let d = tempfile::tempdir().unwrap();
        let (app, new) = (d.path().join("NOCTURNE.app"), d.path().join("new.app"));
        for (p, v) in [(&app, "v1"), (&new, "v2")] {
            std::fs::create_dir_all(p.join("Contents")).unwrap();
            std::fs::write(p.join("Contents/x"), v).unwrap();
        }
        swap(&app, &new).unwrap();
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/x")).unwrap(),
            "v2"
        );
        assert_eq!(
            std::fs::read_to_string(with_suffix(&app, ".old").join("Contents/x")).unwrap(),
            "v1"
        );
        // a failed move-in restores the original
        let _ = std::fs::remove_dir_all(&new);
        assert!(swap(&app, &new).is_err());
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/x")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn macos_tarball_layout_extracts_and_swaps_the_bundle() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let make = |root: &Path, v: &str| {
            std::fs::create_dir_all(root.join("NOCTURNE.app/Contents/MacOS")).unwrap();
            std::fs::write(root.join("NOCTURNE.app/Contents/MacOS/nocturne"), v).unwrap();
        };
        make(&src, "v2");
        // same command as scripts/package-macos.sh
        let tgz = d.path().join("m.tar.gz");
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(&tgz)
                .arg("-C")
                .arg(&src)
                .arg("NOCTURNE.app")
                .status()
                .unwrap()
                .success()
        );
        let apps = d.path().join("Apps");
        make(&apps, "v1");
        let work = apps.join(".nocturne-update");
        std::fs::create_dir_all(&work).unwrap();
        let new = extract_app(&tgz, &work).unwrap();
        let app = apps.join("NOCTURNE.app");
        swap(&app, &new).unwrap();
        let bin = app.join("Contents/MacOS/nocturne");
        assert_eq!(std::fs::read_to_string(bin).unwrap(), "v2");
        assert_eq!(
            std::fs::read_to_string(with_suffix(&app, ".old").join("Contents/MacOS/nocturne"))
                .unwrap(),
            "v1"
        );
        // a tarball without a bundle is rejected
        std::fs::write(d.path().join("junk"), "junk").unwrap();
        std::fs::create_dir_all(d.path().join("w2")).unwrap();
        assert!(extract_app(&d.path().join("junk"), &d.path().join("w2")).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_from_a_tarball_replaces_the_binary() {
        let d = tempfile::tempdir().unwrap();
        let (src, bin) = (d.path().join("src"), d.path().join("prefix/bin"));
        let name = "nocturne-9.9.9-x86_64-linux";
        std::fs::create_dir_all(src.join(name)).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(src.join(name).join("nocturne"), "new").unwrap();
        std::fs::write(src.join(name).join("install.sh"), "x").unwrap();
        let tgz = d.path().join("b.tar.gz");
        let made = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(&src)
            .arg(name)
            .status()
            .unwrap();
        assert!(made.success());
        let exe = bin.join("nocturne");
        std::fs::write(&exe, "old").unwrap();
        assert_eq!(install(InstallKind::PerUser, &tgz, &exe).unwrap(), exe);
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(with_suffix(&exe, ".old")).unwrap(),
            "old"
        );
        assert!(!bin.join(".nocturne-update").exists(), "work dir cleaned");
        // a garbage archive fails and changes nothing
        std::fs::write(d.path().join("junk"), "junk").unwrap();
        assert!(install(InstallKind::PerUser, &d.path().join("junk"), &exe).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
    }

    /// A release tarball laid out like scripts/package-linux.sh: `<name>/nocturne` and, from 0.7, `<name>/nocturne-gateway`.
    #[cfg(target_os = "linux")]
    fn tarball(d: &Path, with_gateway: bool) -> PathBuf {
        let (src, name) = (d.join("src"), "nocturne-9.9.9-x86_64-linux");
        std::fs::create_dir_all(src.join(name)).unwrap();
        std::fs::write(src.join(name).join("nocturne"), "new app").unwrap();
        if with_gateway {
            std::fs::write(src.join(name).join("nocturne-gateway"), "new gateway").unwrap();
        }
        let tgz = d.join("b.tar.gz");
        let made = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(&src)
            .arg(name)
            .status()
            .unwrap();
        assert!(made.success());
        tgz
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_also_replaces_or_adds_the_gateway_helper_when_the_archive_has_it() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("prefix/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("nocturne");
        std::fs::write(&exe, "old app").unwrap();
        let tgz = tarball(d.path(), true);
        // No helper installed yet (an install from before 0.7): it is added.
        install(InstallKind::PerUser, &tgz, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new app");
        let g = bin.join("nocturne-gateway");
        assert_eq!(std::fs::read_to_string(&g).unwrap(), "new gateway");
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&g).unwrap().permissions().mode() & 0o111, 0o111);
        }
        // An installed one is replaced and the old one kept until the next start, like the app itself.
        std::fs::write(&g, "old gateway").unwrap();
        install(InstallKind::PerUser, &tgz, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&g).unwrap(), "new gateway");
        assert_eq!(
            std::fs::read_to_string(with_suffix(&g, ".old")).unwrap(),
            "old gateway"
        );
        cleanup(&exe);
        assert!(!with_suffix(&g, ".old").exists());
        assert!(!bin.join(".nocturne-update").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_archive_without_the_helper_leaves_it_alone_and_still_updates_the_app() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("prefix/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("nocturne");
        std::fs::write(&exe, "old app").unwrap();
        install(InstallKind::PerUser, &tarball(d.path(), false), &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new app");
        assert!(!bin.join("nocturne-gateway").exists());
        std::fs::write(bin.join("nocturne-gateway"), "mine").unwrap();
        install(InstallKind::PerUser, &tarball(d.path(), false), &exe).unwrap();
        assert_eq!(std::fs::read_to_string(bin.join("nocturne-gateway")).unwrap(), "mine");
    }

    #[test]
    fn system_and_dev_installs_never_install() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("nocturne");
        std::fs::write(&exe, "v1").unwrap();
        for k in [
            InstallKind::Dev,
            InstallKind::SystemPackage(crate::update::Manager::Pacman),
            InstallKind::SystemPackage(crate::update::Manager::Apt),
            InstallKind::SystemPackage(crate::update::Manager::Dnf),
        ] {
            assert!(!can_self_update(k, &exe));
            assert!(install(k, &exe, &exe).is_err());
        }
        assert!(can_self_update(InstallKind::PerUser, &exe));
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v1");
    }
}
