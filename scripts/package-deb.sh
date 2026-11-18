#!/usr/bin/env bash
# Build dist/nocturne_<ver>_amd64.deb INSIDE a debian:stable container, so the binary links against that glibc (and
# therefore also runs on Ubuntu LTS releases that are not older than it). No sudo (docker), no hosted CI.
#   usage: [PKG=nocturne-app] [IMG=debian:stable] scripts/package-deb.sh
# Sources are the working tree (mounted read-only); the cargo cache lives in a throwaway volume, removed at the end
# unless KEEP_CACHE=1. Output: dist/nocturne_<ver>_amd64.deb
set -euo pipefail
cd "$(dirname "$0")/.."
PKG="${PKG:-nocturne-app}"; IMG="${IMG:-debian:stable}"
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
VOL=nocturne-deb-cache
mkdir -p dist
trap '[ -n "${KEEP_CACHE:-}" ] || docker volume rm -f "$VOL" >/dev/null 2>&1' EXIT

docker run --rm -v "$PWD":/src:ro -v "$PWD/dist":/out -v "$VOL":/cache -e PKG="$PKG" -e VER="$ver" -e CARGO_PROFILE_RELEASE_LTO -e CARGO_PROFILE_RELEASE_CODEGEN_UNITS -e HOST_UID="$(id -u)" "$IMG" bash -euc '
export DEBIAN_FRONTEND=noninteractive CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target RUSTUP_HOME=/cache/rustup
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential pkg-config curl ca-certificates libdbus-1-dev libudev-dev libegl-dev dpkg-dev binutils >/dev/null
[ -x /cache/cargo/bin/cargo ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
export PATH=/cache/cargo/bin:$PATH
cd /src
RUSTFLAGS="--remap-path-prefix=/cache/cargo=/cargo --remap-path-prefix=/src=/src" cargo build --release --locked -p "$PKG"
bin=/cache/target/release/nocturne

# Linked libraries (libc6, libdbus-1-3, libgcc-s1) get versioned deps from dpkg-shlibdeps; everything dlopen()ed at run
# time is listed by hand (see docs/PACKAGING.md).
w=$(mktemp -d); mkdir -p $w/debian; touch $w/debian/control; cp $bin $w/nocturne
shl=$(cd $w && dpkg-shlibdeps -O -enocturne 2>/dev/null | sed "s/^shlibs:Depends=//")

r=$w/root
install -Dm755 $bin $r/usr/bin/nocturne
install -Dm644 packaging/io.github.ifsvivek.nocturne.desktop -t $r/usr/share/applications
for s in 48 128 256 512; do install -Dm644 packaging/icons/nocturne-$s.png $r/usr/share/icons/hicolor/${s}x${s}/apps/nocturne.png; done
install -Dm644 packaging/nocturne.svg -t $r/usr/share/icons/hicolor/scalable/apps
install -Dm644 packaging/*.metainfo.xml -t $r/usr/share/metainfo
install -Dm644 THIRD_PARTY_NOTICES.md -t $r/usr/share/doc/nocturne
install -Dm644 LICENSE $r/usr/share/doc/nocturne/copyright
mkdir -p $r/DEBIAN
size=$(du -ks $r/usr | cut -f1)
cat > $r/DEBIAN/control <<CTL
Package: nocturne
Version: $VER
Architecture: amd64
Maintainer: ifsvivek <noreply@users.noreply.github.com>
Installed-Size: $size
Section: video
Priority: optional
Homepage: https://nocturne.ifsvivek.in
Depends: $shl, libmpv2, libvulkan1, mesa-vulkan-drivers | vulkan-icd, libwayland-client0, libwayland-egl1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxcb1
Recommends: mpv, gnome-keyring | kwalletmanager | keepassxc, xdg-desktop-portal, fonts-noto-core
Description: Native media center for local folders and Jellyfin servers
 One cinematic, searchable library over local media and any number of
 Jellyfin servers, playing video inside the app window through libmpv.
 Not affiliated with the Jellyfin project.
CTL
dpkg-deb --root-owner-group -Zxz -b $r /out/nocturne_${VER}_amd64.deb
chown $HOST_UID /out/nocturne_${VER}_amd64.deb
'
ls -l "dist/nocturne_${ver}_amd64.deb"
