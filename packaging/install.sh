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
  install -Dm644 io.github.ifsvivek.nocturne.desktop "$P/share/applications/io.github.ifsvivek.nocturne.desktop"
  for s in 48 128 256 512; do
    install -Dm644 "icons/nocturne-$s.png" "$P/share/icons/hicolor/${s}x${s}/apps/nocturne.png"
  done
  install -Dm644 nocturne.svg "$P/share/icons/hicolor/scalable/apps/nocturne.svg"
  install -Dm644 "$META" "$P/share/metainfo/$META"
  install -Dm644 LICENSE "$P/share/licenses/nocturne/LICENSE"
  install -Dm644 THIRD_PARTY_NOTICES.md "$P/share/licenses/nocturne/THIRD_PARTY_NOTICES.md"
  echo "installed to $P"
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
