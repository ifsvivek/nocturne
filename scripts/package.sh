#!/usr/bin/env bash
# Build release artifacts into dist/.   usage: scripts/package.sh tarball
set -euo pipefail
cd "$(dirname "$0")/.."

[ "${1:-}" = tarball ] || { echo "usage: $0 tarball" >&2; exit 2; }
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="nocturne-$ver-$(uname -m)-linux"
bin="${CARGO_TARGET_DIR:-target}/release/nocturne"

cargo build --release --locked -p nocturne-app
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
