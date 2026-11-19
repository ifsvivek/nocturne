#!/usr/bin/env bash
# Theme conformance harness: renders each Jellyfin web theme in the ORIGINAL jellyfin-web (headless Chrome) and in NOCTURNE
# (Server UI on, headless sway session) from the same throwaway server and demo library, then scores the pairs.
# usage: scripts/theme-conformance.sh [--bin PATH-TO-NOCTURNE-DEBUG-BINARY] [--hidden] [--default] [--no-report] [theme-name ...]
#   --default adds a run with no theme (the server's own look, name "default"); --no-report leaves docs/dev/THEME_CONFORMANCE.md alone.
#   NOCTURNE_CONF_DIR, NOCTURNE_CONF_OUT, NOCTURNE_CONF_HS, NOCTURNE_CONF_PORT and NOCTURNE_CONF_CHROME_PORT move the throwaway server, the captures, the headless session and the ports (several runs side by side).
#   no names = every active theme of awesome-jellyfin THEMES.md (fetched at run time); --hidden adds the "hidden pending v12" ones.
# Everything lives under /tmp (server data $CONF, captures $OUT). No theme CSS is stored anywhere: only the @import line.
# Safety: the server is a throwaway on loopback, data under /tmp; branding is written only after the guard below passes.
# See docs/dev/THEME_CONFORMANCE.md for the method, scoring formula and the baseline.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(dirname "$HERE")"
CONF="${NOCTURNE_CONF_DIR:-/tmp/nocturne-conf}"
OUT="${NOCTURNE_CONF_OUT:-/tmp/theme-conformance}"
PORT="${NOCTURNE_CONF_PORT:-8171}"
CHROME_PORT="${NOCTURNE_CONF_CHROME_PORT:-9271}"
URL="http://127.0.0.1:$PORT"
DEMO="$CONF/demo"; JF="$DEMO/jf"
HS="${NOCTURNE_CONF_HS:-conf}"        # headless session name
BIN="$REPO/target/debug/nocturne"; HIDDEN=0; NAMES=(); DEFAULT=0; REPORT=1
while [[ $# -gt 0 ]]; do case "$1" in
  --bin) BIN="$2"; shift 2;; --hidden) HIDDEN=1; shift;; --default) DEFAULT=1; shift;; --no-report) REPORT=0; shift;; -*) echo "unknown flag $1" >&2; exit 2;; *) NAMES+=("$1"); shift;; esac; done
BIN=$(realpath "$BIN"); [[ -x $BIN ]] || { echo "no NOCTURNE binary at $BIN (cargo build -p nocturne-app)" >&2; exit 1; }
[[ $PORT != 8097 ]] || { echo "refusing: port 8097 is the live-test server" >&2; exit 1; }
for b in jellyfin jq curl ffmpeg python3 google-chrome-stable sway grim secret-tool; do command -v "$b" >/dev/null || { echo "missing: $b" >&2; exit 1; }; done
WEBDIR=/usr/share/jellyfin/web; [[ -f $WEBDIR/index.html ]] || { echo "missing jellyfin-web ($WEBDIR)" >&2; exit 1; }
START=$(date +%s); COMMIT=$(cd "$REPO" && git rev-parse --short HEAD)
mkdir -p "$OUT" "$CONF"
PIDS_JF="$CONF/jellyfin.pid"; PIDS_CHROME="$CONF/chrome.pid"
cleanup() {  # only PIDs this script started
  for f in "$PIDS_CHROME" "$PIDS_JF"; do [[ -f $f ]] && { kill "$(cat "$f")" 2>/dev/null || true; rm -f "$f"; }; done
  HS_NAME=$HS "$HERE/headless-session.sh" stop >/dev/null 2>&1 || true
  [[ -n ${KEYRING_USER_KEYS:-} ]] && while read -r k; do secret-tool clear service nocturne-test username "$k" 2>/dev/null || true; done <<<"$KEYRING_USER_KEYS"
}
trap cleanup EXIT

# ---- 1. throwaway server with the demo library (built once), relaunched WITH the web client
if [[ ! -f $DEMO/.built ]]; then
  NOCTURNE_DEMO_DIR=$DEMO NOCTURNE_JF_TEST_PORT=$PORT "$HERE/make-demo-library.sh" build
  NOCTURNE_JF_TEST_DIR=$JF NOCTURNE_JF_TEST_PORT=$PORT "$HERE/test-jellyfin.sh" stop
  touch "$DEMO/.built"
fi
KEY=$(sed -n 's/^NOCTURNE_JF_TEST_TOKEN=//p' "$JF/test.env")
healthy() { curl -fs -m 2 "$URL/System/Info/Public" >/dev/null 2>&1; }
if ! healthy; then
  # Same flags as test-jellyfin.sh, minus --nowebclient. The first answer is followed by a listener rebuild: need two hits.
  setsid nohup jellyfin --webdir "$WEBDIR" --nonetchange --service --datadir "$JF/data" --configdir "$JF/config" \
    --cachedir "$JF/cache" --logdir "$JF/log" --ffmpeg "$(command -v ffmpeg)" >"$JF/stdout.log" 2>&1 </dev/null &
  echo $! >"$PIDS_JF"
  for _ in $(seq 90); do healthy && { sleep 4; healthy && break; }; sleep 1; done
  healthy || { echo "jellyfin did not come up, see $JF/stdout.log" >&2; exit 1; }
fi
AUTHH="MediaBrowser Client=\"nocturne-conf\", Device=\"script\", DeviceId=\"nocturne-conf-script\", Version=\"1\", Token=\"$KEY\""
api() { curl -fsS -m 60 -X "$1" -H "Authorization: $AUTHH" -H 'Content-Type: application/json' ${3:+-d "$3"} "$URL$2"; }

# Guard (same rules as TestServerGuard): loopback, not 8097, and the server's own data path is under the throwaway dir in /tmp.
guard() {
  local dp; dp=$(api GET /System/Info | jq -r .ProgramDataPath)
  case "$dp" in "$JF"/*) ;; *) echo "refusing branding write: server data path '$dp' is not under $JF" >&2; exit 1;; esac
  case "$JF" in /tmp/?*) ;; *) echo "refusing: $JF not under /tmp" >&2; exit 1;; esac
}
guard
ID_MOVIES=$(api GET /Library/VirtualFolders | jq -r '.[]|select(.Name=="Movies")|.ItemId')
ids() { api GET "/Items?Recursive=true&IncludeItemTypes=$1&SearchTerm=$(jq -rn --arg s "$2" '$s|@uri')&Limit=5" | jq -r --arg n "$2" '[.Items[]|select(.Name==$n)][0].Id'; }
MOVIE_NAME="${CONF_MOVIE:-Ember Tide}"; SERIES_NAME="${CONF_SERIES:-The Ashen Coast}"
jq -n --arg l "$ID_MOVIES" --arg m "$(ids Movie "$MOVIE_NAME")" --arg s "$(ids Series "$SERIES_NAME")" \
  '{movies_lib:$l, movie:$m, series:$s}' >"$CONF/ids.json"

# ---- 2. theme list
python3 -I "$HERE/theme-conformance.py" themes "$OUT/themes.json"
mapfile -t LIST < <(jq -r --argjson h "$HIDDEN" '.[] | select(.import_url != null) | select($h==1 or .status=="active") | .name' "$OUT/themes.json" )
if ((${#NAMES[@]})); then LIST=("${NAMES[@]}"); fi
if ((DEFAULT)); then LIST=(default "${LIST[@]}"); fi

# ---- 3. app side: isolated config signed in as `demo` with the server's UI on (keyring service nocturne-test only)
PW=$(cat "$JF/demo-password")
SESS=$(curl -fsS -X POST -H "Authorization: MediaBrowser Client=\"nocturne-conf\", Device=\"script\", DeviceId=\"nocturne-conf-app\", Version=\"1\"" \
  -H 'Content-Type: application/json' -d "$(jq -nc --arg p "$PW" '{Username:"demo",Pw:$p}')" "$URL/Users/AuthenticateByName")
UID_=$(jq -r .User.Id <<<"$SESS")
RUN=/tmp/nocturne-run/$HS; rm -rf "$RUN"; mkdir -p "$RUN/config/nocturne"
# version 2: "All" (and the server alone) takes its UI from source 1, so the capture shows the server's sections.
cat >"$RUN/config/nocturne/sources.toml" <<TOML
version = 2
next_id = 2

[ui_from]
all = "1"
1 = "self"

[[profile]]
id = 1
label = "conf"
enabled = true
kind = "jellyfin"
url = "$URL"
user_id = "$UID_"
user_name = "demo"
TOML
printf 'onboarding = 99\n' >"$RUN/config/nocturne/settings.toml"   # walkthrough done
printf 'servers = ["%s"]\nlast_used = "%s"\n' "$URL" "$URL" >"$RUN/config/nocturne/servers.toml"
KEYRING_USER_KEYS=$(printf '%s\n%s' "$URL" "$URL|$UID_")
sess_json=$(jq -c '{access_token:.AccessToken,user_id:.User.Id,server_id:.ServerId,user_name:.User.Name,is_admin:.User.Policy.IsAdministrator}' <<<"$SESS")
while read -r k; do
  printf '%s' "$sess_json" | secret-tool store --label "nocturne-conf throwaway" service nocturne-test username "$k" application rust-keyring target default
done <<<"$KEYRING_USER_KEYS"

HS_BORDERLESS=1 HS_NAME=$HS "$HERE/headless-session.sh" start 1920x1080
app_shot() { # screen keys out.png [report.json]
  local keys=$2 out=$3 rep=${4:-}
  rm -f "$out"
  [[ -n $rep ]] && { rm -f "$rep"; keys="${keys:+$keys,}theme-report@11000"; }
  HS_NAME=$HS timeout 60 "$HERE/headless-session.sh" run env NOCTURNE_KEYS="$keys" NOCTURNE_THEME_REPORT="${rep:-/dev/null}" NOCTURNE_SNAP="$out" NOCTURNE_SNAP_AFTER_MS="${SNAP_MS:-12500}" \
    "$HERE/run-isolated.sh" --keep "$HS" "$BIN" >"$OUT/app.log" 2>&1 &
  local pid=$!
  for _ in $(seq 50); do [[ -s $out ]] && break; sleep 1; done
  sleep 1; kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true
}
# Keys per screen (see the debug hooks in crates/app/src/snap.rs). Opening an item has no hook yet: first card via `enter`.
declare -A KEYS=(
  [home]=""
  [movies]="open-movies@4000"
  [movie]="open-movies@4000,enter@8000"
  [series]="open-shows@4000,enter@8000"
)

# ---- 4. per theme
declare -a SUMMARY
for name in "${LIST[@]}"; do
  d="$OUT/$(tr -c 'A-Za-z0-9\n' _ <<<"$name")"; mkdir -p "$d"
  if [[ $name == default ]]; then imp=""; css=""; else
    imp=$(jq -r --arg n "$name" '.[]|select(.name==$n)|.import_url // empty' "$OUT/themes.json")
    [[ -n $imp ]] || { echo "== $name: no resolvable @import, skipped"; continue; }
    css="@import url(\"$imp\");"
  fi
  echo "== $name  ($imp)"
  guard
  cfg=$(api GET /System/Configuration/branding)
  api POST /System/Configuration/branding "$(jq -c --arg c "$css" '.CustomCss=$c' <<<"$cfg")" >/dev/null
  python3 -I "$HERE/theme-conformance.py" web --url "$URL" --user demo --pw-file "$JF/demo-password" --out "$d" \
    --chrome "$(command -v google-chrome-stable)" --ids "$CONF/ids.json" --profile "$CONF/chrome-profile" --pidfile "$PIDS_CHROME" --chrome-port "$CHROME_PORT"
  rm -rf "$CONF/chrome-profile"; rm -f "$PIDS_CHROME"
  for scr in home movies movie series; do app_shot "$scr" "${KEYS[$scr]}" "$d/app-$scr.png" "$([[ $scr == home ]] && echo "$d/report.json")"; done
  python3 -I "$HERE/theme-conformance.py" score "$d" >"$d/scores.line"
  python3 -I "$HERE/theme-conformance.py" sheet "$d"
  [[ -s $d/report.json ]] || echo '{}' >"$d/report.json"
  jq -n --arg n "$name" --arg u "$imp" --slurpfile s "$d/scores.json" --slurpfile r "$d/report.json" '{name:$n,import:$u,scores:$s[0],report:$r[0]}' >"$d/result.json"
done
# reset the throwaway server's branding
guard; cfg=$(api GET /System/Configuration/branding); api POST /System/Configuration/branding "$(jq -c '.CustomCss=""' <<<"$cfg")" >/dev/null
DOC="$REPO/docs/dev/THEME_CONFORMANCE.md"
((REPORT)) && [[ -f $DOC ]] && python3 -I "$HERE/theme-conformance.py" report --out "$OUT" --doc "$DOC" --commit "$COMMIT" --jf "$(api GET /System/Info | jq -r .Version)"
python3 -I "$HERE/theme-conformance.py" all "$OUT" || true
echo "done in $(( $(date +%s) - START )) s; captures under $OUT"
