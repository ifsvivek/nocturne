#!/bin/sh
# Install or uninstall NOCTURNE from an unpacked release tarball.
#   ./install.sh [--no-deps] [install|uninstall]
#   env: PREFIX (default /usr/local), DESTDIR (default empty; staging, skips dependency handling),
#        NOCTURNE_YES=1 (pass the package manager's non-interactive flag)
# `install` first makes sure the runtime libraries are present (see `deps` below), installing missing ones
# with the distro package manager via sudo. --no-deps skips that.
set -eu
cd "$(dirname "$0")"
P="${DESTDIR:-}${PREFIX:-/usr/local}"
META=io.github.ifsvivek.nocturne.metainfo.xml

# ---- runtime dependencies: the single list. Keep in sync with docs/PACKAGING.md and packaging/PKGBUILD. ----
# Directly linked: libEGL libdbus-1 libxcb libxkbcommon(-x11) glibc libgcc_s.
# dlopen()ed at run time (invisible to readelf/namcap): libmpv.so.2 (in-window video; the app falls back to the
# `mpv` binary without it), libvulkan.so.1, libwayland-client.so.0, libwayland-egl.so.1.
# A Vulkan driver (mesa vulkan / NVIDIA) is also needed but depends on the GPU, so it is not installed here.
SONAMES="libmpv.so.2 libEGL.so.1 libvulkan.so.1 libwayland-client.so.0 libwayland-egl.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libxcb.so.1 libdbus-1.so.3"

# pkg_for DISTRO_FAMILY WHAT -> package name. WHAT is a soname or the word `mpv` (the binary).
# Families: arch debian fedora suse.
pkg_for() {
  case "$2" in
  libmpv.so.2) case $1 in arch) echo mpv ;; debian) echo libmpv2 ;; fedora) echo mpv-libs ;; suse) echo libmpv2 ;; esac ;;
  mpv) echo mpv ;;
  libEGL.so.1) case $1 in arch) echo libglvnd ;; debian) echo libegl1 ;; fedora) echo libglvnd-egl ;; suse) echo libEGL1 ;; esac ;;
  libvulkan.so.1) case $1 in arch) echo vulkan-icd-loader ;; debian) echo libvulkan1 ;; fedora) echo vulkan-loader ;; suse) echo libvulkan1 ;; esac ;;
  libwayland-client.so.0) case $1 in arch) echo wayland ;; debian) echo libwayland-client0 ;; fedora) echo libwayland-client ;; suse) echo libwayland-client0 ;; esac ;;
  libwayland-egl.so.1) case $1 in arch) echo wayland ;; debian) echo libwayland-egl1 ;; fedora) echo libwayland-egl ;; suse) echo libwayland-egl1 ;; esac ;;
  libxkbcommon.so.0) case $1 in arch | fedora) echo libxkbcommon ;; debian | suse) echo libxkbcommon0 ;; esac ;;
  libxkbcommon-x11.so.0) case $1 in arch | fedora) echo libxkbcommon-x11 ;; debian | suse) echo libxkbcommon-x11-0 ;; esac ;;
  libxcb.so.1) case $1 in arch | fedora) echo libxcb ;; debian | suse) echo libxcb1 ;; esac ;;
  libdbus-1.so.3) case $1 in arch) echo dbus ;; debian) echo libdbus-1-3 ;; fedora) echo dbus-libs ;; suse) echo libdbus-1-3 ;; esac ;;
  esac
}

distro_family() {
  # ID first, then ID_LIKE words, from os-release (OS_RELEASE overrides the path, for tests).
  f="${OS_RELEASE:-/etc/os-release}"
  [ -r "$f" ] || return 0
  ids=$(sed -n 's/^ID=//p;s/^ID_LIKE=//p' "$f" | tr -d "\"'" | tr '\n' ' ')
  for w in $ids; do
    case $w in
    arch | manjaro | cachyos | endeavouros) echo arch; return ;;
    debian | ubuntu) echo debian; return ;;
    fedora | rhel | centos) echo fedora; return ;;
    opensuse* | suse | sles) echo suse; return ;;
    esac
  done
}

have_lib() { ldconfig -p 2>/dev/null | grep -qF "$1"; }

ensure_deps() {
  missing=""
  for so in $SONAMES; do have_lib "$so" || missing="$missing $so"; done
  command -v mpv >/dev/null 2>&1 || missing="$missing mpv"
  [ -n "$missing" ] || { echo "runtime dependencies: all present"; return 0; }

  fam=$(distro_family)
  if [ -z "$fam" ]; then
    echo "error: missing runtime libraries:$missing" >&2
    echo "This distribution is not recognised (supported: Arch, Debian/Ubuntu, Fedora, openSUSE)." >&2
    echo "Install the packages that provide them with your package manager, then run: $0 --no-deps" >&2
    exit 1
  fi
  pkgs=""
  sudo=""
  for w in $missing; do
    n=$(pkg_for "$fam" "$w")
    case " $pkgs " in *" $n "*) ;; *) pkgs="$pkgs $n" ;; esac
  done
  y=""
  [ -z "${NOCTURNE_YES:-}" ] || y=1
  case $fam in
  arch) cmd="pacman -S --needed${y:+ --noconfirm}$pkgs" ;;
  debian) cmd="apt-get install${y:+ -y}$pkgs" ;;
  fedora) cmd="dnf install${y:+ -y}$pkgs" ;;
  suse) cmd="zypper${y:+ --non-interactive} install$pkgs" ;;
  esac
  if [ "$(id -u)" -ne 0 ]; then
    command -v sudo >/dev/null 2>&1 || { echo "error: need root to run: $cmd (sudo not found; use --no-deps to skip)" >&2; exit 1; }
    sudo=sudo
    cmd="sudo $cmd"
  fi
  echo "missing:$missing"
  if [ "$fam" = debian ]; then # a fresh system may have no package lists yet
    echo "running: ${sudo:+sudo }apt-get update"
    ${sudo:+sudo} apt-get update || echo "warning: apt-get update failed, trying the install anyway" >&2
  fi
  echo "running: $cmd"
  # shellcheck disable=SC2086
  $cmd || { echo "error: dependency install failed; fix it or rerun with --no-deps" >&2; exit 1; }
}

report_ldd() {
  nf=$(ldd "$P/bin/nocturne" 2>&1 | grep "not found" || true)
  if [ -n "$nf" ]; then
    echo "warning: libraries still missing for $P/bin/nocturne:" >&2
    echo "$nf" >&2
  fi
}

DEPS=1
if [ "${1:-}" = "--no-deps" ]; then DEPS=; shift; fi
case "${1:-install}" in
install)
  [ -z "$DEPS" ] || [ -n "${DESTDIR:-}" ] || ensure_deps
  install -Dm755 nocturne "$P/bin/nocturne"
  # Absolute Exec so launchers find the binary even when they do not have PREFIX/bin on their PATH.
  mkdir -p "$P/share/applications"
  sed "s|^Exec=nocturne|Exec=${PREFIX:-/usr/local}/bin/nocturne|" io.github.ifsvivek.nocturne.desktop \
    > "$P/share/applications/io.github.ifsvivek.nocturne.desktop"
  chmod 644 "$P/share/applications/io.github.ifsvivek.nocturne.desktop"
  for s in 48 128 256 512; do
    install -Dm644 "icons/nocturne-$s.png" "$P/share/icons/hicolor/${s}x${s}/apps/nocturne.png"
  done
  install -Dm644 nocturne.svg "$P/share/icons/hicolor/scalable/apps/nocturne.svg"
  install -Dm644 "$META" "$P/share/metainfo/$META"
  install -Dm644 LICENSE "$P/share/licenses/nocturne/LICENSE"
  install -Dm644 THIRD_PARTY_NOTICES.md "$P/share/licenses/nocturne/THIRD_PARTY_NOTICES.md"
  # Refresh caches so launchers pick the entry up now (best effort; skipped when staging into DESTDIR).
  if [ -z "${DESTDIR:-}" ]; then
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$P/share/applications" || true
    command -v gtk-update-icon-cache >/dev/null && [ -f "$P/share/icons/hicolor/index.theme" ] && gtk-update-icon-cache -qtf "$P/share/icons/hicolor" || true
  fi
  [ -n "${DESTDIR:-}" ] || report_ldd
  echo "installed to $P"
  echo "If NOCTURNE does not show in your app launcher, restart the launcher or log out and back in, or run: nocturne"
  ;;
uninstall)
  rm -f "$P/bin/nocturne" "$P/share/applications/io.github.ifsvivek.nocturne.desktop" \
    "$P/share/icons/hicolor/scalable/apps/nocturne.svg" "$P/share/metainfo/$META"
  for s in 48 128 256 512; do rm -f "$P/share/icons/hicolor/${s}x${s}/apps/nocturne.png"; done
  rm -rf "$P/share/licenses/nocturne"
  echo "removed from $P"
  ;;
*) echo "usage: $0 [--no-deps] [install|uninstall]" >&2; exit 2 ;;
esac
