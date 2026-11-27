#!/usr/bin/env bash
# Throwaway Radarr and Sonarr for NOCTURNE's request tests.
# NEVER touches a real service: binds 127.0.0.1 only, ports 7879 / 8990 (never 7878 / 8989), data under /tmp,
# containers named nocturne-radarr-test and nocturne-sonarr-test.
# Usage: scripts/test-arr.sh {start|stop|reset|status}
# Needs docker, curl and openssl. Adding a title makes the service look it up on the internet (its own metadata servers).
# Environment: NOCTURNE_ARR_TEST_DIR (default /tmp/nocturne-arr-test), NOCTURNE_RADARR_TEST_PORT, NOCTURNE_SONARR_TEST_PORT,
#   RADARR_IMAGE, SONARR_IMAGE.
# The API keys are random, written to $T/test.env (mode 600) and into each service's config.xml before its first start; nothing is printed.
set -euo pipefail

T="${NOCTURNE_ARR_TEST_DIR:-/tmp/nocturne-arr-test}"
RPORT="${NOCTURNE_RADARR_TEST_PORT:-7879}"
SPORT="${NOCTURNE_SONARR_TEST_PORT:-8990}"
RIMAGE="${RADARR_IMAGE:-lscr.io/linuxserver/radarr:latest}"
SIMAGE="${SONARR_IMAGE:-lscr.io/linuxserver/sonarr:latest}"
RNAME=nocturne-radarr-test
SNAME=nocturne-sonarr-test
ENVF="$T/test.env"

case "$T" in /tmp/?*) ;; *) echo "refusing: test dir must be under /tmp (got $T)" >&2; exit 1;; esac
case "$T" in *..*) echo "refusing: test dir must not contain '..' (got $T)" >&2; exit 1;; esac
case "$(realpath -m "$T")" in /tmp/?*) ;; *) echo "refusing: test dir resolves outside /tmp (got $T)" >&2; exit 1;; esac
for p in "$RPORT" "$SPORT"; do
  case "$p" in 7878|8989) echo "refusing: $p is a default port, a real service may be there" >&2; exit 1;; esac
done
for b in docker curl openssl; do command -v "$b" >/dev/null || { echo "missing: $b" >&2; exit 1; }; done

running() { [[ $(docker inspect -f '{{.State.Running}}' "$1" 2>/dev/null) == true ]]; }
env_get() { sed -n "s/^$1=//p" "$ENVF" | head -1; }
api() { # name method path [json-body]  (as the service's own API key)
  local url key
  if [[ $1 == radarr ]]; then url="http://127.0.0.1:$RPORT"; key=$(env_get NOCTURNE_ARR_TEST_RADARR_KEY); else url="http://127.0.0.1:$SPORT"; key=$(env_get NOCTURNE_ARR_TEST_SONARR_KEY); fi
  curl -fsS -m 60 -X "$2" -H "X-Api-Key: $key" -H 'Content-Type: application/json' ${4:+-d "$4"} "$url/api/v3$3"
}
healthy() { api "$1" GET /system/status >/dev/null 2>&1; }

write_config() { # dir port key  (the services read ApiKey, Port and BindAddress from config.xml at startup)
  mkdir -p "$1"
  cat >"$1/config.xml" <<XML
<Config>
  <BindAddress>127.0.0.1</BindAddress>
  <Port>$2</Port>
  <ApiKey>$3</ApiKey>
  <AuthenticationMethod>None</AuthenticationMethod>
  <AuthenticationRequired>DisabledForLocalAddresses</AuthenticationRequired>
  <LaunchBrowser>False</LaunchBrowser>
  <UpdateMechanism>Docker</UpdateMechanism>
</Config>
XML
}

launch() { # name image port keyvar mediadir
  docker rm -f "$1" >/dev/null 2>&1 || true
  docker run -d --name "$1" --network host -e PUID="$(id -u)" -e PGID="$(id -g)" -e TZ=UTC \
    -v "$T/$1/config:/config" -v "$T/media/$5:/$5" "$2" >/dev/null
}

stop() { docker rm -f "$RNAME" "$SNAME" >/dev/null 2>&1 || true; }

case "${1:-}" in
  start)
    if running "$RNAME" && running "$SNAME" && [[ -f $ENVF ]] && healthy radarr && healthy sonarr; then echo "already running"; else
      stop
      mkdir -p "$T/media/movies" "$T/media/tv"
      if [[ ! -f $ENVF ]]; then
        rk=$(openssl rand -hex 16); sk=$(openssl rand -hex 16)
        ( umask 077
          printf 'NOCTURNE_ARR_TEST_RADARR_URL=http://127.0.0.1:%s\nNOCTURNE_ARR_TEST_RADARR_KEY=%s\nNOCTURNE_ARR_TEST_SONARR_URL=http://127.0.0.1:%s\nNOCTURNE_ARR_TEST_SONARR_KEY=%s\nNOCTURNE_ARR_TEST_DATA=%s\n' \
            "$RPORT" "$rk" "$SPORT" "$sk" "$T" >"$ENVF" )
        write_config "$T/$RNAME/config" "$RPORT" "$rk"
        write_config "$T/$SNAME/config" "$SPORT" "$sk"
      fi
      launch "$RNAME" "$RIMAGE" "$RPORT" key movies
      launch "$SNAME" "$SIMAGE" "$SPORT" key tv
      for n in $(seq 120); do healthy radarr && healthy sonarr && break; sleep 1; done
      healthy radarr && healthy sonarr || { echo "services did not become healthy; see: docker logs $RNAME / $SNAME" >&2; exit 1; }
      # A root folder each (the mounted, empty media directories); the default quality profiles ship with the services.
      api radarr GET /rootfolder | grep -q '"/movies' || api radarr POST /rootfolder '{"path":"/movies"}' >/dev/null
      api sonarr GET /rootfolder | grep -q '"/tv' || api sonarr POST /rootfolder '{"path":"/tv"}' >/dev/null
    fi
    echo "up: radarr 127.0.0.1:$RPORT sonarr 127.0.0.1:$SPORT"; echo "export NOCTURNE_ARR_TEST_ENV=$ENVF" ;;
  stop) stop; echo stopped ;;
  reset)
    stop
    rm -rf "${T:?}" 2>/dev/null || {
      docker run --rm -u 0 -v "$T:/d" --entrypoint /bin/sh "$RIMAGE" -c 'rm -rf /d/* /d/.[!.]*' >/dev/null 2>&1 || true
      rm -rf "${T:?}"
    }
    echo "removed $T" ;;
  status)
    if running "$RNAME" && running "$SNAME" && [[ -f $ENVF ]] && healthy radarr && healthy sonarr; then echo "running"; else echo "stopped"; exit 1; fi ;;
  *) echo "usage: $0 {start|stop|reset|status}" >&2; exit 2 ;;
esac
