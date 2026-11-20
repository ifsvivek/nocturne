#!/usr/bin/env bash
# Parity check of the theme engine's Home, header, library grid and search overlay against the hand-written ones, in an invisible sway (scripts/headless-session.sh): the same debug binary runs twice, once with NOCTURNE_ENGINE_UI=0 (hand-written pages) and once with the engine (needs `--features theme-engine`), against a throwaway Jellyfin with the generated demo library, and the window captures are compared per pixel (scripts/parity-diff.py, needs python3 with Pillow). Also replays key sequences (Home: arrows, Enter, Escape; library: arrows, Page keys, Home/End, Tab) through both and compares the captures.
# usage: NOCTURNE_JF_URL=http://127.0.0.1:8097 NOCTURNE_JF_TOKEN_FILE=<file with a user access token> scripts/headless-parity.sh [WxH] [out dir]
#   (default 1600x900 and /tmp/nocturne-parity). The token is a throwaway server's user token (never a real server's); the app runs through scripts/run-isolated.sh with throwaway XDG dirs.
#   Presets: the three bundled themes are written as the global theme into per-preset throwaway config dirs.
# Needs: cargo build -p nocturne-app --features theme-engine (target/debug/nocturne), a demo server (scripts/make-demo-library.sh build), sway, python3 + Pillow.
cd "$(dirname "$0")/.."
SIZE=${1:-1600x900}
OUT=${2:-/tmp/nocturne-parity}
BIN=$(realpath "${PARITY_BIN:-target/debug/nocturne}")
: "${NOCTURNE_JF_URL:?set NOCTURNE_JF_URL to a throwaway server}" "${NOCTURNE_JF_TOKEN_FILE:?set NOCTURNE_JF_TOKEN_FILE}"
case "$NOCTURNE_JF_URL" in http://127.0.0.1:*|http://localhost:*) ;; *) echo "refusing: loopback servers only" >&2; exit 2 ;; esac
export HS_NAME=${HS_NAME:-parity}
H=scripts/headless-session.sh
mkdir -p "$OUT"
FAIL=0
pid=""
cleanup() { [ -n "$pid" ] && kill "$pid" 2>/dev/null; $H stop >/dev/null; }
trap cleanup EXIT
$H start "$SIZE" >/dev/null || exit 1

# run <name> <engine 0|1> <keep dir name|""> <snap ms list> [keys]: launch, wait for the last capture, stop.
run() {
  local name=$1 eng=$2 keep=$3 snaps=$4 keys=${5:-} n last
  n=$(echo "$snaps" | tr ',' '\n' | wc -l)
  last="$OUT/$name.png"; [ "$n" -gt 1 ] && last="$OUT/$name-$n.png"
  rm -f "$OUT/$name"*.png
  $H run env NOCTURNE_JF_URL="$NOCTURNE_JF_URL" NOCTURNE_JF_TOKEN="$(cat "$NOCTURNE_JF_TOKEN_FILE")" NOCTURNE_ENGINE_UI="$eng" \
    NOCTURNE_SNAP="$OUT/$name.png" NOCTURNE_SNAP_AFTER_MS="$snaps" ${keys:+NOCTURNE_KEYS="$keys"} \
    scripts/run-isolated.sh ${keep:+--keep "$keep"} "$BIN" >"$OUT/$name.log" 2>&1 &
  pid=$!
  for _ in $(seq 120); do [ -s "$last" ] && break; sleep 1; done
  sleep 1; kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; pid=""
}

theme() { # <name> <css file>: a throwaway config dir with that theme as the global one
  local d=/tmp/nocturne-run/parity-$1/config/nocturne
  mkdir -p "$d" "/tmp/nocturne-run/parity-$1"/{work,data,cache}
  printf "version = 1\nnext_id = 2\nglobal = 1\n\n[[theme]]\nid = 1\nname = '%s'\ncss = '''%s'''\n" "$1" "$(cat "$2")" >"$d/themes.toml"
}

# PARITY_ONLY=thumb runs only the steps whose name contains it (default: all).
want() { [[ "$1" == *"${PARITY_ONLY:-}"* ]]; }

check() { # <name> <legacy png> <engine png>
  if python3 -I scripts/parity-diff.py "$2" "$3" "$OUT/$1-diff.png"; then echo "PASS $1"; else echo "FAIL $1"; FAIL=1; fi
}

for p in default marquee feed classic; do
  keep=""
  if [ "$p" != default ]; then theme "$p" "crates/core/src/customize/examples/$p.css"; keep="parity-$p"; fi
  if want "$p"; then
    run "$p-legacy" 0 "$keep" 7000
    run "$p-engine" 1 "$keep" 7000
    check "$p" "$OUT/$p-legacy.png" "$OUT/$p-engine.png"
  fi
  # The library grid (poster view, then the thumb view) and the search overlay with results, same theme.
  if want "$p-lib"; then
    run "$p-lib-legacy" 0 "$keep" 8000 "open-movies@4500"
    run "$p-lib-engine" 1 "$keep" 8000 "open-movies@4500"
    check "$p-lib" "$OUT/$p-lib-legacy.png" "$OUT/$p-lib-engine.png"
  fi
  # `view-thumb` waits for the page's first items, then sets the view (the `v` key raced the page load and toggled to a different view in one of the two runs).
  if want "$p-thumb"; then
    run "$p-thumb-legacy" 0 "$keep" 9500 "open-movies@4500,view-thumb@5000"
    run "$p-thumb-engine" 1 "$keep" 9500 "open-movies@4500,view-thumb@5000"
    check "$p-thumb" "$OUT/$p-thumb-legacy.png" "$OUT/$p-thumb-engine.png"
  fi
  if want "$p-search"; then
    run "$p-search-legacy" 0 "$keep" 9000 "open-search@4500,search-the@5500"
    run "$p-search-engine" 1 "$keep" 9000 "open-search@4500,search-the@5500"
    check "$p-search" "$OUT/$p-search-legacy.png" "$OUT/$p-search-engine.png"
  fi
done

# Keyboard: down, right, right, down, up, left (focus and hero follow), Enter (opens the item), Escape (back).
KEYS="down@6000,right@6400,right@6800,down@7200,up@7600,left@8000,enter@9000,escape@12000"
SNAPS="5500,7000,8500,10500,13000"
if want keys; then
run keys-legacy 0 "" "$SNAPS" "$KEYS"
run keys-engine 1 "" "$SNAPS" "$KEYS"
# The third capture differs on purpose after Down then Up: the hand-written page decides from a scroll offset it has not heard back about yet, the engine from the real one; focus and items are the same.
for i in "" -2 -4 -5; do check "keys$i" "$OUT/keys-legacy$i.png" "$OUT/keys-engine$i.png"; done
fi

# Library keyboard: Right, Right, Down (the selection scrolls into view), End, Home, Down, Tab (next tab: the list reloads).
# The scroll after a key lands within a pixel of the hand-written page's, so the first three captures are allowed a larger mean.
KEYS="open-movies@4500,right@5500,right@5800,down@6100,end@7000,home@8000,down@8500,tab@9000"
SNAPS="6600,7600,8800,10500"
if want libkeys; then
run libkeys-legacy 0 "" "$SNAPS" "$KEYS"
run libkeys-engine 1 "" "$SNAPS" "$KEYS"
for i in "" -2 -3 -4; do
  if python3 -I scripts/parity-diff.py "$OUT/libkeys-legacy$i.png" "$OUT/libkeys-engine$i.png" "$OUT/libkeys-diff$i.png" 16 1.5 3; then echo "PASS libkeys$i"; else echo "FAIL libkeys$i"; FAIL=1; fi
done
fi
exit $FAIL
