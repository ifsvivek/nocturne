#!/usr/bin/env bash
# Build the Windows installer and a portable zip into dist/, entirely in Docker containers (no sudo, nothing installed
# on the host, no hosted CI). Cross-compiles x86_64-pc-windows-gnu, downloads the pinned mpv (packaging/windows/mpv.pin,
# SHA256 verified), makes the .ico, runs makensis.
#   usage: scripts/package-windows.sh      Output: dist/nocturne-<ver>-x86_64-setup.exe, dist/nocturne-<ver>-x86_64-windows.zip
# Unsigned: SmartScreen will warn until a code-signing certificate exists (docs/PACKAGING.md).
set -euo pipefail
cd "$(dirname "$0")/.."
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
. packaging/windows/mpv.pin
work=dist/win; mkdir -p "$work/stage" dist

# 1. nocturne.exe. Same cache volumes as scripts/check-cross.sh. Paths in the binary are the container's (/src, /usr/local/cargo), nothing personal.
docker run --rm -v "$PWD":/src:ro -v nocturne-cross-cargo:/usr/local/cargo/registry -v nocturne-cross-target:/target \
  -v "$PWD/$work":/out -e CARGO_TARGET_DIR=/target \
  -w /src rust:1-bookworm bash -euc '
    apt-get update -qq >/dev/null && apt-get install -y -qq gcc-mingw-w64-x86-64 >/dev/null
    rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1
    cargo build --release --locked --target x86_64-pc-windows-gnu -p nocturne-app 2>&1 | tail -n 5
    cp /target/x86_64-pc-windows-gnu/release/nocturne.exe /out/stage/'

# 2. pinned mpv, hash checked before anything is unpacked
[ -f "$work/$MPV_ASSET" ] || curl -fsSL -o "$work/$MPV_ASSET" "$MPV_URL"
echo "$MPV_SHA256  $work/$MPV_ASSET" | sha256sum -c - || { rm -f "$work/$MPV_ASSET"; echo "mpv archive hash mismatch" >&2; exit 1; }

# 3. licence page (GPL text + the bundled-library notice) and notices
cat LICENSE > "$work/stage/LICENSE.txt"
printf '\n\n----------------------------------------------------------------------\n\n' >> "$work/stage/LICENSE.txt"
python3 scripts/gen-notices.py --bundled >> "$work/stage/LICENSE.txt"
cp THIRD_PARTY_NOTICES.md "$work/stage/"

# 4. unpack mpv.exe, make the .ico, build the installer and the zip (debian container)
docker run --rm -v "$PWD":/src:ro -v "$PWD/$work":/w -e VER="$ver" -e ASSET="$MPV_ASSET" -e HOST_UID="$(id -u)" debian:stable bash -euc '
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq >/dev/null
  apt-get install -y -qq --no-install-recommends nsis 7zip imagemagick zip >/dev/null
  cd /w
  7z e -y -ostage "$ASSET" mpv.exe >/dev/null
  convert /src/packaging/icons/nocturne-256.png -define icon:auto-resize=256,128,48,32,16 stage/nocturne.ico
  makensis -V2 -DVERSION="$VER" -DSTAGE=/w/stage -DOUTFILE=/w/nocturne-$VER-x86_64-setup.exe /src/packaging/windows/nocturne.nsi
  (cd stage && zip -q -9 ../nocturne-$VER-x86_64-windows.zip nocturne.exe mpv.exe nocturne.ico LICENSE.txt THIRD_PARTY_NOTICES.md)
  chown -R $HOST_UID /w'
mv "$work/nocturne-$ver-x86_64-setup.exe" "$work/nocturne-$ver-x86_64-windows.zip" dist/
(cd dist && sha256sum "nocturne-$ver-x86_64-setup.exe" "nocturne-$ver-x86_64-windows.zip")
