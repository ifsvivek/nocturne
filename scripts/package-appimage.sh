#!/usr/bin/env bash
# EVALUATION build of dist/NOCTURNE-<ver>-x86_64.AppImage inside a debian:bookworm container (glibc 2.36; bookworm is the
# oldest common base that still ships libmpv.so.2, which the app dlopen()s; Ubuntu 22.04 only has libmpv.so.1).
# Bundles the app plus libmpv.so.2 and its library closure (ffmpeg, libass, ...). Deliberately NOT bundled, they must
# come from the host: glibc, libstdc++/libgcc_s, libGL/libEGL/GLX/libvulkan loaders and every GPU driver, libdrm, libgbm,
# all libwayland-*, libX11 and libxcb (the other X11 extension libs ARE bundled), libva/libvdpau (the host's drivers need the host's loader ABI), libxkbcommon.
#   usage: [PKG=nocturne-app] [IMG=debian:bookworm] [PROFILE=release|debug] scripts/package-appimage.sh
#   PROFILE=debug makes dist/NOCTURNE-<ver>-debug-x86_64.AppImage, whose NOCTURNE_PREVIEW=player can play a file (testing only).
# Needs network (rustup, apt, and the appimagetool release from GitHub, downloaded unverified: no pinned checksum).
# No sudo (docker), no hosted CI. Cargo cache is a throwaway volume (KEEP_CACHE=1 keeps it).
set -euo pipefail
cd "$(dirname "$0")/.."
PKG="${PKG:-nocturne-app}"; PROFILE="${PROFILE:-release}"; IMG="${IMG:-debian:bookworm}"
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
VOL=nocturne-appimage-cache
mkdir -p dist
trap '[ -n "${KEEP_CACHE:-}" ] || docker volume rm -f "$VOL" >/dev/null 2>&1' EXIT

docker run --rm -v "$PWD":/src:ro -v "$PWD/dist":/out -v "$VOL":/cache -e PKG="$PKG" -e VER="$ver" -e PROFILE="$PROFILE" -e HOST_UID="$(id -u)" "$IMG" bash -euc '
export DEBIAN_FRONTEND=noninteractive CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target RUSTUP_HOME=/cache/rustup
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential pkg-config curl ca-certificates libdbus-1-dev libudev-dev libegl-dev libmpv2 file binutils >/dev/null
[ -x /cache/cargo/bin/cargo ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
export PATH=/cache/cargo/bin:$PATH
cd /src
RUSTFLAGS="--remap-path-prefix=/cache/cargo=/cargo --remap-path-prefix=/src=/src" cargo build $([ "$PROFILE" = release ] && echo --release) --locked -p "$PKG"

A=/tmp/AppDir; rm -rf $A; mkdir -p $A/usr/bin $A/usr/lib
install -m755 /cache/target/$([ "$PROFILE" = release ] && echo release || echo debug)/nocturne $A/usr/bin/nocturne
# libmpv closure minus host-provided libraries (see header).
skip="^(ld-linux.*|libc|libm|libdl|libpthread|librt|libutil|libresolv|libnsl|libanl|libstdc\+\+|libgcc_s|libGL|libGLX|libGLdispatch|libEGL|libOpenGL|libGLESv[12]|libvulkan|libgbm|libdrm.*|libwayland-.*|libX11|libxcb|libxkbcommon.*|libva.*|libvdpau|libnvidia.*)\.so"
mpv=$(ldconfig -p | awk "/libmpv.so.2 /{print \$NF; exit}")
for l in $mpv $(ldd $mpv | awk "/=> \//{print \$3}"); do
  b=$(basename $l); echo "$b" | grep -Eq "$skip" || cp -L $l $A/usr/lib/$b
done
cp -L $mpv $A/usr/lib/libmpv.so.2
printf "%s\n" "#!/bin/sh" "HERE=\$(dirname \"\$(readlink -f \"\$0\")\")" \
  "export LD_LIBRARY_PATH=\"\$HERE/usr/lib\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}\"" \
  "exec \"\$HERE/usr/bin/nocturne\" \"\$@\"" > $A/AppRun; chmod +x $A/AppRun
install -Dm644 packaging/io.github.ifsvivek.nocturne.desktop $A/io.github.ifsvivek.nocturne.desktop
install -Dm644 packaging/icons/nocturne-256.png $A/nocturne.png
install -Dm644 packaging/icons/nocturne-256.png $A/usr/share/icons/hicolor/256x256/apps/nocturne.png
install -Dm644 packaging/*.metainfo.xml -t $A/usr/share/metainfo
install -Dm644 LICENSE THIRD_PARTY_NOTICES.md -t $A/usr/share/licenses/nocturne

curl -fsSL -o /tmp/appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x /tmp/appimagetool
[ "$PROFILE" = release ] && sfx= || sfx=-$PROFILE
out=/out/NOCTURNE-$VER$sfx-x86_64.AppImage
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 /tmp/appimagetool --no-appstream -n $A $out
chown $HOST_UID $out
echo "bundled libraries: $(ls $A/usr/lib | wc -l), AppDir $(du -sh $A | cut -f1)"
'
ls -l dist/NOCTURNE-"${ver}"*-x86_64.AppImage
