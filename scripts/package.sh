#!/usr/bin/env bash
# Build release artifacts into dist/.   usage: scripts/package.sh tarball|deb|rpm|appimage
# deb, rpm and appimage build inside a distro container (docker, no sudo) via scripts/package-<format>.sh.
# tarball, deb and rpm share ONE binary built in a debian:stable container (scripts/build-linux-bin.sh); LOCAL=1 builds the tarball on the host instead.
# PROFILE=fast-release (thin LTO, quick) for QA and package tests; the default `release` is for published artifacts.
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in
  tarball) ;;
  deb|rpm|appimage) exec "scripts/package-$1.sh" ;;
  *) echo "usage: $0 tarball|deb|rpm|appimage" >&2; exit 2 ;;
esac
# The one place that names the app package (crates/app, binary `nocturne`; installed name is always `nocturne`).
PKG="${PKG:-nocturne-app}"; BIN="${BIN:-nocturne}"; PROFILE="${PROFILE:-release}"; export PROFILE
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="nocturne-$ver-$(uname -m)-linux"
if [ -n "${LOCAL:-}" ]; then
  source scripts/build-env.sh
  bin="${CARGO_TARGET_DIR:-target}/$PROFILE/$BIN"
  # Keep build-machine paths (home directory, checkout) out of the shipped binary.
  export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/home/build --remap-path-prefix=$PWD=/src"
  scripts/cache-budget.sh --quiet || true
  cargo build --profile "$PROFILE" --locked -p "$PKG"
else
  PKG="$PKG" scripts/build-linux-bin.sh
  bin="dist/bin/nocturne-$PROFILE"
fi
rm -rf "dist/$name" && mkdir -p "dist/$name/icons"
install -m755 "$bin" "dist/$name/nocturne"
install -m644 LICENSE THIRD_PARTY_NOTICES.md "dist/$name/"
install -m644 packaging/io.github.ifsvivek.nocturne.desktop packaging/nocturne.svg packaging/*.metainfo.xml "dist/$name/"
install -m644 packaging/icons/*.png "dist/$name/icons/"
install -m755 packaging/install.sh "dist/$name/"
# Fixed owner/mtime-independent-ish ordering so rebuilds of the same binary hash the same.
tar -C dist --sort=name --owner=0 --group=0 --numeric-owner -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "dist/$name.tar.gz"; cat "dist/$name.tar.gz.sha256"
