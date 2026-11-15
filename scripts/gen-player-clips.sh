#!/usr/bin/env bash
# Adds a player-feature test movie to the throwaway Jellyfin library (scripts/test-jellyfin.sh must be running):
# 90 s 720p, two audio tracks (eng stereo, jpn 5.1), two embedded subtitle tracks (eng, spa), chapters,
# plus an external .srt sidecar. Everything lives under $NOCTURNE_JF_TEST_DIR (default /tmp/nocturne-jf-test).
set -euo pipefail
T="${NOCTURNE_JF_TEST_DIR:-/tmp/nocturne-jf-test}"
case "$T" in /tmp/?*) ;; *) echo "refusing: test dir must be under /tmp" >&2; exit 1;; esac
case "$T" in *..*) echo "refusing: '..' in test dir" >&2; exit 1;; esac
D="$T/media/Movies/Player Test (2010)"
mkdir -p "$D"
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
for l in eng spa; do
  { for i in 0 1 2 3 4 5; do
      printf '%d\n00:00:%02d,000 --> 00:00:%02d,800\n%s subtitle line %d\n\n' $((i+1)) $((i*10+2)) $((i*10+2)) "$l" $((i+1))
    done; } > "$W/$l.srt"
done
cat > "$W/chapters.txt" <<'EOM'
;FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=30000
title=Opening
[CHAPTER]
TIMEBASE=1/1000
START=30000
END=60000
title=Middle
[CHAPTER]
TIMEBASE=1/1000
START=60000
END=90000
title=Finale
EOM
ffmpeg -v error -y -f lavfi -i "testsrc2=size=1280x720:rate=30:duration=90" \
  -f lavfi -i "sine=frequency=440:duration=90" -f lavfi -i "sine=frequency=880:duration=90" \
  -i "$W/eng.srt" -i "$W/spa.srt" -i "$W/chapters.txt" \
  -map 0:v -map 1:a -map 2:a -map 3 -map 4 -map_metadata 5 \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -ac:a:1 6 -c:s srt \
  -metadata:s:a:0 language=eng -metadata:s:a:1 language=jpn \
  -metadata:s:s:0 language=eng -metadata:s:s:1 language=spa \
  -disposition:a:0 default "$D/Player Test (2010).mkv"
cp "$W/eng.srt" "$D/Player Test (2010).en.srt"
# shellcheck disable=SC1091
set -a; . "$T/test.env"; set +a
curl -fsS -m 20 -X POST -H "Authorization: MediaBrowser Token=\"$NOCTURNE_JF_TEST_TOKEN\"" "$NOCTURNE_JF_TEST_URL/Library/Refresh" -o /dev/null
echo "added Player Test (2010); library refresh requested"
