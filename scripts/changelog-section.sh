#!/usr/bin/env bash
# Print the body of the `## [x.y.z]` section of CHANGELOG.md (without the heading), for the release notes.
#   usage: scripts/changelog-section.sh 0.4.0
set -euo pipefail
cd "$(dirname "$0")/.."
ver="${1:?usage: $0 <version>}"
out=$(awk -v v="$ver" '
  /^## \[/ { if (on) exit; on = index($0, "## [" v "]") == 1; next }
  on { print }
' CHANGELOG.md)
# Trim leading and trailing blank lines.
out=$(printf '%s' "$out" | sed '/./,$!d')
[ -n "$out" ] || { echo "no CHANGELOG section for $ver" >&2; exit 1; }
printf '%s\n' "$out"
