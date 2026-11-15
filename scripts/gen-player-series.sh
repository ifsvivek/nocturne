#!/usr/bin/env bash
# Sibling of gen-player-clips.sh (scripts/test-jellyfin.sh must be running): adds a 3-episode test series
# (100 s each, 640x360, chapters "Intro"/"Episode"/"Credits", an eng sidecar .srt) to a new "Test Shows"
# library and turns trickplay extraction on for it and for "Test Movies", then runs the trickplay and
# media-segment tasks. THROWAWAY SERVER ONLY: everything lives under $NOCTURNE_JF_TEST_DIR (default
# /tmp/nocturne-jf-test) and the URL/token come from its test.env.
set -euo pipefail
T="${NOCTURNE_JF_TEST_DIR:-/tmp/nocturne-jf-test}"
case "$T" in /tmp/?*) ;; *) echo "refusing: test dir must be under /tmp" >&2; exit 1;; esac
case "$T" in *..*) echo "refusing: '..' in test dir" >&2; exit 1;; esac
# shellcheck disable=SC1091
set -a; . "$T/test.env"; set +a
URL="$NOCTURNE_JF_TEST_URL"
case "$URL" in http://127.0.0.1:8097*) ;; *) echo "refusing: not the throwaway server" >&2; exit 1;; esac
AUTH="MediaBrowser Client=\"nocturne-test\", Device=\"script\", DeviceId=\"nocturne-test-script\", Version=\"1\", Token=\"$NOCTURNE_JF_TEST_TOKEN\""
api() { curl -fsS -m 30 -X "$1" -H "Authorization: $AUTH" -H 'Content-Type: application/json' ${3:+-d "$3"} "$URL$2"; }
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
S="$T/media/Shows/Player Series (2011)/Season 01"
mkdir -p "$S"
cat > "$W/chapters.txt" <<'EOM'
;FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=20000
title=Intro
[CHAPTER]
TIMEBASE=1/1000
START=20000
END=80000
title=Episode
[CHAPTER]
TIMEBASE=1/1000
START=80000
END=100000
title=Credits
EOM
for i in 1 2 3; do
  f="$S/Player Series S01E0$i.mkv"
  [[ -f $f ]] && continue
  printf '1\n00:00:02,000 --> 00:00:04,000\nEpisode %d subtitle\n\n' "$i" > "$W/e$i.srt"
  ffmpeg -v error -y -f lavfi -i "testsrc2=size=640x360:rate=30:duration=100" -f lavfi -i "sine=frequency=$((300 + 100 * i)):duration=100" \
    -i "$W/chapters.txt" -map 0:v -map 1:a -map_metadata 2 -c:v libx264 -pix_fmt yuv420p -g 30 -c:a aac -metadata:s:a:0 language=eng "$f"
  cp "$W/e$i.srt" "${f%.mkv}.en.srt"
done
if ! api GET /Library/VirtualFolders | grep -q '"Test Shows"'; then
  api POST "/Library/VirtualFolders?name=Test%20Shows&collectionType=tvshows&refreshLibrary=false&paths=$(printf %s "$T/media/Shows" | sed 's#/#%2F#g')" '{"LibraryOptions":{"EnableTrickplayImageExtraction":true,"ExtractTrickplayImagesDuringLibraryScan":true}}' >/dev/null
fi
# Trickplay on for the movies library too.
for id in $(api GET /Library/VirtualFolders | jq -r '.[].ItemId'); do
  api POST /Library/VirtualFolders/LibraryOptions "$(api GET /Library/VirtualFolders | jq -c --arg id "$id" '.[] | select(.ItemId==$id) | {Id:$id, LibraryOptions: (.LibraryOptions + {EnableTrickplayImageExtraction:true, ExtractTrickplayImagesDuringLibraryScan:true})}')" >/dev/null
done
api POST /Library/Refresh >/dev/null
echo "series added; library refresh requested (trickplay/segments run as scheduled tasks)"
