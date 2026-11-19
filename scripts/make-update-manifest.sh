#!/bin/sh
# Maintainer-only release step: write the update manifest (docs/dev/UPDATER_PLAN.md) for signed artifacts.
#   scripts/make-update-manifest.sh <version> <notes_url> <artifact>...   -> stable.json (OUT=path overrides)
# Each artifact needs its <artifact>.sig from scripts/sign-update.sh. The platform comes from the file name:
#   *-x86_64-linux.tar.gz        linux-x86_64     (the tarball from scripts/package.sh)
#   *-x86_64-windows.zip         windows-x86_64   (the portable zip, holds nocturne.exe)
#   *-aarch64-macos.tar.gz       macos-aarch64    (tar.gz of NOCTURNE.app; also *-x86_64-macos.tar.gz)
# Other files are rejected. URLs point at the GitHub release (RELEASE_BASE overrides the download base).
set -eu
[ $# -ge 3 ] || { echo "usage: $0 <version> <notes_url> <artifact>..." >&2; exit 2; }
ver=$1 notes=$2; shift 2
base="${RELEASE_BASE:-https://github.com/ifsvivek/nocturne/releases/download/v$ver}"
out="${OUT:-stable.json}"
plat='{}'
for a in "$@"; do
  n=$(basename "$a")
  case $n in
  *-x86_64-linux.tar.gz) k=linux-x86_64 ;;
  *-x86_64-windows.zip) k=windows-x86_64 ;;
  *-aarch64-macos.tar.gz) k=macos-aarch64 ;;
  *-x86_64-macos.tar.gz) k=macos-x86_64 ;;
  *) echo "error: cannot tell the platform of $n" >&2; exit 1 ;;
  esac
  [ -f "$a.sig" ] || { echo "error: missing $a.sig (run scripts/sign-update.sh)" >&2; exit 1; }
  plat=$(printf '%s' "$plat" | jq --arg k "$k" --arg url "$base/$n" --argjson size "$(stat -c%s "$a")" \
    --arg sha "$(sha256sum "$a" | cut -d' ' -f1)" --arg sig "$(cat "$a.sig")" \
    '.[$k] = {url: $url, size: $size, sha256: $sha, signature: $sig}')
done
jq -n --arg v "$ver" --arg d "$(date +%F)" --arg notes "$notes" --argjson p "$plat" \
  '{version: $v, date: $d, notes_url: $notes, platforms: $p}' > "$out"
echo "wrote $out"
