#!/usr/bin/env bash
# Throwaway Seerr for NOCTURNE's request tests, attached to the throwaway Jellyfin of scripts/test-jellyfin.sh.
# NEVER touches a real Seerr: binds 127.0.0.1 only, port 5057 (never 5055), data under /tmp, container named nocturne-seerr-test.
# Usage: scripts/test-seerr.sh {start|stop|reset|status}
# Needs docker, curl, jq, and the throwaway Jellyfin running (scripts/test-jellyfin.sh start).
# Environment: NOCTURNE_JF_TEST_ENV (the Jellyfin's test.env, default /tmp/nocturne-jf-test/test.env),
#   NOCTURNE_SEERR_TEST_PORT (default 5057), NOCTURNE_SEERR_TEST_DIR (default /tmp/nocturne-seerr-test), SEERR_IMAGE.
# Credentials (the Jellyfin admin's, alice's) are copied from the Jellyfin's files into $T/test.env (mode 600); nothing is printed.
set -euo pipefail

T="${NOCTURNE_SEERR_TEST_DIR:-/tmp/nocturne-seerr-test}"
PORT="${NOCTURNE_SEERR_TEST_PORT:-5057}"
IMAGE="${SEERR_IMAGE:-ghcr.io/seerr-team/seerr:v3.5.2}"
NAME=nocturne-seerr-test
URL="http://127.0.0.1:$PORT"
ENVF="$T/test.env"
JF_ENV="${NOCTURNE_JF_TEST_ENV:-/tmp/nocturne-jf-test/test.env}"

case "$T" in /tmp/?*) ;; *) echo "refusing: test dir must be under /tmp (got $T)" >&2; exit 1;; esac
case "$T" in *..*) echo "refusing: test dir must not contain '..' (got $T)" >&2; exit 1;; esac
case "$(realpath -m "$T")" in /tmp/?*) ;; *) echo "refusing: test dir resolves outside /tmp (got $T)" >&2; exit 1;; esac
[[ $PORT == 5055 ]] && { echo "refusing: 5055 is Seerr's own port, a real server may be there" >&2; exit 1; }
for b in docker curl jq; do command -v "$b" >/dev/null || { echo "missing: $b" >&2; exit 1; }; done

running() { [[ $(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null) == true ]]; }
healthy() { curl -fs -m 2 "$URL/api/v1/status" >/dev/null 2>&1; }
jf_get() { sed -n "s/^$1=//p" "$JF_ENV" | head -1; }
JAR="$T/admin.jar"
api() { # method path [json-body]  (as the Seerr admin; cookie jar kept in $T)
  curl -fsS -m 60 -X "$1" -b "$JAR" -c "$JAR" -H 'Content-Type: application/json' ${3:+-d "$3"} "$URL/api/v1$2"
}

seed() {
  local jf_url jf_host jf_port apw
  jf_url=$(jf_get NOCTURNE_JF_TEST_URL)
  [[ -n $jf_url ]] || { echo "no Jellyfin test env at $JF_ENV (run scripts/test-jellyfin.sh start)" >&2; exit 1; }
  jf_host=$(sed -E 's#^https?://([^:/]+).*#\1#' <<<"$jf_url"); jf_port=$(sed -E 's#^.*:([0-9]+)/?$#\1#' <<<"$jf_url")
  [[ $jf_host == 127.0.0.1 ]] || { echo "refusing: Jellyfin must be on 127.0.0.1 (got $jf_host)" >&2; exit 1; }
  apw=$(cat "$(dirname "$JF_ENV")/admin-password")
  : >"$JAR"; chmod 600 "$JAR"
  # First sign-in on an uninitialised Seerr sets up its media server and makes this user the administrator.
  api POST /auth/jellyfin "$(jq -nc --arg p "$apw" --arg h "$jf_host" --argjson port "$jf_port" \
    '{username:"admin",password:$p,hostname:$h,port:$port,useSsl:false,urlBase:"",email:"admin@example.invalid",serverType:2}')" >/dev/null
  # Libraries: sync, enable all (Seerr's scan needs them for the Available state).
  local id
  for id in $(api POST /settings/jellyfin/library/sync | jq -r '.[].id'); do
    api PUT "/settings/jellyfin/library/$id" '{"enabled":true}' >/dev/null
  done
  api POST /settings/initialize >/dev/null
  # Alice signs in once so her Seerr user exists (default permission: request).
  local alice_pw; alice_pw=$(jf_get NOCTURNE_JF_TEST_ALICE_PW)
  curl -fsS -m 60 -X POST -H 'Content-Type: application/json' \
    -d "$(jq -nc --arg p "$alice_pw" '{username:"alice",password:$p}')" "$URL/api/v1/auth/jellyfin" >/dev/null
  ( umask 077
    printf 'NOCTURNE_SEERR_TEST_URL=%s\nNOCTURNE_SEERR_TEST_DATA=%s\nNOCTURNE_SEERR_TEST_ADMIN_USER=admin\nNOCTURNE_SEERR_TEST_ADMIN_PW=%s\nNOCTURNE_SEERR_TEST_ALICE_PW=%s\n' \
      "$URL" "$T/config" "$apw" "$alice_pw" >"$ENVF" )
}

launch() {
  mkdir -p "$T/config"
  # The image runs as uid 1000; make the bind mount writable whoever owns it.
  [[ $(stat -c %u "$T/config") == 1000 ]] || chmod 777 "$T/config"
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run -d --name "$NAME" --network host -e HOST=127.0.0.1 -e PORT="$PORT" -e LOG_LEVEL=info \
    -v "$T/config:/app/config" "$IMAGE" >/dev/null
  local n
  for n in $(seq 90); do healthy && return 0; running || break; sleep 1; done
  echo "seerr did not become healthy; see: docker logs $NAME" >&2; exit 1
}

stop() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }

case "${1:-}" in
  start)
    if running && healthy && [[ -f $ENVF ]]; then echo "already running: $URL"; else
      stop; launch
      [[ -f $ENVF ]] || seed
    fi
    echo "up: $URL"; echo "export NOCTURNE_SEERR_TEST_ENV=$ENVF" ;;
  stop) stop; echo stopped ;;
  reset)
    stop
    # The bind mount is owned by the container's user: remove it from inside a throwaway container if plain rm fails.
    rm -rf "${T:?}" 2>/dev/null || {
      docker run --rm -u 0 -v "$T:/d" --entrypoint /bin/sh "$IMAGE" -c 'rm -rf /d/* /d/.[!.]*' >/dev/null 2>&1 || true
      rm -rf "${T:?}"
    }
    echo "removed $T" ;;
  status)
    if running && healthy; then echo "running: $URL"; else echo "stopped"; exit 1; fi ;;
  *) echo "usage: $0 {start|stop|reset|status}" >&2; exit 2 ;;
esac
