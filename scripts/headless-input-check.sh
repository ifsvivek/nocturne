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
# One wtype process is one virtual keyboard (it vanishes when the process exits), and the app only receives its keys when the
# window is re-focused AFTER the compositor has seen that keyboard. A fixed sleep raced the keyboard's start-up and lost about
# one chord in five, so a key is sent as: start wtype with a long lead-in sleep, wait until sway lists the new keyboard,
# refocus, check that NOCTURNE really holds the focus, then let the keys fire.
focused() { [ "$(run swaymsg -t get_tree -r 2>/dev/null | jq -r '.. | objects | select(.focused == true) | .name' | head -1)" = NOCTURNE ]; }
key() {
  run wtype -s 2000 "$@" >/dev/null 2>&1 &
  local w=$! n
  local k=0 f=0
  for n in $(seq 60); do run swaymsg -t get_inputs -r 2>/dev/null | grep -q wlr_virtual_keyboard && { k=$n; break; }; sleep 0.05; done
  for n in 1 2 3 4 5; do
    run swaymsg 'focus parent; focus child' >/dev/null 2>&1
    run swaymsg '[title="NOCTURNE"] focus' >/dev/null 2>&1
    focused && { f=$n; break; }
    sleep 0.1
  done
  [ -n "${DEBUG_KEYS:-}" ] && echo "KEY $* kbd_poll=$k focus_try=$f" >&2
  wait "$w" 2>/dev/null
  sleep 0.3
}
chord() { key -M ctrl "$1" -m ctrl; }
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
sleep 1.5 # the last screen animates in
shot empty
key "$ADDR"
shot typed
check "typing shows text in the field" differs empty typed
# Every step starts from a known clipboard sentinel and selects the whole field first, so a lost chord is simply sent once more
# (retry2); the check itself stays the real outcome.
retry2() { "$@" || { echo "RETRY $*"; "$@"; } || { echo "RETRY2 $*"; "$@"; }; }
copy_a() { echo "$1" | run wl-copy -n; chord a; chord c; clip_is "$ADDR"; }
cut_a() { echo SENT1 | run wl-copy -n; chord a; chord x; clip_is "$ADDR"; }
paste_a() { echo "$ADDR" | run wl-copy -n; chord a; chord v; copy_a SENT3; }
retry2 copy_a SENT1
check "Ctrl+A, Ctrl+C copies the field" clip_is "$ADDR"
retry2 cut_a
check "Ctrl+X keeps the text on the clipboard" clip_is "$ADDR"
# The empty field is proved by state like the rest (a pixel diff of the field flaked on the blinking caret): select-all + copy on an empty field leaves the sentinel alone.
echo SENT5 | run wl-copy -n; chord a; chord c
check "Ctrl+X empties the field" clip_is SENT5
# The field moves when a validation message appears, so a fixed crop can miss it: prove the paste by state. Paste over the
# (empty) field, then copy it back over a sentinel; the clipboard only holds the address again if the paste landed.
retry2 paste_a
check "Ctrl+V restores the text" clip_is "$ADDR"
# Tab and Shift+Tab move the focus, so they are not simply repeatable: each is pressed until its state check passes, with a bounded count. After Tab the field no longer has the focus, so select-all + copy leaves the sentinel
# alone (a lost Tab shows as the address); after Shift+Tab the field has it again, so the copy works (a chord that went missing is retried before the key is pressed again).
tab_a() { echo SENT2 | run wl-copy -n; chord a; chord c; clip_is SENT2; }
tabs=0
for _ in 1 2 3; do key -k Tab; tabs=$((tabs + 1)); tab_a && break; echo "RETRY tab"; done
check "Tab leaves the field (Ctrl+C copies nothing)" clip_is SENT2
for _ in $(seq $((tabs + 1))); do
  key -M shift -k Tab -m shift
  copy_a SENT4 || copy_a SENT4 && break
  echo "RETRY shift+tab"
done
check "Shift+Tab returns to the field" clip_is "$ADDR"
echo "SKIP sign-in password field: needs a reachable server"
exit $FAIL
