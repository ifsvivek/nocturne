#!/usr/bin/env bash
# Clean-container release gate: build the Arch package from `git archive HEAD` in a throwaway archlinux:base-devel
# container, install it, then RUN the installed `nocturne` inside the container on the host's Wayland session
# (socket bind-mounted, software rendering only: mesa lavapipe, no /dev/dri). No GitHub Actions, no sudo (docker group).
#   usage: [PKG=nocturne-app BIN=nocturne] scripts/test-playback.sh
#   (defaults nocturne-app / nocturne; the PKGBUILD's _pkg/_bin; the installed name is always `nocturne`)
#   env: HOLD (10)  seconds the window must stay up;  KEEP_IMAGES=1 keeps an image this script pulled.
# Commit first: the package is built from HEAD, not from the working tree.
#
# What this proves: the package builds and installs from a clean image with the declared depends only; every linked
# library resolves; the binary reaches the windowing stage headless without a loader error; the installed binary
# connects to a Wayland compositor, creates a window (seen by hyprctl, matched by pid through `docker top`), paints
# with the Vulkan software rasteriser and survives HOLD seconds without a crash; the installed libmpv.so.2 decodes
# a generated clip (mpv --vo=null).
# What it does NOT prove: in-window video (libmpv render API into the Wayland subsurface, input, controls over video),
# hardware decoding/rendering, the UI contents (no screenshot path exists in a release build), a real library. Those
# are verified on the host (docs/QA_REPORT.md). It never contacts a Jellyfin server: no URL/token is passed in.
set -uo pipefail
cd "$(dirname "$0")/.."
PKG="${PKG:-nocturne-app}"; BIN="${BIN:-nocturne}"; HOLD="${HOLD:-10}"
for c in docker jq hyprctl; do command -v "$c" >/dev/null || { echo "need $c" >&2; exit 2; }; done
[ -n "${WAYLAND_DISPLAY:-}" ] && [ -S "${XDG_RUNTIME_DIR:-/nonexistent}/$WAYLAND_DISPLAY" ] || { echo "no Wayland session on the host" >&2; exit 2; }
git diff --quiet HEAD -- || echo "warning: uncommitted changes are NOT in the package (built from HEAD)" >&2

IMG=archlinux:base-devel; CT=nocturne-playback-$$; UID_H=$(id -u)
WORK=$(mktemp -d /tmp/nocturne-playback.XXXXXX); PULLED=""; FAILS=0
cleanup() {
  docker rm -f "$CT" >/dev/null 2>&1
  [ -z "$PULLED" ] || [ -n "${KEEP_IMAGES:-}" ] || docker rmi "$IMG" >/dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT
pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*"; FAILS=$((FAILS + 1)); }
info() { echo "INFO  $*"; }
dx() { docker exec "$CT" "$@"; }                      # root in the container
ux() { docker exec -u builder "$CT" "$@"; }           # unprivileged user with the host uid

docker image inspect "$IMG" >/dev/null 2>&1 || { docker pull -q "$IMG" >/dev/null && PULLED=1; } || { echo "cannot pull $IMG"; exit 1; }

pv=$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)
mkdir -p "$WORK/pkg"
git archive --format=tar.gz --prefix="nocturne-$pv/" -o "$WORK/pkg/nocturne-$pv.tar.gz" HEAD
sed -e "s|^source=.*|source=(\"nocturne-$pv.tar.gz\")|" -e "s|^sha256sums=.*|sha256sums=('SKIP')|" packaging/PKGBUILD > "$WORK/pkg/PKGBUILD"

# The compositor socket lands at /tmp/wl/wayland-0; XDG_RUNTIME_DIR=/tmp/wl inside.
docker run -d --name "$CT" -v "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY:/tmp/wl/wayland-0" -v "$WORK:/w" "$IMG" sleep 3600 >/dev/null || { echo "cannot start container"; exit 1; }

echo "== build and install the package in the container (slow: full release build)"
dx sh -c "pacman -Syu --noconfirm sudo >/dev/null 2>&1
  useradd -m -u $UID_H builder && echo 'builder ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/b
  cp -r /w/pkg /home/builder/pkg && chown -R builder /home/builder/pkg
  chown builder /tmp/wl && chmod 700 /tmp/wl" || { fail "container setup"; exit 1; }
ux sh -c "cd ~/pkg && _pkg='$PKG' _bin='$BIN' makepkg -si --noconfirm --nocheck > /tmp/mk.log 2>&1"; rc=$?
[ $rc -eq 0 ] && pass "makepkg -si ($PKG -> /usr/bin/nocturne)" || { fail "makepkg failed"; ux tail -30 /tmp/mk.log; exit 1; }
info "$(dx pacman -Qi nocturne | grep -E '^Depends On' | tr -s ' ')"
[ "$(dx sh -c 'ldd /usr/bin/nocturne | grep -c "not found"')" = 0 ] && pass "ldd: no missing library" || fail "ldd reports missing libraries"
dx sh -c 'ldconfig -p | grep -q "libmpv.so.2"' && pass "installed libmpv.so.2 (ldconfig)" || fail "libmpv.so.2 not found"

echo "== loader check without a display"
out=$(dx sh -c 'unset WAYLAND_DISPLAY DISPLAY; timeout 15 /usr/bin/nocturne 2>&1 | head -20')
if grep -qE 'neither DISPLAY nor WAYLAND_DISPLAY|neither WAYLAND_DISPLAY nor|open window' <<<"$out" && ! grep -qE 'error while loading shared libraries|cannot open shared object|GLIBC_[0-9.]+. not found' <<<"$out"; then
  pass "headless start reaches the windowing stage, no loader error"
else fail "headless start: $(head -3 <<<"$out" | tr '\n' '|')"; fi
info "headless output: $(head -3 <<<"$out" | tr '\n' '|')"

echo "== headless libmpv: generated clip through the installed mpv"
dx pacman -S --noconfirm --needed mesa vulkan-swrast vulkan-icd-loader >/dev/null 2>&1 || fail "cannot install software rendering stack"
mp=$(ux sh -c 'cd /tmp && ffmpeg -loglevel error -f lavfi -i testsrc2=size=640x360:rate=30 -f lavfi -i sine=frequency=440 -t 4 -c:v libx264 -c:a aac -shortest clip.mp4 \
  && mpv --no-config --vo=null --ao=null --hwdec=no --frames=60 clip.mp4 2>&1; echo mpv_rc=$?')
grep -q 'mpv_rc=0' <<<"$mp" && pass "mpv --vo=null --frames=60 clip.mp4 (installed libmpv decodes video)" || fail "mpv clip: $(tail -4 <<<"$mp" | tr '\n' '|')"
info "mpv: $(grep -E 'libmpv|Video|Audio|Exiting' <<<"$mp" | head -4 | tr '\n' '|')"

echo "== installed app on the host Wayland session (software rendering), ${HOLD}s hold"
ux sh -c 'mkdir -p /tmp/run/cfg /tmp/run/data /tmp/run/cache /tmp/run/cwd'
ux sh -c "cd /tmp/run/cwd && env -u NOCTURNE_JF_URL -u NOCTURNE_JF_TOKEN XDG_RUNTIME_DIR=/tmp/wl WAYLAND_DISPLAY=wayland-0 \
  XDG_CONFIG_HOME=/tmp/run/cfg XDG_DATA_HOME=/tmp/run/data XDG_CACHE_HOME=/tmp/run/cache NOCTURNE_KEYRING_SERVICE=nocturne-test \
  WGPU_BACKEND=vulkan RUST_BACKTRACE=1 nohup /usr/bin/nocturne > /tmp/app.log 2>&1 & echo \$! > /tmp/app.pid"
# Host pids of everything in the container (hyprctl reports the host-namespace pid of the Wayland client).
mapped() {
  local pids; pids=$(docker top "$CT" -eo pid 2>/dev/null | tail -n +2 | tr -d ' ' | jq -Rs '[split("\n")[]|select(length>0)|tonumber]')
  hyprctl clients -j 2>/dev/null | jq -e --argjson p "$pids" 'any(.[]; .class=="nocturne" and .mapped and (.pid as $x | $p | index($x) != null))' >/dev/null
}
up=""; for _ in $(seq 1 120); do mapped && { up=1; break; }; ux sh -c 'kill -0 $(cat /tmp/app.pid)' 2>/dev/null || break; sleep 0.5; done
if [ -n "$up" ]; then
  pass "window class=nocturne from the container appeared on the host compositor"
  sleep "$HOLD"
  if mapped && ux sh -c 'kill -0 $(cat /tmp/app.pid)'; then pass "still mapped and running after ${HOLD}s"; else fail "window or process gone within ${HOLD}s"; fi
else fail "no nocturne window from the container within 60 s"; fi
ux sh -c 'kill $(cat /tmp/app.pid) 2>/dev/null; sleep 1'
log=$(ux cat /tmp/app.log); grep -qiE 'panicked|segmentation|SIGSEGV|fatal' <<<"$log" && fail "panic/crash in app log" || pass "no panic in app log"
info "app log (first lines): $(head -6 <<<"$log" | cut -c1-160 | tr '\n' '|')"
echo "failures: $FAILS"
[ "$FAILS" -eq 0 ]
