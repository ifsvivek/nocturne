//! Runtime-dependency probes and install hints (no UI deps, so the app decides how to show them).

use std::path::{Path, PathBuf};

/// `Ok` when libmpv (`libmpv.so.2` / `libmpv.2.dylib` / `libmpv-2.dll`) can be loaded, else a short reason. The in-window player needs it;
/// without it use the external-`mpv` [`crate::Player`] (see [`mpv_binary_available`]).
pub fn libmpv_available() -> Result<(), String> {
    crate::mpv_ffi::available()
}

pub(crate) fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    p.is_file()
}

/// The `mpv` the child-process [`crate::Player`] runs. Linux: whatever `PATH` resolves (unchanged).
/// macOS/Windows: a copy shipped next to the app first, then `PATH`, then (macOS GUI apps get a
/// minimal `PATH`) the Homebrew prefixes.
pub(crate) fn mpv_path() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "mpv.exe" } else { "mpv" };
    let mut dirs: Vec<PathBuf> = vec![];
    if !cfg!(target_os = "linux") {
        dirs.extend(crate::mpv_ffi::exe_dir());
    }
    dirs.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    dirs.into_iter()
        .map(|d| d.join(exe))
        .find(|p| is_executable(p))
}

/// Program name or path to spawn; a bare `mpv` lets the OS report "not found" itself.
pub(crate) fn mpv_program() -> PathBuf {
    mpv_path().unwrap_or_else(|| "mpv".into())
}

/// True when an executable `mpv` was found (see [`mpv_path`]).
pub fn mpv_binary_available() -> bool {
    mpv_path().is_some()
}

/// Install command for the mpv library/player, chosen from `/etc/os-release` contents (`ID`, `ID_LIKE`).
pub fn install_hint_for(os_release: &str) -> String {
    let field = |key: &str| {
        os_release
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| {
                v.trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_lowercase()
            })
            .unwrap_or_default()
    };
    let (id, like) = (field("ID"), field("ID_LIKE"));
    let is = |names: &[&str]| {
        id.split_whitespace()
            .chain(like.split_whitespace())
            .any(|w| names.contains(&w))
    };
    let cmd = if is(&["arch", "manjaro", "cachyos", "endeavouros"]) {
        "sudo pacman -S mpv"
    } else if is(&["debian", "ubuntu"]) {
        "sudo apt install libmpv2"
    } else if is(&["fedora", "rhel", "centos"]) {
        "sudo dnf install mpv-libs"
    } else if is(&[
        "opensuse",
        "opensuse-leap",
        "opensuse-tumbleweed",
        "suse",
        "sles",
    ]) {
        "sudo zypper install libmpv2"
    } else {
        return "Install mpv's shared library (libmpv.so.2) with your package manager.".into();
    };
    format!("Install it with: {cmd}")
}

/// macOS and Windows ship libmpv inside the app, so a missing one means a damaged install.
fn bundled_hint() -> String {
    "The mpv library is bundled with NOCTURNE and could not be loaded; reinstall NOCTURNE.".into()
}

/// [`install_hint_for`] applied to this machine's `/etc/os-release` (Linux); macOS/Windows: reinstall.
pub fn install_hint() -> String {
    if cfg!(target_os = "linux") {
        install_hint_for(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
    } else {
        bundled_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_by_distro() {
        let h = |s| install_hint_for(s);
        assert!(h("NAME=\"Arch Linux\"\nID=arch\n").ends_with("sudo pacman -S mpv"));
        assert!(h("ID=cachyos\nID_LIKE=\"arch\"\n").ends_with("sudo pacman -S mpv"));
        assert!(h("ID=ubuntu\nID_LIKE=debian\n").ends_with("sudo apt install libmpv2"));
        assert!(
            h("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n").ends_with("sudo apt install libmpv2")
        );
        assert!(h("ID=fedora\n").ends_with("sudo dnf install mpv-libs"));
        assert!(
            h("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n")
                .ends_with("sudo zypper install libmpv2")
        );
        assert!(h("ID=nixos\n").contains("package manager"));
        assert!(h("").contains("package manager"));
    }

    #[test]
    fn bundled_platforms_say_reinstall() {
        assert!(bundled_hint().contains("reinstall"));
    }

    #[test]
    fn id_prefix_not_confused_with_id_like() {
        // VERSION_ID / BUILD_ID must not be read as ID.
        assert!(install_hint_for("VERSION_ID=\"12\"\nID=debian\n").ends_with("libmpv2"));
    }
}
