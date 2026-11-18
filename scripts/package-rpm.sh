#!/usr/bin/env bash
# Build dist/nocturne-<ver>-1.<dist>.x86_64.rpm INSIDE a fedora container (packaging/nocturne.spec), packaging the binary
# scripts/build-linux-bin.sh built on debian:stable (older glibc, runs on Fedora). Nothing is compiled in the fedora container.
#   usage: [PROFILE=release|fast-release] [IMG=fedora:latest] scripts/package-rpm.sh
# Sources are the working tree (mounted read-only, tarred without target/dist/website).
set -euo pipefail
cd "$(dirname "$0")/.."
IMG="${IMG:-fedora:latest}"; PROFILE="${PROFILE:-release}"; export PROFILE
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
scripts/build-linux-bin.sh
mkdir -p dist

docker run --rm -v "$PWD":/src:ro -v "$PWD/dist":/out -e PROFILE="$PROFILE" -e VER="$ver" -e HOST_UID="$(id -u)" "$IMG" bash -euc '
dnf install -y -q rpm-build tar gzip >/dev/null
mkdir -p ~/rpmbuild/SOURCES
tar -C /src --exclude=./target --exclude=./dist --exclude=./website --exclude=./.git --exclude=./.claude \
  --transform "s,^\.,nocturne-$VER," -czf ~/rpmbuild/SOURCES/nocturne-$VER.tar.gz .
rpmbuild -bb --define "pkgver $VER" --define "binpath /src/dist/bin/nocturne-$PROFILE" /src/packaging/nocturne.spec
cp ~/rpmbuild/RPMS/*/nocturne-*.rpm /out/
chown $HOST_UID /out/nocturne-*.rpm
'
ls -l dist/nocturne-"$ver"-*.rpm
