#!/usr/bin/env bash
# Throwaway Jellyfin for NOCTURNE admin tests. NEVER touches /var/lib/jellyfin, ~/.config or port 8096.
# Usage: scripts/test-jellyfin.sh {start|stop|reset|status}
# Set NOCTURNE_JF_TEST_PORT (and NOCTURNE_JF_TEST_DIR) to run a second instance next to the default one.
# Everything (data, config, secrets, media, env file) lives under $NOCTURNE_JF_TEST_DIR (default /tmp/nocturne-jf-test).
set -euo pipefail

T="${NOCTURNE_JF_TEST_DIR:-/tmp/nocturne-jf-test}"
PORT="${NOCTURNE_JF_TEST_PORT:-8097}"
URL="http://127.0.0.1:$PORT"
ENVF="$T/test.env"
PIDF="$T/jellyfin.pid"

case "$T" in /tmp/?*) ;; *) echo "refusing: test dir must be under /tmp (got $T)" >&2; exit 1;; esac
case "$T" in *..*) echo "refusing: test dir must not contain '..' (got $T)" >&2; exit 1;; esac
# Canonical path (symlinks resolved) must still be strictly under /tmp.
case "$(realpath -m "$T")" in /tmp/?*) ;; *) echo "refusing: test dir resolves outside /tmp (got $T)" >&2; exit 1;; esac
for b in jellyfin curl jq ffmpeg; do command -v "$b" >/dev/null || { echo "missing: $b" >&2; exit 1; }; done

running() { [[ -f $PIDF ]] && kill -0 "$(cat "$PIDF")" 2>/dev/null; }
healthy() { curl -fs -m 2 "$URL/System/Info/Public" >/dev/null 2>&1; }
AUTH_ID='MediaBrowser Client="nocturne-test", Device="script", DeviceId="nocturne-test-script", Version="1"'
api() { # method path [json-body] [token]
  local m=$1 p=$2 b=${3:-} tok=${4:-}
  local a="$AUTH_ID"; [[ -n $tok ]] && a="$a, Token=\"$tok\""
  curl -fsS -m 30 -X "$m" -H "Authorization: $a" -H 'Content-Type: application/json' ${b:+-d "$b"} "$URL$p"
}

media() {
  mkdir -p "$T/media/Movies"
  local i
  for i in 1 2 3; do
    local f="$T/media/Movies/Test Clip $i (200$i)/Test Clip $i (200$i).mp4"
    [[ -f $f ]] && continue
    mkdir -p "$(dirname "$f")"
    ffmpeg -v error -y -f lavfi -i "testsrc2=size=160x90:rate=10:duration=2" \
      -f lavfi -i "sine=frequency=$((300 * i)):duration=2" -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$f"
  done
}

launch() {
  mkdir -p "$T"/{data,config,cache,log}
  # Bind to loopback only, port 8097, no UDP autodiscovery (7359 belongs to the real server), no UPnP/HTTPS.
  [[ -f $T/config/network.xml ]] || cat >"$T/config/network.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<NetworkConfiguration xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <BaseUrl />
  <EnableHttps>false</EnableHttps>
  <InternalHttpPort>$PORT</InternalHttpPort>
  <InternalHttpsPort>8921</InternalHttpsPort>
  <PublicHttpPort>$PORT</PublicHttpPort>
  <PublicHttpsPort>8921</PublicHttpsPort>
  <AutoDiscovery>false</AutoDiscovery>
  <EnableUPnP>false</EnableUPnP>
  <EnableIPv4>true</EnableIPv4>
  <EnableIPv6>false</EnableIPv6>
  <LocalNetworkAddresses>
    <string>127.0.0.1</string>
  </LocalNetworkAddresses>
</NetworkConfiguration>
XML
  setsid nohup jellyfin --nowebclient --nonetchange --service \
    --datadir "$T/data" --configdir "$T/config" --cachedir "$T/cache" --logdir "$T/log" \
    --ffmpeg "$(command -v ffmpeg)" >"$T/stdout.log" 2>&1 </dev/null &
  echo $! >"$PIDF"
  local n
  # Jellyfin answers once, then rebuilds its listener: only trust two successes a few seconds apart.
  for n in $(seq 60); do healthy && { sleep 4; healthy && return 0; }; running || break; sleep 1; done
  echo "jellyfin did not become healthy; see $T/stdout.log and $T/log" >&2; exit 1
}

rnd() { head -c 12 /dev/urandom | base64 | tr -d '/+='; }

seed() { # first run only: wizard, admin, api key, users, library
  local pw; pw=$(head -c 18 /dev/urandom | base64 | tr -d '/+=')
  ( umask 077; printf '%s\n' "$pw" >"$T/admin-password" )
  for _ in $(seq 60); do api GET /Startup/Configuration >/dev/null 2>&1 && break; sleep 1; done   # answers 503 for a while after /System/Info/Public is up
  api POST /Startup/Configuration '{"UICulture":"en-US","MetadataCountryCode":"US","PreferredMetadataLanguage":"en"}' >/dev/null
  api GET /Startup/User >/dev/null   # wizard needs the default user row created first
  api POST /Startup/User "$(jq -nc --arg p "$pw" '{Name:"admin",Password:$p}')" >/dev/null
  api POST /Startup/Complete >/dev/null
  local tok; tok=$(api POST /Users/AuthenticateByName "$(jq -nc --arg p "$pw" '{Username:"admin",Pw:$p}')" | jq -r .AccessToken)
  api POST "/Auth/Keys?app=nocturne-test" "" "$tok" >/dev/null
  local key; key=$(api GET /Auth/Keys "" "$tok" | jq -r '.Items[] | select(.AppName=="nocturne-test") | .AccessToken' | head -1)
  [[ -n $key ]] || { echo "could not create API key" >&2; exit 1; }
  local u
  for u in alice bob; do
    api POST /Users/New "$(jq -nc --arg n "$u" --arg p "$(rnd)" '{Name:$n,Password:$p}')" "$tok" >/dev/null
  done
  api POST "/Library/VirtualFolders?name=Test%20Movies&collectionType=movies&refreshLibrary=true" \
    "$(jq -nc --arg p "$T/media/Movies" '{LibraryOptions:{PathInfos:[{Path:$p}]}}')" "$tok" >/dev/null
  ( umask 077; printf 'NOCTURNE_JF_TEST_URL=%s\nNOCTURNE_JF_TEST_TOKEN=%s\n' "$URL" "$key" >"$ENVF" )
}

# Every start (idempotent): Quick Connect on, and alice/bob get KNOWN passwords kept only in test.env (mode 600).
ensure() {
  local key; key=$(sed -n 's/^NOCTURNE_JF_TEST_TOKEN=//p' "$ENVF")
  local cfg; cfg=$(api GET /System/Configuration "" "$key")
  [[ $(jq -r .QuickConnectAvailable <<<"$cfg") == true ]] ||
    api POST /System/Configuration "$(jq -c '.QuickConnectAvailable=true' <<<"$cfg")" "$key" >/dev/null
  local u id pw
  for u in alice bob; do
    grep -q "^NOCTURNE_JF_TEST_${u^^}_PW=" "$ENVF" && continue
    id=$(api GET /Users "" "$key" | jq -r --arg n "$u" '.[] | select(.Name==$n) | .Id')
    [[ -n $id ]] || { echo "seeded user $u missing" >&2; exit 1; }
    pw=$(rnd)
    api POST "/Users/$id/Password" "$(jq -nc --arg p "$pw" '{NewPw:$p}')" "$key" >/dev/null
    ( umask 077; printf 'NOCTURNE_JF_TEST_%s_PW=%s\n' "${u^^}" "$pw" >>"$ENVF" )
  done
}

stop() {
  if running; then
    local p; p=$(cat "$PIDF"); kill "$p" 2>/dev/null || true
    for _ in $(seq 20); do kill -0 "$p" 2>/dev/null || break; sleep 0.5; done
    kill -9 "$p" 2>/dev/null || true
  fi
  rm -f "$PIDF"
}

case "${1:-}" in
  start)
    if running && healthy; then echo "already running: $URL"; else
      stop; media; launch
      [[ -f $ENVF ]] || seed
    fi
    ensure
    echo "up: $URL"; echo "export NOCTURNE_JF_TEST_ENV=$ENVF" ;;
  stop) stop; echo stopped ;;
  reset) stop; rm -rf "${T:?}"; echo "removed $T" ;;
  status)
    if running && healthy; then echo "running: $URL (pid $(cat "$PIDF"))"; else echo "stopped"; exit 1; fi ;;
  *) echo "usage: $0 {start|stop|reset|status}" >&2; exit 2 ;;
esac
