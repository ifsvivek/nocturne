#!/usr/bin/env bash
# Run a NOCTURNE binary for testing, isolated from the developer's real setup:
# a throwaway working directory OUTSIDE the repo (debug builds read `.env` from the cwd or any ancestor),
# blank server variables (process env beats `.env`), throwaway XDG dirs and a test keyring service.
#   scripts/run-isolated.sh [--keep NAME] BINARY [ARGS...]     extra env (e.g. NOCTURNE_JF_URL=...) passes through.
# --keep NAME reuses /tmp/nocturne-run/NAME (sources survive restarts); otherwise a fresh dir each run.
set -euo pipefail
base=/tmp/nocturne-run
if [ "${1:-}" = --keep ]; then dir="$base/$2"; shift 2; else dir=$(mktemp -d "$base.XXXXXX"); fi
bin=$(realpath "$1"); shift
mkdir -p "$dir"/{work,config,data,cache}
cd "$dir/work"
export XDG_CONFIG_HOME="$dir/config" XDG_DATA_HOME="$dir/data" XDG_CACHE_HOME="$dir/cache"
export NOCTURNE_KEYRING_SERVICE="${NOCTURNE_KEYRING_SERVICE:-nocturne-test}"
export NOCTURNE_JF_URL="${NOCTURNE_JF_URL:-}" NOCTURNE_JF_TOKEN="${NOCTURNE_JF_TOKEN:-}"
exec "$bin" "$@"
