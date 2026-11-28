//! Mounts: `/proc/self/mountinfo` parsing, which mount a root lives on, existing SMB/NFS mounts, user-space SMB mounting through GVfs (`gio mount`), and instructions for what needs the administrator once.
use crate::{Kind, Root};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MOUNTINFO: &str = "/proc/self/mountinfo";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Mount {
    pub point: PathBuf,
    pub fstype: String,
    pub source: String,
}

/// mountinfo escapes space, tab, newline and backslash as `\ooo`.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 3 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 4])
                .ok()
                .and_then(|d| u8::from_str_radix(d, 8).ok())
        {
            out.push(v);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `id parent maj:min root point opts [optional...] - fstype source superopts`.
pub fn parse(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            let sep = f.iter().position(|x| *x == "-")?;
            Some(Mount {
                point: PathBuf::from(unescape(f.get(4)?)),
                fstype: (*f.get(sep + 1)?).to_string(),
                source: unescape(f.get(sep + 2)?),
            })
        })
        .collect()
}

pub fn read(path: &Path) -> Vec<Mount> {
    std::fs::read_to_string(path)
        .map(|t| parse(&t))
        .unwrap_or_default()
}

pub fn is_network(fstype: &str) -> bool {
    matches!(
        fstype,
        "cifs" | "smb3" | "smbfs" | "nfs" | "nfs4" | "fuse.gvfsd-fuse"
    )
}

/// The mount a path is on: the deepest mount point above it (lexically; no file-system call, so a dead share cannot hang this).
pub fn mount_of<'a>(mounts: &'a [Mount], path: &Path) -> Option<&'a Mount> {
    mounts
        .iter()
        .filter(|m| path.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count())
}

/// Whether the root's share or disk is there. A lost mount leaves an empty folder on the parent file system; writing there would fill the wrong disk.
pub fn mounted(root: &Root, mounts: &[Mount]) -> bool {
    match root.kind {
        Kind::Agent => true,
        Kind::Smb | Kind::Nfs => {
            mount_of(mounts, &root.path).is_some_and(|m| is_network(&m.fstype))
        }
        Kind::Local => {
            root.mount_point.as_os_str().is_empty()
                || mount_of(mounts, &root.path).is_some_and(|m| m.point == root.mount_point)
        }
    }
}

/// `(host, path)` of `smb://[user@]host/share/dir`, `//host/share/dir` (cifs source) or `host:/export/dir` (nfs).
pub fn split_remote(s: &str) -> Option<(String, PathBuf)> {
    let (host, path) = if let Some(r) = s.strip_prefix("smb://").or_else(|| s.strip_prefix("//")) {
        let (h, p) = r.split_once('/').unwrap_or((r, ""));
        (h, format!("/{p}"))
    } else {
        let (h, p) = s.split_once(':')?;
        if !p.starts_with('/') {
            return None;
        }
        (h, p.to_string())
    };
    let host = host.rsplit('@').next()?.to_ascii_lowercase();
    (!host.is_empty()).then(|| (host, PathBuf::from(path.trim_end_matches('/'))))
}

/// Where an SMB or NFS URL is already mounted (by fstab, autofs, or by hand), and the folder of the URL in it.
pub fn find_existing(mounts: &[Mount], url: &str) -> Option<PathBuf> {
    let (host, want) = split_remote(url)?;
    mounts
        .iter()
        .filter(|m| is_network(&m.fstype))
        .find_map(|m| {
            let (h, src) = split_remote(&m.source)?;
            let rest = want.strip_prefix(&src).ok()?;
            (h == host).then(|| m.point.join(rest))
        })
}

/// GVfs's folder for an SMB share under `$XDG_RUNTIME_DIR/gvfs` (matched without regard to case, as GVfs may normalise it), plus the URL's sub-folder.
pub fn gvfs_path(runtime: &Path, url: &str) -> Option<PathBuf> {
    let (host, path) = split_remote(url)?;
    let mut parts = path.components().skip(1);
    let share = parts.next()?.as_os_str().to_str()?.to_string();
    let rest: PathBuf = parts.collect();
    let gvfs = runtime.join("gvfs");
    let name = format!("smb-share:server={host},share={share}");
    let found = std::fs::read_dir(&gvfs).ok().and_then(|d| {
        d.flatten()
            .map(|e| e.file_name())
            .find(|n| n.to_str().is_some_and(|n| n.eq_ignore_ascii_case(&name)))
    });
    Some(
        gvfs.join(found.map_or(name.into(), PathBuf::from))
            .join(rest),
    )
}

pub struct SmbLogin<'a> {
    pub user: &'a str,
    pub domain: &'a str,
    pub password: &'a str,
}

/// Mount an SMB share in user space with `gio mount` (GVfs; no root). The login answers gio's prompts on stdin (user, domain, password), so it never appears in a process list. `Ok` also when it is mounted already.
pub async fn gio_mount(gio: &Path, url: &str, login: Option<SmbLogin<'_>>) -> Result<(), String> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    if !url.starts_with("smb://") || url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("an SMB address looks like smb://host/share".into());
    }
    if let Some(l) = &login
        && [l.user, l.domain, l.password]
            .iter()
            .any(|s| s.contains(['\n', '\r']))
    {
        return Err("the login must not contain line breaks".into());
    }
    let mut cmd = tokio::process::Command::new(gio);
    cmd.arg("mount");
    if login.is_none() {
        cmd.arg("--anonymous");
    }
    cmd.arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", gio.display()))?;
    if let (Some(l), Some(mut stdin)) = (login, child.stdin.take()) {
        let answers = format!("{}\n{}\n{}\n", l.user, l.domain, l.password);
        let _ = stdin.write_all(answers.as_bytes()).await;
    }
    let out = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .map_err(|_| "gio mount did not finish within a minute".to_string())?
        .map_err(|e| e.to_string())?;
    let err = String::from_utf8_lossy(&out.stderr);
    if out.status.success() || err.contains("already mounted") {
        Ok(())
    } else {
        Err(format!("gio mount failed: {}", err.trim()))
    }
}

/// What the administrator does once so the share is mounted without NOCTURNE needing root: an fstab line with an automount, then the mount point as the root's path.
pub fn instructions(kind: Kind, url: &str) -> String {
    // SAFETY: getuid/getgid have no preconditions.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let name = split_remote(url)
        .and_then(|(_, p)| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "share".into());
    match kind {
        Kind::Smb => {
            let src = url.strip_prefix("smb:").unwrap_or(url);
            format!(
                "NOCTURNE mounts SMB shares itself through GVfs (install gvfs-smb). Without it, add this line to /etc/fstab as root (put username=, password= and domain= in /etc/nocturne-{name}.cred, mode 600), then `sudo mkdir -p /mnt/{name}` and use /mnt/{name} as this root's path:\n{src}  /mnt/{name}  cifs  credentials=/etc/nocturne-{name}.cred,uid={uid},gid={gid},noauto,x-systemd.automount,_netdev  0 0"
            )
        }
        Kind::Nfs => format!(
            "NFS needs root to mount. Add this line to /etc/fstab as root, then `sudo mkdir -p /mnt/{name}` and use /mnt/{name} as this root's path (it mounts on first access):\n{url}  /mnt/{name}  nfs  noauto,x-systemd.automount,_netdev  0 0"
        ),
        Kind::Local | Kind::Agent => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "\
22 1 0:21 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
30 22 0:40 / /mnt/usb rw,relatime shared:5 - exfat /dev/sdb1 rw
31 22 0:41 / /mnt/nas rw,relatime shared:6 - cifs //NAS.local/media rw,vers=3.0
32 22 0:42 / /srv/nfs\\040data rw shared:7 master:2 - nfs4 box:/export rw
33 22 0:43 / /run/user/1000/gvfs rw,nosuid shared:8 - fuse.gvfsd-fuse gvfsd-fuse rw
";

    #[test]
    fn parses_mountinfo_with_escapes_and_optional_fields() {
        let m = parse(INFO);
        assert_eq!(m.len(), 5);
        assert_eq!(m[3].point, Path::new("/srv/nfs data"));
        assert_eq!(
            (m[3].fstype.as_str(), m[3].source.as_str()),
            ("nfs4", "box:/export")
        );
        assert_eq!(
            mount_of(&m, Path::new("/mnt/nas/movies")).unwrap().fstype,
            "cifs"
        );
        assert_eq!(
            mount_of(&m, Path::new("/mnt/nasty")).unwrap().point,
            Path::new("/")
        );
    }

    #[test]
    fn finds_existing_shares() {
        let m = parse(INFO);
        assert_eq!(
            find_existing(&m, "smb://user@nas.local/media/Movies"),
            Some("/mnt/nas/Movies".into())
        );
        assert_eq!(
            find_existing(&m, "box:/export/tv/"),
            Some("/srv/nfs data/tv".into())
        );
        assert_eq!(find_existing(&m, "smb://other/media"), None);
        assert_eq!(find_existing(&m, "box:/exported"), None);
    }

    #[test]
    fn lost_mounts_are_noticed() {
        let m = parse(INFO);
        let gone = parse("22 1 0:21 / / rw - ext4 /dev/sda1 rw\n");
        let smb = Root {
            kind: Kind::Smb,
            path: "/mnt/nas/Movies".into(),
            ..Root::default()
        };
        let usb = Root {
            kind: Kind::Local,
            path: "/mnt/usb/tv".into(),
            mount_point: "/mnt/usb".into(),
            ..Root::default()
        };
        assert!(mounted(&smb, &m) && mounted(&usb, &m));
        assert!(!mounted(&smb, &gone) && !mounted(&usb, &gone));
    }

    #[test]
    fn gvfs_folder() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            gvfs_path(d.path(), "smb://NAS/media/films").unwrap(),
            d.path().join("gvfs/smb-share:server=nas,share=media/films")
        );
        std::fs::create_dir_all(d.path().join("gvfs/smb-share:server=nas,share=Media")).unwrap();
        assert_eq!(
            gvfs_path(d.path(), "smb://nas/media").unwrap(),
            d.path().join("gvfs/smb-share:server=nas,share=Media")
        );
    }

    #[test]
    fn instructions_name_the_share() {
        assert!(instructions(Kind::Nfs, "box:/export/tv").contains("box:/export/tv  /mnt/tv  nfs"));
        assert!(
            instructions(Kind::Smb, "smb://nas/media").contains("//nas/media  /mnt/media  cifs")
        );
    }

    /// A stand-in `gio` that records its arguments and stdin.
    #[tokio::test]
    async fn gio_mount_sends_the_login_on_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let gio = d.path().join("gio");
        let log = d.path().join("log");
        std::fs::write(
            &gio,
            format!(
                "#!/bin/sh\necho \"$@\" > '{0}'\ncat >> '{0}'\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&gio, std::fs::Permissions::from_mode(0o755)).unwrap();
        let login = SmbLogin {
            user: "u",
            domain: "WG",
            password: "pw",
        };
        gio_mount(&gio, "smb://nas/media", Some(login))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "mount smb://nas/media\nu\nWG\npw\n"
        );
        gio_mount(&gio, "smb://nas/media", None).await.unwrap();
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .starts_with("mount --anonymous")
        );
        assert!(gio_mount(&gio, "-x smb://nas", None).await.is_err());
        std::fs::write(&gio, "#!/bin/sh\necho 'no such share' >&2\nexit 2\n").unwrap();
        let e = gio_mount(&gio, "smb://nas/media", None).await.unwrap_err();
        assert!(e.contains("no such share"));
    }
}
