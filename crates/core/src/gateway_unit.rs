//! "Keep running when NOCTURNE is closed": the systemd user unit that starts `nocturne-gateway` at login (docs/dev/GATEWAY_PLAN.md section 5, decision 5). Linux only; launchd and a Windows login task come with those platforms.
//!
//! A user unit starts when the user logs in, which is also when the Secret Service is available; `loginctl enable-linger` is not supported because the keyring would be locked. The text and the file handling are pure and tested; `enable` / `disable` call `systemctl --user`.
use std::io;
use std::path::{Path, PathBuf};

pub const UNIT_NAME: &str = "nocturne-gateway.service";

/// `$XDG_CONFIG_HOME/systemd/user`.
pub fn unit_dir() -> Option<PathBuf> {
    nocturne_session::paths::config_dir()?
        .parent()
        .map(|p| p.join("systemd").join("user"))
}

/// A path as one `ExecStart=` / `ReadWritePaths=` word: quoted, with the characters systemd would expand escaped.
fn quote(p: &Path) -> io::Result<String> {
    let s = p
        .to_str()
        .filter(|s| !s.contains(['\n', '\r', '\0']))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "path is not usable in a unit file",
            )
        })?;
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    ))
}

/// The unit file. `writable` are the directories the gateway writes to (its data, the image cache); each is optional (`-`) so a first start before they exist does not fail.
pub fn unit_text(exe: &Path, writable: &[&Path]) -> io::Result<String> {
    let mut rw = String::from("ReadWritePaths=-%t/nocturne");
    for p in writable {
        rw.push_str(" -");
        rw.push_str(&quote(p)?.trim_matches('"').replace(' ', "\\x20"));
    }
    Ok(format!(
        "[Unit]\n\
         Description=NOCTURNE gateway (one Jellyfin-compatible address over your sources)\n\
         Documentation=https://github.com/ifsvivek/nocturne\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         NoNewPrivileges=yes\n\
         ProtectSystem=strict\n\
         ProtectHome=read-only\n\
         {rw}\n\
         PrivateTmp=yes\n\
         RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exec = quote(exe)?,
    ))
}

/// Write the unit into `dir` (created if missing). Returns its path.
pub fn write_unit(dir: &Path, exe: &Path, writable: &[&Path]) -> io::Result<PathBuf> {
    let text = unit_text(exe, writable)?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(UNIT_NAME);
    let tmp = dir.join(format!("{UNIT_NAME}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Delete the unit from `dir`; true if there was one.
pub fn remove_unit(dir: &Path) -> io::Result<bool> {
    match std::fs::remove_file(dir.join(UNIT_NAME)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn systemctl(args: &[&str]) -> io::Result<()> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Install and start the unit: written to the user's unit directory, then enabled now and at every login. Blocking; run it off the UI thread.
pub fn enable(exe: &Path) -> io::Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(io::Error::other("Keep running is available on Linux only"));
    }
    let dir = unit_dir().ok_or_else(|| io::Error::other("no user config directory"))?;
    let data = nocturne_session::paths::data_dir().map(|d| d.join("gateway"));
    let cache = nocturne_session::paths::cache_dir();
    let writable: Vec<&Path> = data
        .iter()
        .chain(cache.iter())
        .map(PathBuf::as_path)
        .collect();
    write_unit(&dir, exe, &writable)?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", UNIT_NAME])
}

/// Stop and remove the unit. Missing is fine.
pub fn disable() -> io::Result<()> {
    let Some(dir) = unit_dir() else {
        return Ok(());
    };
    if dir.join(UNIT_NAME).exists() {
        let _ = systemctl(&["disable", "--now", UNIT_NAME]);
        remove_unit(&dir)?;
        let _ = systemctl(&["daemon-reload"]);
    }
    Ok(())
}

/// Whether the unit file is in place (what the Settings row reflects).
pub fn installed() -> bool {
    unit_dir().is_some_and(|d| d.join(UNIT_NAME).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_has_the_hardening_and_the_quoted_binary() {
        let t = unit_text(
            Path::new("/opt/My Apps/nocturne-gateway"),
            &[Path::new("/home/u/.local/share/nocturne/gateway")],
        )
        .unwrap();
        assert!(t.contains("ExecStart=\"/opt/My Apps/nocturne-gateway\"\n"));
        for must in [
            "Restart=on-failure",
            "NoNewPrivileges=yes",
            "ProtectSystem=strict",
            "ProtectHome=read-only",
            "PrivateTmp=yes",
            "RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX",
            "WantedBy=default.target",
            "ReadWritePaths=-%t/nocturne -/home/u/.local/share/nocturne/gateway\n",
        ] {
            assert!(t.contains(must), "{must}\n{t}");
        }
        // One section each, in order, and nothing that widens access.
        let at = |s: &str| t.find(s).unwrap();
        assert!(at("[Unit]") < at("[Service]") && at("[Service]") < at("[Install]"));
        for never in [
            "ProtectSystem=no",
            "NoNewPrivileges=no",
            "PrivateNetwork",
            "User=root",
        ] {
            assert!(!t.contains(never));
        }
    }

    /// `systemd-analyze verify` is offline (it reads the file, starts nothing, never talks to the user's manager). Skipped where the tool is missing.
    #[test]
    fn systemd_accepts_the_unit() {
        let Ok(exe) = std::fs::canonicalize("/usr/bin/true") else {
            return;
        };
        let d = tempfile::tempdir().unwrap();
        let data = d.path().join("data dir");
        let p = write_unit(d.path(), &exe, &[&data]).unwrap();
        let out = match std::process::Command::new("systemd-analyze")
            .args(["verify", "--user"])
            .arg(&p)
            .output()
        {
            Ok(o) => o,
            Err(_) => return,
        };
        let text = String::from_utf8_lossy(&out.stderr).into_owned()
            + &String::from_utf8_lossy(&out.stdout);
        // Anything it prints about this unit is a finding (a missing Documentation URL scheme or ExecStart would show here).
        assert!(out.status.success() && !text.contains(UNIT_NAME), "{text}");
    }

    #[test]
    fn awkward_paths_cannot_inject_directives() {
        let t = unit_text(Path::new("/x/a\"b%c$d\\e"), &[]).unwrap();
        assert!(t.contains(r#"ExecStart="/x/a\"b%%c$$d\\e""#), "{t}");
        assert!(unit_text(Path::new("/x\nExecStart=/bin/sh"), &[]).is_err());
        assert!(unit_text(Path::new("/ok"), &[Path::new("/y\nZ=1")]).is_err());
    }

    #[test]
    fn write_replaces_and_remove_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("systemd/user");
        let p = write_unit(&dir, Path::new("/a/nocturne-gateway"), &[]).unwrap();
        assert_eq!(p, dir.join(UNIT_NAME));
        write_unit(&dir, Path::new("/b/nocturne-gateway"), &[]).unwrap();
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("/b/nocturne-gateway")
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left"
        );
        assert!(remove_unit(&dir).unwrap());
        assert!(!remove_unit(&dir).unwrap());
    }
}
