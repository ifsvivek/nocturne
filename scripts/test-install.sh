#!/usr/bin/env bash
# Clean-environment install tests in throwaway Docker containers. No sudo needed (docker group), no GitHub Actions.
# usage: scripts/test-install.sh [all|arch|nompv|debian|ubuntu|fedora|pkgbuild ...]   (default: all except pkgbuild)
#   pkgbuild is slow (a full release build in the container), so it only runs when named.
# Builds the tarball with scripts/package.sh (PKG/BIN env choose the app package, see there) unless dist/ already has one for the current version
# (set REBUILD=1 to force). Prints PASS/FAIL lines; exit status 1 if any FAIL. "INFO" lines are recorded facts
# (expected failures such as a glibc mismatch on an older distro). Containers use --rm; images this script
# pulled are removed at the end (KEEP_IMAGES=1 keeps them). Nothing here touches the host or any server.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
WORK=$(mktemp -d /tmp/nocturne-install-test.XXXXXX)
FAILS=0
PULLED=()
trap 'rm -rf "$WORK"; [ -n "${KEEP_IMAGES:-}" ] || { [ ${#PULLED[@]} -eq 0 ] || docker rmi "${PULLED[@]}" >/dev/null 2>&1; }' EXIT

pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*"; FAILS=$((FAILS + 1)); }
info() { echo "INFO  $*"; }

ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TAR="dist/nocturne-$ver-$(uname -m)-linux.tar.gz"
if [ -n "${REBUILD:-}" ] || [ ! -f "$TAR" ]; then scripts/package.sh tarball >/dev/null || { echo "tarball build failed"; exit 1; }; fi
cp "$TAR" "$WORK/nocturne.tar.gz"

# image NAME: pull if missing (remember to remove it afterwards).
image() {
  docker image inspect "$1" >/dev/null 2>&1 || { docker pull -q "$1" >/dev/null && PULLED+=("$1"); }
}
# run IMAGE SCRIPT-FILE [extra docker args]: SCRIPT-FILE is under $WORK; /w is the work dir; output captured in $OUT.
run() {
  local img=$1 s=$2; shift 2
  OUT=$(docker run --rm -v "$WORK:/w" "$@" "$img" sh /w/"$s" 2>&1); RC=$?
}

# Common prelude: unpack the tarball into /t.
cat > "$WORK/pre.sh" <<'EOF'
mkdir -p /t && tar -xzf /w/nocturne.tar.gz -C /t && cd /t/nocturne-* || exit 90
EOF

# Headless start: no display, must fail on the windowing stage, never on the dynamic loader.
cat > "$WORK/headless.sh" <<'EOF'
unset WAYLAND_DISPLAY DISPLAY
echo "--- headless start"; timeout 15 "$1" 2>&1 | head -20; echo "exit=${PIPESTATUS[0]:-$?}"
EOF

loader_ok() { # $1 = text of a headless start
  grep -qE 'neither DISPLAY nor WAYLAND_DISPLAY|neither WAYLAND_DISPLAY nor|open window' <<<"$1" && # must actually reach the windowing stage
  ! grep -qE 'error while loading shared libraries|cannot open shared object|version .GLIBC_[0-9.]+. not found' <<<"$1"
}

t_arch() {
  image archlinux:latest || { fail "arch: cannot pull image"; return; }
  cat > "$WORK/arch.sh" <<'EOF'
. /w/pre.sh
pacman -Syu --noconfirm >/dev/null 2>&1   # a real system is synced and upgraded; the image is stale (-Sy alone = partial upgrade, file conflicts)
NOCTURNE_YES=1 ./install.sh; echo "install_rc=$?"
echo "--- pacman"; for p in mpv libglvnd vulkan-icd-loader wayland libxkbcommon libxkbcommon dbus; do pacman -Q $p >/dev/null 2>&1 && echo "have $p" || echo "MISSING $p"; done
echo "--- libmpv"; ldconfig -p | grep -c 'libmpv.so.2'
echo "--- ldd"; ldd /usr/local/bin/nocturne | grep -c 'not found'
bash /w/headless.sh /usr/local/bin/nocturne
./install.sh uninstall >/dev/null; echo "--- after uninstall"; ls /usr/local/bin/nocturne /usr/local/share/applications/io.github.ifsvivek.nocturne.desktop /usr/local/share/licenses/nocturne 2>&1 | grep -c 'No such file'
EOF
  run archlinux:latest arch.sh
  grep -q 'install_rc=0' <<<"$OUT" && pass "arch: install.sh exit 0" || fail "arch: install.sh failed"
  grep -q 'MISSING' <<<"$OUT" && fail "arch: runtime package missing ($(grep MISSING <<<"$OUT" | tr '\n' ' '))" || pass "arch: all runtime packages installed by pacman"
  [ "$(sed -n '/^--- libmpv/{n;p}' <<<"$OUT")" -ge 1 ] 2>/dev/null && pass "arch: libmpv.so.2 present" || fail "arch: libmpv.so.2 missing"
  [ "$(sed -n '/^--- ldd/{n;p}' <<<"$OUT")" = 0 ] && pass "arch: ldd has no 'not found'" || fail "arch: ldd reports missing libraries"
  loader_ok "$(sed -n '/^--- headless/,/^exit=/p' <<<"$OUT")" && pass "arch: headless start reaches windowing stage (no loader error)" || fail "arch: loader error on headless start"
  info "arch headless output: $(sed -n '/^--- headless/,/^exit=/p' <<<"$OUT" | tail -n +2 | head -5 | tr '\n' '|')"
  [ "$(sed -n '/^--- after uninstall/{n;p}' <<<"$OUT")" = 3 ] && pass "arch: uninstall removed binary, desktop file and licenses" || fail "arch: files left after uninstall"
}

t_nompv() {
  image archlinux:latest || return
  cat > "$WORK/nompv.sh" <<'EOF'
. /w/pre.sh
pacman -Syu --noconfirm >/dev/null 2>&1
pacman -S --noconfirm --needed libglvnd vulkan-icd-loader wayland libxkbcommon libxkbcommon dbus >/dev/null 2>&1
pacman -Q mpv >/dev/null 2>&1 && echo "mpv present (unexpected)" || echo "mpv absent"
./install.sh --no-deps >/dev/null; echo "install_rc=$?"
echo "--- ldd"; ldd /usr/local/bin/nocturne | grep -c 'not found'
echo "--- strings"; ldd /usr/local/bin/nocturne | grep -c libmpv
bash /w/headless.sh /usr/local/bin/nocturne
EOF
  run archlinux:latest nompv.sh
  grep -q 'mpv absent' <<<"$OUT" || { fail "nompv: could not set up a container without mpv"; return; }
  [ "$(sed -n '/^--- ldd/{n;p}' <<<"$OUT")" = 0 ] && pass "nompv: ldd has no 'not found' (other libs present)" || fail "nompv: ldd reports missing libraries"
  [ "$(sed -n '/^--- strings/{n;p}' <<<"$OUT")" = 0 ] && pass "nompv: binary has no libmpv DT_NEEDED (dlopen)" || fail "nompv: libmpv is linked"
  loader_ok "$(sed -n '/^--- headless/,/^exit=/p' <<<"$OUT")" && pass "nompv: no loader error without libmpv, reaches windowing stage" || fail "nompv: loader error without libmpv"
  info "nompv headless output: $(sed -n '/^--- headless/,/^exit=/p' <<<"$OUT" | tail -n +2 | head -5 | tr '\n' '|')"
}

t_apt() { # image
  local img=$1 tag=${1//[:\/]/_}
  image "$img" || { fail "$img: cannot pull image"; return; }
  cat > "$WORK/$tag.sh" <<'EOF'
. /w/pre.sh
NOCTURNE_YES=1 ./install.sh; echo "install_rc=$?"
echo "--- dpkg"; for p in libmpv2 mpv libegl1 libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libdbus-1-3; do dpkg -s $p >/dev/null 2>&1 && echo "have $p" || echo "MISSING $p"; done
echo "--- ldd"; ldd /usr/local/bin/nocturne 2>&1 | grep -E 'not found|GLIBC' | head -5
bash /w/headless.sh /usr/local/bin/nocturne
EOF
  run "$img" "$tag.sh"
  grep -q 'install_rc=0' <<<"$OUT" && pass "$img: install.sh dependency step and install, exit 0" || fail "$img: install.sh failed: $(grep -E '^E:|error' <<<"$OUT" | head -3 | tr '\n' '|')"
  grep -q 'MISSING' <<<"$OUT" && fail "$img: package missing after install ($(grep MISSING <<<"$OUT" | tr '\n' ' '))" || pass "$img: apt resolved and installed every package name"
  info "$img: binary run: $(sed -n '/^--- ldd/,$p' <<<"$OUT" | head -8 | tr '\n' '|')"
}

t_fedora() {
  image fedora:latest || { fail "fedora: cannot pull image"; return; }
  cat > "$WORK/fedora.sh" <<'EOF'
. /w/pre.sh
echo "--- per-package availability"
for p in mpv-libs mpv libglvnd-egl vulkan-loader libwayland-client libwayland-egl libxkbcommon dbus-libs; do
  if dnf -q repoquery --available "$p" 2>/dev/null | grep -q .; then echo "available $p"; else echo "UNAVAILABLE $p"; fi
done
echo "--- install.sh"
NOCTURNE_YES=1 ./install.sh 2>&1 | tail -15; echo "install_rc=${PIPESTATUS[0]}"
EOF
  # fedora.sh uses PIPESTATUS, so run it with bash
  OUT=$(docker run --rm -v "$WORK:/w" fedora:latest bash /w/fedora.sh 2>&1); RC=$?
  grep -q '^UNAVAILABLE' <<<"$OUT" && info "fedora: not in the default repos: $(grep ^UNAVAILABLE <<<"$OUT" | tr '\n' ' ')" || pass "fedora: every package name resolves in default repos"
  info "fedora: $(sed -n '/^--- install.sh/,$p' <<<"$OUT" | grep -E 'missing:|running:|Installing|mpv|error|rc=' | head -8 | tr '\n' '|')"
  grep -q 'install_rc=0' <<<"$OUT" && pass "fedora: install.sh exit 0" || info "fedora: install.sh did not complete: $(sed -n '/^--- install.sh/,$p' <<<"$OUT" | tail -6 | tr '\n' '|')"
}

t_pkgbuild() {
  image archlinux:base-devel || { fail "pkgbuild: cannot pull archlinux:base-devel"; return; }
  local pv; pv=$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)
  git archive --format=tar.gz --prefix="nocturne-$pv/" -o "$WORK/nocturne-$pv.tar.gz" HEAD
  mkdir -p "$WORK/pkg"; cp "$WORK/nocturne-$pv.tar.gz" "$WORK/pkg/"
  # Local source instead of the release tag; checksum skipped for the temporary copy only.
  sed -e "s|^source=.*|source=(\"nocturne-$pv.tar.gz\")|" -e "s|^sha256sums=.*|sha256sums=('SKIP')|" packaging/PKGBUILD > "$WORK/pkg/PKGBUILD"
  cat > "$WORK/pkg.sh" <<'EOF'
pacman -Syu --noconfirm sudo >/dev/null 2>&1
useradd -m builder && echo 'builder ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/b
cp -r /w/pkg /home/builder/pkg && chown -R builder /home/builder/pkg
su builder -c 'cd ~/pkg && makepkg -si --noconfirm --nocheck' > /tmp/mk.log 2>&1; echo "makepkg_rc=$?"
tail -25 /tmp/mk.log
echo "--- installed"; pacman -Q nocturne mpv 2>&1
echo "--- deps"; pacman -Qi nocturne | grep -E '^Depends On'
echo "--- ldd"; ldd /usr/bin/nocturne | grep -c 'not found'
bash /w/headless.sh /usr/bin/nocturne
EOF
  run archlinux:base-devel pkg.sh
  grep -q 'makepkg_rc=0' <<<"$OUT" && pass "pkgbuild: makepkg -si succeeded" || { fail "pkgbuild: makepkg failed"; echo "$OUT" | tail -30; return; }
  grep -qE '^mpv ' <<<"$OUT" && pass "pkgbuild: package pulled in mpv" || fail "pkgbuild: mpv not installed"
  [ "$(sed -n '/^--- ldd/{n;p}' <<<"$OUT")" = 0 ] && pass "pkgbuild: ldd clean" || fail "pkgbuild: ldd missing libs"
  loader_ok "$(sed -n '/^--- headless/,/^exit=/p' <<<"$OUT")" && pass "pkgbuild: headless start, no loader error" || fail "pkgbuild: loader error"
  info "pkgbuild: $(grep -E '^Depends On' <<<"$OUT")  (check() skipped via --nocheck)"
}

targets=("$@"); [ ${#targets[@]} -gt 0 ] || targets=(all)
for t in "${targets[@]}"; do
  case $t in
  all) t_arch; t_nompv; t_apt debian:stable; t_apt ubuntu:latest; t_fedora ;;
  arch) t_arch ;; nompv) t_nompv ;; debian) t_apt debian:stable ;; ubuntu) t_apt ubuntu:latest ;;
  fedora) t_fedora ;; pkgbuild) t_pkgbuild ;;
  *) echo "unknown target $t" >&2; exit 2 ;;
  esac
done
echo "failures: $FAILS"
[ "$FAILS" -eq 0 ]
