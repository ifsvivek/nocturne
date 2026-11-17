#!/usr/bin/env bash
# Cross-compile CHECK of the app for Windows and macOS, inside a Docker container (nothing is
# installed on the host). `cargo check` never links, so no Windows/macOS SDK is needed, only a C
# compiler for build scripts (ring, ...). This proves the code compiles for the target; it does
# NOT prove it runs (docs/PLATFORMS.md: macOS/Windows runtime is untested).
#
#   scripts/check-cross.sh            # windows, then macos
#   scripts/check-cross.sh windows    # or: macos
#
# Caches (registry, target dir) live in docker volumes, so reruns are incremental.
# Env: CROSS_IMAGE (default rust:1-bookworm).
set -uo pipefail
cd "$(dirname "$0")/.."

IMAGE=${CROSS_IMAGE:-rust:1-bookworm}
targets=("${@:-windows macos}")
[ $# -eq 0 ] && targets=(windows macos)

run() { # run <label> <rust target> <apt packages> <cargo args...>
  local label=$1 triple=$2 pkgs=$3
  shift 3
  echo "== $label ($triple): cargo check $*"
  docker run --rm \
    -v "$PWD":/src:ro \
    -v nocturne-cross-cargo:/usr/local/cargo/registry \
    -v nocturne-cross-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_TERM_COLOR=never \
    -e CC_x86_64_apple_darwin=clang \
    -w /src "$IMAGE" bash -c "
      set -e
      apt-get update -qq >/dev/null && apt-get install -y -qq $pkgs >/dev/null
      rustup target add $triple >/dev/null 2>&1
      cargo check --locked --target $triple $* 2>&1 | tail -n 60
      exit \${PIPESTATUS[0]}"
}

rc=0
for t in "${targets[@]}"; do
  case $t in
    windows)
      run windows x86_64-pc-windows-gnu gcc-mingw-w64-x86-64 -p nocturne-app || rc=1 ;;
    macos)
      # Whole app first; if a C dependency blocks it (no Apple SDK), fall back to the crates
      # that hold our OS-specific code so the failure is pinned to the dependency, not to us.
      if ! run macos x86_64-apple-darwin clang -p nocturne-app; then
        echo "-- macOS app check failed; checking our own crates only"
        run macos-libs x86_64-apple-darwin clang \
          -p nocturne-session -p nocturne-player -p nocturne-local -p nocturne-core || rc=1
        rc=1
      fi ;;
    *) echo "unknown target $t (windows|macos)"; rc=2 ;;
  esac
done
exit $rc
