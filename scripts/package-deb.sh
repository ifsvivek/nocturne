#!/usr/bin/env bash
# Build dist/nocturne_<ver>_amd64.deb (the desktop app) and dist/nocturne-server_<ver>_amd64.deb (the headless server, which the app depends on) from the binary scripts/build-linux-bin.sh makes in a debian:stable container (that glibc,
# so it also runs on Ubuntu LTS releases that are not older than it). No sudo (docker), no hosted CI.
#   usage: [PROFILE=release|fast-release] [IMG=debian:stable] scripts/package-deb.sh
# Packaging runs in the local nocturne-build image (build-essential brings dpkg-dev; no apt at run time), so it takes seconds once the binary exists. Output: dist/nocturne_<ver>_amd64.deb
set -euo pipefail
cd "$(dirname "$0")/.."
IMG="${IMG:-debian:stable}"; PROFILE="${PROFILE:-release}"; export IMG PROFILE
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
scripts/build-linux-bin.sh
mkdir -p dist

docker run --rm -v "$PWD":/src:ro -v "$PWD/dist":/out -e BIN=/src/dist/bin/nocturne-$PROFILE -e GWBIN=/src/dist/bin/nocturne-gateway-$PROFILE -e SRVBIN=/src/dist/bin/nocturne-server-$PROFILE -e VER="$ver" -e HOST_UID="$(id -u)" "nocturne-build:$(echo "$IMG" | tr ':/' '--')" bash -euc '
cd /src
bin=$BIN

# Linked libraries (libc6, libdbus-1-3, libgcc-s1) get versioned deps from dpkg-shlibdeps; everything dlopen()ed at run
# time is listed by hand (see docs/PACKAGING.md).
w=$(mktemp -d); mkdir -p $w/debian; touch $w/debian/control; cp $bin $w/nocturne
shl=$(cd $w && dpkg-shlibdeps -O -enocturne 2>/dev/null | sed "s/^shlibs:Depends=//")
[ -n "$shl" ] || { echo "dpkg-shlibdeps found no dependencies (a linked library is not installed in this container?)" >&2; exit 1; }

r=$w/root
install -Dm755 $bin $r/usr/bin/nocturne
install -Dm755 $GWBIN $r/usr/bin/nocturne-gateway
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
Depends: $shl, nocturne-server (= $VER), libmpv2, libvulkan1, mesa-vulkan-drivers | vulkan-icd, libwayland-client0, libwayland-egl1, libxkbcommon0
Recommends: mpv, gnome-keyring | kwalletmanager | keepassxc, xdg-desktop-portal, fonts-noto-core
Description: Native media center for local folders and Jellyfin servers
 One cinematic, searchable library over local media and any number of
 Jellyfin servers, playing video inside the app window through libmpv.
 Not affiliated with the Jellyfin project.
CTL
dpkg-deb --root-owner-group -Zxz -b $r /out/nocturne_${VER}_amd64.deb

# The server: only its linked libraries (nothing is dlopen()ed), no GUI stack.
s=$(mktemp -d); mkdir -p $s/debian; touch $s/debian/control; cp $SRVBIN $s/nocturne-server
sshl=$(cd $s && dpkg-shlibdeps -O -enocturne-server 2>/dev/null | sed "s/^shlibs:Depends=//")
[ -n "$sshl" ] || { echo "dpkg-shlibdeps found no dependencies for nocturne-server" >&2; exit 1; }
r=$s/root
install -Dm755 $SRVBIN $r/usr/bin/nocturne-server
install -Dm644 THIRD_PARTY_NOTICES.md -t $r/usr/share/doc/nocturne-server
install -Dm644 LICENSE $r/usr/share/doc/nocturne-server/copyright
mkdir -p $r/DEBIAN
size=$(du -ks $r/usr | cut -f1)
cat > $r/DEBIAN/control <<CTL
Package: nocturne-server
Version: $VER
Architecture: amd64
Maintainer: ifsvivek <noreply@users.noreply.github.com>
Installed-Size: $size
Section: video
Priority: optional
Homepage: https://nocturne.ifsvivek.in
Depends: $sshl
Recommends: gnome-keyring | kwalletmanager | keepassxc
Suggests: cloudflared, gvfs-backends, nfs-common
Description: NOCTURNE headless media server
 Library manager, downloads, requests, gateway and storage modules behind
 one REST and WebSocket API, with a web dashboard. Used by the NOCTURNE
 desktop app; runs alone on a headless machine.
CTL
dpkg-deb --root-owner-group -Zxz -b $r /out/nocturne-server_${VER}_amd64.deb
chown $HOST_UID /out/nocturne_${VER}_amd64.deb /out/nocturne-server_${VER}_amd64.deb
'
ls -l "dist/nocturne_${ver}_amd64.deb" "dist/nocturne-server_${ver}_amd64.deb"
