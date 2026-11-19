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
clip_is() { sleep 0.6; [ "$(run wl-paste -n 2>/dev/null)" = "$1" ]; }

$H start 1280x800 >/dev/null || exit 1
run scripts/run-isolated.sh "$BIN" >"$T/app.log" 2>&1 &
pid=$!
sleep 6
for _ in 1 2 3; do key -k Return; done # Get started, A Jellyfin server, Connect a server
shot empty
key "$ADDR"
shot typed
check "typing shows text in the field" differs empty typed
# Synthetic ctrl chords occasionally go missing (1 run in 5 lost the very first one), so each repeatable step retries up
# to three times from a known clipboard sentinel; the check is still the real outcome.
# try3 <sentinel> <c|x>: set the sentinel, send ctrl+a then ctrl+<c|x>, until the clipboard holds the address.
try3() {
  local n
  for n in 1 2 3; do
    echo "$1" | run wl-copy -n
    key -M ctrl a -m ctrl
    key -M ctrl "$2" -m ctrl
    clip_is "$ADDR" && return 0
  done
  return 1
}
try3 SENT1 c
check "Ctrl+A, Ctrl+C copies the field" clip_is "$ADDR"
try3 SENT1 x
shot cut
check "Ctrl+X empties the field" same empty cut
check "Ctrl+X keeps the text on the clipboard" clip_is "$ADDR"
# The field moves when a validation message appears, so a fixed crop can miss it: prove the paste by state. Paste into
# the empty field, then copy it back over a sentinel; the clipboard only holds the address again if the paste landed.
key -M ctrl v -m ctrl
try3 SENT3 c
check "Ctrl+V restores the text" clip_is "$ADDR"
echo SENT2 | run wl-copy -n
key -k Tab
key -M ctrl a -m ctrl
key -M ctrl c -m ctrl
check "Tab leaves the field (Ctrl+C copies nothing)" clip_is SENT2
key -M shift -k Tab -m shift
sleep 1
try3 SENT4 c
check "Shift+Tab returns to the field" clip_is "$ADDR"
echo "SKIP sign-in password field: needs a reachable server"
exit $FAIL
