#!/usr/bin/env bash
# Real-input check of the connect form in an invisible sway (scripts/headless-session.sh): typing, select-all/copy/cut/paste and Tab/Shift+Tab, with synthetic keys, wl-paste and captures. Builds nothing.
# usage: scripts/headless-input-check.sh [path/to/nocturne]   (default target/debug/nocturne); exits 1 when a check fails.
# The sign-in form (password field) needs a reachable server and is not covered here.
cd "$(dirname "$0")/.."
H=scripts/headless-session.sh
BIN=$(realpath "${1:-target/debug/nocturne}")
T=$(mktemp -d /tmp/nocturne-input-check.XXXXXX)
ADDR="http://abc.test:8096"
FAIL=0
pid=""
cleanup() { [ -n "$pid" ] && kill "$pid" 2>/dev/null; $H stop >/dev/null; [ -n "${KEEP:-}" ] || rm -rf "$T"; }
trap cleanup EXIT
run() { $H run "$@"; }
# Synthetic keys only arrive when the window is re-focused after wtype's virtual keyboard connects.
key() {
  (run wtype -s 900 "$@" >/dev/null 2>&1 &)
  sleep 0.4
  run swaymsg 'focus parent; focus child' >/dev/null 2>&1
  run swaymsg '[title="NOCTURNE"] focus' >/dev/null 2>&1
  sleep 1.3
}
# The server address field on the 1280x800 output.
shot() { run grim -t ppm -g "444,537 392x44" "$T/$1.ppm"; }
ndiff() { cmp -l "$T/$1.ppm" "$T/$2.ppm" 2>/dev/null | wc -l; }
differs() { [ "$(ndiff "$1" "$2")" -gt 600 ]; }
same() { [ "$(ndiff "$1" "$2")" -le 600 ]; }
check() { if "${@:2}"; then echo "PASS $1"; else echo "FAIL $1"; FAIL=1; fi; }
clip_is() { [ "$(run wl-paste -n 2>/dev/null)" = "$1" ]; }

$H start 1280x800 >/dev/null || exit 1
run scripts/run-isolated.sh "$BIN" >"$T/app.log" 2>&1 &
pid=$!
sleep 6
for _ in 1 2 3; do key -k Return; done # Get started, A Jellyfin server, Connect a server
shot empty
key "$ADDR"
shot typed
check "typing shows text in the field" differs empty typed
echo SENT1 | run wl-copy -n
key -M ctrl a -m ctrl
key -M ctrl c -m ctrl
check "Ctrl+A, Ctrl+C copies the field" clip_is "$ADDR"
key -M ctrl x -m ctrl
shot cut
check "Ctrl+X empties the field" same empty cut
check "Ctrl+X keeps the text on the clipboard" clip_is "$ADDR"
key -M ctrl v -m ctrl
shot pasted
check "Ctrl+V restores the text" differs empty pasted
echo SENT2 | run wl-copy -n
key -k Tab
key -M ctrl a -m ctrl
key -M ctrl c -m ctrl
check "Tab leaves the field (Ctrl+C copies nothing)" clip_is SENT2
key -M shift -k Tab -m shift
key -M ctrl a -m ctrl
key -M ctrl c -m ctrl
check "Shift+Tab returns to the field" clip_is "$ADDR"
echo "SKIP sign-in password field: needs a reachable server"
exit $FAIL
