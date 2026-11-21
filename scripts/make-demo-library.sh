#!/usr/bin/env bash
# Builds a fully synthetic DEMO library on a THROWAWAY Jellyfin (loopback :8097 or $NOCTURNE_JF_TEST_PORT, everything under /tmp)
# for README/website screenshots. No real titles, people or artwork: see scripts/demo_assets.py.
# Usage: scripts/make-demo-library.sh {build|stop|clean} [--upcoming]
#   --upcoming  (build) adds the show "Night Shift Radio" whose episodes air in the next weeks (dates relative to today), for the Calendar.
#   build  start the throwaway server, generate media, add libraries, scan, create user `demo`,
#          favourite/collection/resume/next-up state. Prints connection info (no secrets).
#   stop   stop the server.   clean  stop and delete $DEMO entirely.
# The demo password is written to $DEMO/jf/demo-password (mode 600). Nothing from $DEMO is ever committed.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEMO="${NOCTURNE_DEMO_DIR:-/tmp/nocturne-demo}"
case "$DEMO" in /tmp/?*) ;; *) echo "refusing: demo dir must be under /tmp" >&2; exit 1;; esac
case "$DEMO" in *..*) echo "refusing: '..' in demo dir" >&2; exit 1;; esac
export NOCTURNE_JF_TEST_DIR="$DEMO/jf"
JF="$NOCTURNE_JF_TEST_DIR"
LIB="$DEMO/lib"
URL="http://127.0.0.1:${NOCTURNE_JF_TEST_PORT:-8097}"
cmd="${1:-build}"

case "$cmd" in
  stop) "$HERE/test-jellyfin.sh" stop; exit 0 ;;
  clean) "$HERE/test-jellyfin.sh" stop || true; rm -rf "${DEMO:?}"; echo "removed $DEMO"; exit 0 ;;
  build) ;;
  *) echo "usage: $0 {build|stop|clean}" >&2; exit 2 ;;
esac
for b in jq curl python3 ffmpeg base64; do command -v "$b" >/dev/null || { echo "missing: $b" >&2; exit 1; }; done

mkdir -p "$LIB"
"$HERE/test-jellyfin.sh" start >/dev/null
KEY=$(sed -n 's/^NOCTURNE_JF_TEST_TOKEN=//p' "$JF/test.env")
AUTH="MediaBrowser Client=\"nocturne-demo\", Device=\"script\", DeviceId=\"nocturne-demo-script\", Version=\"1\", Token=\"$KEY\""
api() { curl -fsS -m 60 -X "$1" -H "Authorization: $AUTH" -H 'Content-Type: application/json' ${3:+-d "$3"} "$URL$2"; }
img() { base64 -w0 "$3" | curl -fsS -m 60 -X POST -H "Authorization: $AUTH" -H "Content-Type: image/$(case "$3" in *.png) echo png;; *) echo jpeg;; esac)" --data-binary @- "$URL/Items/$2/Images/$1" >/dev/null; }
count() { api GET "/Items?Recursive=true&IncludeItemTypes=$1&CollapseBoxSetItems=false&Limit=0" | jq .TotalRecordCount; }
find_id() { api GET "/Items?Recursive=true&IncludeItemTypes=$1&SearchTerm=$(jq -rn --arg s "$2" '$s|@uri')&Limit=5" | jq -r --arg n "$2" '[.Items[]|select(.Name==$n)][0].Id'; }

echo "generating media"
python3 -I "$HERE/demo_assets.py" "$LIB" ${2:+"$2"} >"$DEMO/counts.json"

# Server: keep short clips resumable.
api GET /System/Configuration | jq -c '.ServerName="NOCTURNE Demo"|.MinResumePct=0|.MaxResumePct=90|.MinResumeDurationSeconds=0|.QuickConnectAvailable=true' |
  api POST /System/Configuration "$(cat)" >/dev/null

# Replace the generic test library with the demo libraries. Every metadata/image fetcher is disabled, so
# nothing is looked up online and nothing can match a real title; only the NFO files and local images count.
api DELETE "/Library/VirtualFolders?name=Test%20Movies&refreshLibrary=false" >/dev/null 2>&1 || true
opts() { # path, then Jellyfin item types
  local p=$1; shift
  jq -nc --arg p "$p" '$ARGS.positional as $t | {LibraryOptions:{
    PathInfos:[{Path:$p}], EnableRealtimeMonitor:false, EnableInternetProviders:false, AutomaticRefreshIntervalDays:0,
    SaveLocalMetadata:false, LocalMetadataReaderOrder:["Nfo"], MetadataSavers:[], PreferredMetadataLanguage:"en",
    EnableChapterImageExtraction:false, ExtractChapterImagesDuringLibraryScan:false, EnableTrickplayImageExtraction:false,
    TypeOptions:[$t[]|{Type:., MetadataFetchers:[], MetadataFetcherOrder:[], ImageFetchers:[], ImageFetcherOrder:[]}]}}' --args "$@"
}
add_lib() { # name collectionType path types...
  local n=$1 t=$2 p=$3; shift 3
  api GET /Library/VirtualFolders | jq -e --arg n "$n" 'any(.[]; .Name==$n)' >/dev/null && return 0
  api POST "/Library/VirtualFolders?name=$n&collectionType=$t&refreshLibrary=true" "$(opts "$p" "$@")" >/dev/null
}
add_lib Movies movies "$LIB/media/Movies" Movie
add_lib Shows tvshows "$LIB/media/Shows" Series Season Episode
add_lib Music music "$LIB/media/Music" MusicArtist MusicAlbum Audio
add_lib Photos homevideos "$LIB/media/Photos" Video Photo

# Libraries added while a scan runs can be skipped by it, so ask for one more full refresh.
api POST /Library/Refresh >/dev/null
echo "waiting for scan"
want=$(cat "$DEMO/counts.json")
for _ in $(seq 180); do
  m=$(count Movie); e=$(count Episode); s=$(count Series); a=$(count Audio); p=$(count Photo)
  nov=$(api GET "/Items?Recursive=true&IncludeItemTypes=Movie,Series,Episode&CollapseBoxSetItems=false&Fields=Overview&Limit=500" | jq '[.Items[]|select(.Type!="BoxSet" and (.Overview//"")=="")]|length')
  [[ $m -ge $(jq .movies <<<"$want") && $e -ge $(jq .episodes <<<"$want") && $s -ge $(jq .series <<<"$want") \
     && $a -ge $(jq .tracks <<<"$want") && $p -ge $(jq .photos <<<"$want") && $nov -eq 0 ]] && break
  sleep 2
done
echo "movies=$m episodes=$e series=$s audio=$a photos=$p without_overview=$nov"
[[ $nov -eq 0 ]] || { echo "metadata incomplete" >&2; exit 1; }
# Refuse to continue if anything looks like it came from an online provider.
if grep -qiE 'api\.themoviedb|image\.tmdb|musicbrainz\.org|theaudiodb\.com|omdbapi|thetvdb' "$JF"/log/*.log 2>/dev/null; then echo "online provider seen in log" >&2; exit 1; fi

# User `demo` (administrator, so Admin screens show), password only in $JF/demo-password.
if [[ ! -f $JF/demo-password ]]; then
  ( umask 077; head -c 18 /dev/urandom | base64 | tr -d '/+=' >"$JF/demo-password" )
fi
PW=$(cat "$JF/demo-password")
UID_=$(api GET /Users | jq -r '.[]|select(.Name=="demo")|.Id')
if [[ -z $UID_ ]]; then
  UID_=$(api POST /Users/New "$(jq -nc --arg p "$PW" '{Name:"demo",Password:$p}')" | jq -r .Id)
fi
api GET "/Users/$UID_" | jq -c '.Policy|.IsAdministrator=true' | api POST "/Users/$UID_/Policy" "$(cat)" >/dev/null

# State: favourite, resume points, one fully watched season so Next Up has entries, collection, images.
userdata() { api POST "/UserItems/$1/UserData?userId=$UID_" "$2" >/dev/null; }
ago() { date -u -d "-$1 hours" +%Y-%m-%dT%H:%M:%SZ; }
dur() { api GET "/Items/$1?userId=$UID_" | jq .RunTimeTicks; }
for n in "Quiet Orbit" "The Lanternkeeper" "Low Tide Choir"; do userdata "$(find_id Movie "$n")" '{"IsFavorite":true}'; done
h=2
for n in "Hollow Meridian" "Ember Tide"; do
  id=$(find_id Movie "$n"); userdata "$id" "{\"PlaybackPositionTicks\":$(( $(dur "$id") * 4 / 10 )),\"LastPlayedDate\":\"$(ago $h)\"}"; h=$((h + 5))
done
for n in "Saltglass" "Paper Cathedrals" "The Fox of Vellum Hill"; do
  userdata "$(find_id Movie "$n")" "{\"Played\":true,\"PlayCount\":1,\"LastPlayedDate\":\"$(ago $((h += 9)))\"}"
done
# First two episodes of each series watched (so Next Up shows the third); the third of The Ashen Coast is part-way.
ASH=$(find_id Series "The Ashen Coast"); SIG=$(find_id Series "Signal Garden")
for sid in $ASH $SIG; do
  eps=$(api GET "/Shows/$sid/Episodes?userId=$UID_" | jq -r '.Items[0:3][].Id')
  i=0
  for id in $eps; do
    if [[ $i -lt 2 ]]; then userdata "$id" "{\"Played\":true,\"PlayCount\":1,\"LastPlayedDate\":\"$(ago $((30 - i * 3)))\"}"
    elif [[ $sid == "$ASH" ]]; then userdata "$id" "{\"PlaybackPositionTicks\":$(( $(dur "$id") * 3 / 10 )),\"LastPlayedDate\":\"$(ago 1)\"}"; fi
    i=$((i + 1))
  done
done
if [[ $(count BoxSet) -eq 0 ]]; then
  ids=$(for n in "Hollow Meridian" "Quiet Orbit" "Static Bloom"; do find_id Movie "$n"; done | paste -sd,)
  cid=$(api POST "/Collections?name=Cold%20Frequencies&ids=$ids" | jq -r .Id)
  img Primary "$cid" "$LIB/collection.jpg"
  img Backdrop "$cid" "$LIB/collection-backdrop.jpg"
  img Logo "$cid" "$LIB/collection-logo.png"
  api GET "/Items?ids=$cid&Fields=Overview,Genres,Tags,Studios,People,ProviderIds,LockedFields,Taglines" |
    jq -c '.Items[0]|.Overview="Three quiet, signal-haunted films from Northfold Pictures."|.LockData=false|.Genres=(.Genres//[])|.Tags=(.Tags//[])|.People=(.People//[])|.Studios=(.Studios//[])|.Taglines=(.Taglines//[])|.ProviderIds=(.ProviderIds//{})|.LockedFields=(.LockedFields//[])' |
    api POST "/Items/$cid" "$(cat)" >/dev/null
fi
for f in "$LIB"/people/*.jpg; do
  n=$(basename "$f" .jpg); pid=$(api GET "/Persons?searchTerm=$(jq -rn --arg s "$n" '$s|@uri')&Limit=3" | jq -r --arg n "$n" '[.Items[]|select(.Name==$n)][0].Id // empty')
  [[ -n $pid ]] && img Primary "$pid" "$f"
done
echo "ready: $URL  user=demo  (password in $JF/demo-password)"
