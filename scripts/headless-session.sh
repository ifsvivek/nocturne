#!/usr/bin/env bash
# An invisible Wayland session for tests: a headless sway with its own display socket, so synthetic input (wtype), clipboard (wl-copy/wl-paste) and captures (grim) never reach the user's desktop. Audio is sent nowhere.
# usage: [HS_NAME=name] scripts/headless-session.sh start [WxH] | run <cmd...> | stop | status
# `run` refuses unless the target display belongs to the headless sway this script started; never point wtype at the user's session.
set -euo pipefail
# HS_NAME=<name> gives each agent its own session (directory, sway and display), so one agent's stop never ends another's.
D=/tmp/nocturne-headless${HS_NAME:+-$HS_NAME}; export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
alive() { [ -f "$D/pid" ] && kill -0 "$(cat "$D/pid")" 2>/dev/null; }
case "${1:-}" in
start)
  alive && { echo "running on $(cat "$D/display")"; exit 0; }
  command -v sway >/dev/null || { echo "needs sway (pacman -S sway)" >&2; exit 1; }
  rm -rf "$D"; mkdir -p "$D"; chmod 700 "$D"
  printf 'output HEADLESS-1 resolution %s\nfocus_follows_mouse no\n' "${2:-1920x1080}" > "$D/config"
  before=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' || true)
  env -u WAYLAND_DISPLAY -u DISPLAY WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman SWAYSOCK="$D/sway.sock" sway -c "$D/config" > "$D/log" 2>&1 &
  echo $! > "$D/pid"
  for _ in $(seq 50); do
    new=$(comm -13 <(echo "$before" | sort) <(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | sort) | head -1)
    [ -n "$new" ] && break; sleep 0.1
  done
  [ -n "${new:-}" ] || { echo "sway did not start, see $D/log" >&2; exit 1; }
  echo "$new" > "$D/display"; echo "headless display $new (pid $(cat "$D/pid"))" ;;
run)
  shift; alive || { echo "not running: start first" >&2; exit 1; }
  disp=$(cat "$D/display")
  # Never the caller's own session, and our sway must answer with headless outputs only.
  [ "$disp" != "${WAYLAND_DISPLAY:-}" ] || { echo "refusing: $disp is the calling session's display" >&2; exit 1; }
  outs=$(SWAYSOCK="$D/sway.sock" swaymsg -t get_outputs -r 2>/dev/null | grep -o '"name": "[^"]*"' || true)
  [ -n "$outs" ] && ! grep -qv HEADLESS <<<"$outs" || { echo "refusing: the sway behind $disp is not headless" >&2; exit 1; }
  # Silent: no PulseAudio/PipeWire server reachable, so mpv falls back to no audio output.
  exec env WAYLAND_DISPLAY="$disp" SWAYSOCK="$D/sway.sock" PULSE_SERVER=unix:/nonexistent PIPEWIRE_REMOTE=nonexistent "$@" ;;
stop) alive && kill "$(cat "$D/pid")"; rm -rf "$D"; echo stopped ;;
status) alive && echo "running on $(cat "$D/display")" || echo "not running" ;;
*) sed -n 2,4p "$0"; exit 1 ;;
esac
