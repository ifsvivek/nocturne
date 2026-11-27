#!/usr/bin/env bash
# Build the Linux binaries ONCE, in a debian:stable container (oldest glibc we ship against), into dist/bin/nocturne-<profile> and dist/bin/nocturne-gateway-<profile> (the optional gateway companion).
# The tarball, .deb and .rpm all reuse it (a newer-glibc distro such as Fedora runs it fine).
#   usage: [PROFILE=release|fast-release] [IMG=debian:stable] [PKG=nocturne-app] scripts/build-linux-bin.sh
# release = published artifacts (fat LTO); fast-release = QA and package tests (thin LTO, 16 codegen units, see Cargo.toml).
# One persistent named volume per base image holds the cargo registry, rustup and target dir, so reruns are incremental;
# after each build the volume is trimmed to CACHE_CAP_GB (default 15): other profiles first, everything if still over.
set -euo pipefail
cd "$(dirname "$0")/.."
PKG="${PKG:-nocturne-app}"; IMG="${IMG:-debian:stable}"; PROFILE="${PROFILE:-release}"
CACHE_CAP_GB="${CACHE_CAP_GB:-15}"
JOBS="${JOBS:-$(nproc)}"  # JOBS=6 leaves the desktop responsive; the container is also limited to that many CPUs
case "$PROFILE" in release|fast-release) ;; *) echo "PROFILE must be release or fast-release" >&2; exit 2 ;; esac
scripts/cache-budget.sh --quiet || true
VOL="nocturne-cache-$(echo "$IMG" | tr ':/' '--')"
BIMG="nocturne-build:$(echo "$IMG" | tr ':/' '--')"
mkdir -p dist/bin
# Build dependencies live in a small local image (built once), not reinstalled on every run.
docker image inspect "$BIMG" >/dev/null 2>&1 || docker build -q -t "$BIMG" - >/dev/null <<DOCKERFILE
FROM $IMG
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends build-essential pkg-config curl ca-certificates libdbus-1-dev libudev-dev libegl-dev && rm -rf /var/lib/apt/lists/*
DOCKERFILE

docker run --rm --cpus "$JOBS" -e CARGO_BUILD_JOBS="$JOBS" -v "$PWD":/src:ro -v "$PWD/dist/bin":/out -v "$VOL":/cache -e PKG="$PKG" -e PROFILE="$PROFILE" -e CAP="$CACHE_CAP_GB" -e HOST_UID="$(id -u)" "$BIMG" bash -euc '
export CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target RUSTUP_HOME=/cache/rustup
[ -x /cache/cargo/bin/cargo ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
export PATH=/cache/cargo/bin:$PATH
cd /src
RUSTFLAGS="--remap-path-prefix=/cache/cargo=/cargo --remap-path-prefix=/src=/src" cargo build --profile "$PROFILE" --locked -p "$PKG" -p nocturne-gateway
install -m755 /cache/target/$PROFILE/nocturne /out/nocturne-$PROFILE
install -m755 /cache/target/$PROFILE/nocturne-gateway /out/nocturne-gateway-$PROFILE
chown $HOST_UID /out/nocturne-$PROFILE /out/nocturne-gateway-$PROFILE
# bound the volume
over() { [ "$(du -sk /cache | cut -f1)" -gt $((CAP * 1024 * 1024)) ]; }
if over; then
  for p in debug release fast-release; do [ "$p" = "$PROFILE" ] || rm -rf /cache/target/$p; done
  find /cache/target -name incremental -type d -prune -exec rm -rf {} +
  if over; then rm -rf /cache/target/*; echo "cache volume over ${CAP} GB: target cleared"; fi
fi
'
echo "dist/bin/nocturne-$PROFILE ($(du -h "dist/bin/nocturne-$PROFILE" | cut -f1)); cache volume $VOL"
