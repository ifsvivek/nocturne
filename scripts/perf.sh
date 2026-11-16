#!/usr/bin/env bash
# Footprint measurements for a NOCTURNE binary (see docs/PLATFORMS.md, docs/dev/PERF.md). Linux only.
# usage: scripts/perf.sh [BINARY] [LABEL]     (default target/release/nocturne)
#   env: PERF_DEMO_ENV  test.env of the throwaway demo server (default /tmp/nocturne-demo/jf/test.env); when it
#                       exists, the "loaded home" row runs in dev-token mode against it, otherwise it prints n/a.
#        PERF_RUNS (5)  PERF_IDLE_WAIT (10 s)  PERF_CPU_SECS (30)
# Needs jq and a running Wayland session for the window rows; start time needs hyprctl (else n/a). It opens real windows.
# Every run uses throwaway XDG dirs, NOCTURNE_KEYRING_SERVICE=nocturne-test and a cwd outside the repo (so no
# repo .env is ever read). Never point it at a real server. Output: a markdown table on stdout.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$(readlink -f "${1:-$HERE/target/release/nocturne}")"
LABEL="${2:-$(basename "$BIN")}"
RUNS="${PERF_RUNS:-5}"; WAIT="${PERF_IDLE_WAIT:-10}"; CPUS="${PERF_CPU_SECS:-30}"
DEMO_ENV="${PERF_DEMO_ENV:-/tmp/nocturne-demo/jf/test.env}"
[ -x "$BIN" ] || { echo "no binary: $BIN" >&2; exit 1; }
TMP="$(mktemp -d /tmp/nocturne-perf.XXXXXX)"; PID=""
cleanup() { [ -z "$PID" ] || kill "$PID" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
HAVE_HC=""; if command -v hyprctl >/dev/null && hyprctl version >/dev/null 2>&1; then HAVE_HC=1; fi
TCK=$(getconf CLK_TCK)
now_ms() { echo $(( $(date +%s%N) / 1000000 )); }
mib() { awk -v b="$1" 'BEGIN{printf "%.2f", b/1048576}'; }

# --- sizes ---
bytes=$(stat -c %s "$BIN")
tar_row="n/a"; t=$(ls -t "$HERE"/dist/*.tar.gz 2>/dev/null | head -1 || true)
if [ -n "$t" ]; then tar_row="$(stat -c %s "$t") B ($(mib "$(stat -c %s "$t")") MiB)"; fi
S="$TMP/stage"; mkdir -p "$S/icons"            # same layout as scripts/package.sh's tarball
cp "$BIN" "$S/nocturne"; cp "$HERE"/LICENSE "$HERE"/THIRD_PARTY_NOTICES.md "$S/"
cp "$HERE"/packaging/io.github.* "$HERE"/packaging/nocturne.svg "$HERE"/packaging/install.sh "$S/"
cp "$HERE"/packaging/icons/*.png "$S/icons/"
(cd "$S" && DESTDIR="$TMP/root" ./install.sh install >/dev/null)
inst=$(find "$TMP/root" -type f -printf '%s\n' | awk '{s+=$1} END{print s}')

# --- one run: sets MAPPED_MS, INTERACTIVE_MS, PID; $1 = mode (empty|demo) ---
launch() {
  local mode=$1 d="$TMP/run.$RANDOM"; mkdir -p "$d/cfg" "$d/data" "$d/cache" "$d/cwd"
  local envs=(XDG_CONFIG_HOME="$d/cfg" XDG_DATA_HOME="$d/data" XDG_CACHE_HOME="$d/cache"
              NOCTURNE_KEYRING_SERVICE=nocturne-test NOCTURNE_TIMING=1 NOCTURNE_JF_TOKEN= NOCTURNE_JF_URL=)
  if [ "$mode" = demo ]; then
    local key; key=$(sed -n 's/^NOCTURNE_JF_TEST_TOKEN=//p' "$DEMO_ENV")
    envs+=(NOCTURNE_JF_TOKEN="$key" NOCTURNE_JF_URL="http://127.0.0.1:${NOCTURNE_JF_TEST_PORT:-8097}")
  fi
  LOG="$d/out.log"
  local t0; t0=$(now_ms)
  (cd "$d/cwd" && exec env "${envs[@]}" "$BIN" >"$LOG" 2>&1) & PID=$!
  MAPPED_MS="n/a"
  if [ -n "$HAVE_HC" ]; then
    for _ in $(seq 1 1000); do   # 10 s ceiling
      kill -0 "$PID" 2>/dev/null || break
      if hyprctl clients -j 2>/dev/null | jq -e --argjson p "$PID" 'any(.[]; .pid == $p and .mapped)' >/dev/null; then MAPPED_MS=$(( $(now_ms) - t0 )); break; fi
      sleep 0.01
    done
  fi
  sleep 1
  # Timing line (Iced UI only): any line mentioning "interactive" with a number in ms.
  INTERACTIVE_MS=$(grep -i interactive "$LOG" 2>/dev/null | grep -oE '[0-9]+(\.[0-9]+)? ?ms' | head -1 | tr -d ' ms' || true)
  INTERACTIVE_MS="${INTERACTIVE_MS:-n/a}"
}
stop() { kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; PID=""; }
median() { sort -n | awk '{a[NR]=$1} END{if(NR) print a[int((NR+1)/2)]; else print "n/a"}'; }

# --- start time (sign-in screen, throwaway empty config) ---
maps=(); ints=()
for _ in $(seq 1 "$RUNS"); do
  launch empty
  if [ "$MAPPED_MS" != n/a ]; then maps+=("$MAPPED_MS"); fi
  if [ "$INTERACTIVE_MS" != n/a ]; then ints+=("$INTERACTIVE_MS"); fi
  stop
done
map_med=$(printf '%s\n' "${maps[@]:-}" | grep . | median || true)
int_med=$(printf '%s\n' "${ints[@]:-}" | grep . | median || true)

# --- idle RSS / CPU ---
idle() { # mode -> "RSS MiB / CPU %"
  launch "$1"; sleep "$WAIT"
  if ! kill -0 "$PID" 2>/dev/null; then stop; echo "exited / exited"; return; fi
  local rss a b; rss=$(awk '/VmRSS/{print $2}' /proc/$PID/status)
  a=$(awk '{print $14+$15}' /proc/$PID/stat); sleep "$CPUS"; b=$(awk '{print $14+$15}' /proc/$PID/stat)
  stop
  awk -v r="$rss" -v a="$a" -v b="$b" -v t="$TCK" -v s="$CPUS" 'BEGIN{printf "%.1f / %.2f", r/1024, (b-a)/t/s*100}'
}
signin=$(idle empty)
if [ -f "$DEMO_ENV" ]; then home=$(idle demo); else home="n/a / n/a"; fi

cat <<OUT
| Metric | $LABEL |
|---|---|
| Binary | $bytes B ($(mib "$bytes") MiB) |
| Tarball | $tar_row |
| Installed (install.sh, DESTDIR) | $inst B ($(mib "$inst") MiB) |
| Start to window mapped, median of $RUNS (ms) | ${map_med:-n/a} |
| Start to interactive, NOCTURNE_TIMING=1 (ms) | ${int_med:-n/a} |
| Idle sign-in: RSS MiB after ${WAIT}s / CPU % over ${CPUS}s | $signin |
| Idle loaded home (demo, dev token): RSS MiB / CPU % | $home |
| CPU during 60 s 1080p embedded playback | manual |
OUT
# Manual row: play a 1080p demo clip embedded, then with the app pid:
#   a=$(awk '{print $14+$15}' /proc/PID/stat); sleep 60; b=$(awk '{print $14+$15}' /proc/PID/stat)
#   echo "($b-$a)/$(getconf CLK_TCK)/60*100" | bc -l
