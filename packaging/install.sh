#!/bin/sh
# Install or uninstall NOCTURNE from an unpacked release tarball.
#   ./install.sh [install|uninstall]      env: PREFIX (default /usr/local), DESTDIR (default empty)
set -eu
cd "$(dirname "$0")"
P="${DESTDIR:-}${PREFIX:-/usr/local}"
META=io.github.ifsvivek.nocturne.metainfo.xml

case "${1:-install}" in
install)
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
*) echo "usage: $0 [install|uninstall]" >&2; exit 2 ;;
esac
