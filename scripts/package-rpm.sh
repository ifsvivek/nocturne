#!/usr/bin/env bash
# Build dist/nocturne-<ver>-1.<dist>.x86_64.rpm INSIDE a fedora container (packaging/nocturne.spec).
#   usage: [PKG unsupported: spec builds nocturne-app] [IMG=fedora:latest] scripts/package-rpm.sh
# Sources are the working tree (mounted read-only, tarred without target/dist/website); the cargo cache lives in a
# throwaway volume removed at the end unless KEEP_CACHE=1.
set -euo pipefail
cd "$(dirname "$0")/.."
IMG="${IMG:-fedora:latest}"
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
VOL=nocturne-rpm-cache
mkdir -p dist
trap '[ -n "${KEEP_CACHE:-}" ] || docker volume rm -f "$VOL" >/dev/null 2>&1' EXIT

docker run --rm -v "$PWD":/src:ro -v "$PWD/dist":/out -v "$VOL":/cache -e VER="$ver" -e CARGO_PROFILE_RELEASE_LTO -e CARGO_PROFILE_RELEASE_CODEGEN_UNITS -e HOST_UID="$(id -u)" "$IMG" bash -euc '
export CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target RUSTUP_HOME=/cache/rustup
dnf install -y -q rpm-build gcc gcc-c++ make pkgconf-pkg-config dbus-devel systemd-devel libglvnd-devel curl tar gzip >/dev/null
[ -x /cache/cargo/bin/cargo ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
mkdir -p ~/rpmbuild/SOURCES
tar -C /src --exclude=./target --exclude=./dist --exclude=./website --exclude=./.git --exclude=./.claude \
  --transform "s,^\.,nocturne-$VER," -czf ~/rpmbuild/SOURCES/nocturne-$VER.tar.gz .
rpmbuild -bb --define "pkgver $VER" /src/packaging/nocturne.spec
cp ~/rpmbuild/RPMS/*/nocturne-*.rpm /out/
chown $HOST_UID /out/nocturne-*.rpm
'
ls -l dist/nocturne-"$ver"-*.rpm
