#!/usr/bin/env bash
# Disk budget for this project: build cache <= 70 GB, whole project (repo + every worktree's target + sccache + this
# project's docker volumes and images) <= 100 GB. Prints a table; --enforce trims in a safe order until under both limits.
#   usage: scripts/cache-budget.sh [--enforce] [--quiet] [--prune-images]
# --quiet prints only when a limit is exceeded and then enforces (the package scripts call it that way).
# Only things this project created are measured or touched: targets, ~/.cache/nocturne/sccache, docker volumes named
# nocturne-* and images named nocturne-*. Base images our scripts pull (debian, ubuntu, fedora, archlinux, rust) are
# measured but removed only with --prune-images. Never touched: website/static/repo, keys/, any other docker object.
# Env: BUDGET_CACHE_GB (70), BUDGET_TOTAL_GB (100), STALE_DAYS (3).
set -uo pipefail
cd "$(dirname "$0")/.."
LIM_CACHE=${BUDGET_CACHE_GB:-70}; LIM_TOTAL=${BUDGET_TOTAL_GB:-100}; STALE=${STALE_DAYS:-3}
ENFORCE= QUIET= PRUNE_IMG=
for a in "$@"; do case $a in --enforce) ENFORCE=1 ;; --quiet) QUIET=1 ;; --prune-images) PRUNE_IMG=1 ;; *) echo "usage: $0 [--enforce] [--quiet] [--prune-images]" >&2; exit 2 ;; esac; done

mapfile -t trees < <(git worktree list --porcelain | sed -n 's/^worktree //p')
main=${trees[0]}
SCCACHE=${SCCACHE_DIR:-$HOME/.cache/nocturne/sccache}
BASE_IMAGES='^(debian|ubuntu|fedora|archlinux|rust):'
have_docker() { command -v docker >/dev/null 2>&1 && command -v jq >/dev/null 2>&1 && docker info >/dev/null 2>&1; }
kb() { if [ -e "$1" ]; then du -sk "$1" 2>/dev/null | cut -f1; else echo 0; fi; }
gb() { awk -v k="$1" 'BEGIN{printf "%.1f", k/1048576}'; }
# "199.2MB" / "1.5GB" / "12kB" -> KB
tokb() { awk -v s="$1" 'BEGIN{n=s+0; u=s; gsub(/[0-9.]/,"",u); m=(u~/^[Kk]/)?1:(u~/^M/)?1024:(u~/^G/)?1048576:(u~/^T/)?1073741824:1/1024; printf "%d", n*m}'; }
ours_image() { [[ $1 == nocturne-* || $1 =~ $BASE_IMAGES ]]; }

rows=(); cache_kb=0; total_kb=0
measure() {
  rows=(); cache_kb=0; total_kb=0
  local t k n sz img_kb=0 repo_kb
  for t in "${trees[@]}"; do
    [ -d "$t/target" ] || continue
    k=$(kb "$t/target"); rows+=("target|${t/#$HOME/~}/target|$k"); cache_kb=$((cache_kb+k))
  done
  if [ -n "${CARGO_TARGET_DIR:-}" ] && [ -d "$CARGO_TARGET_DIR" ]; then
    k=$(kb "$CARGO_TARGET_DIR"); rows+=("target|\$CARGO_TARGET_DIR $CARGO_TARGET_DIR|$k"); cache_kb=$((cache_kb+k))
  fi
  k=$(kb "$SCCACHE"); rows+=("sccache|${SCCACHE/#$HOME/~}|$k"); cache_kb=$((cache_kb+k))
  if have_docker; then
    while IFS=$'\t' read -r n sz; do
      [ -n "$n" ] || continue
      k=$(tokb "$sz"); rows+=("volume|$n|$k"); cache_kb=$((cache_kb+k))
    done < <(docker system df -v --format '{{json .Volumes}}' 2>/dev/null | jq -r '.[] | select(.Name|startswith("nocturne-")) | [.Name,.Size] | @tsv')
    while read -r n sz; do
      ours_image "$n" || continue
      k=$(tokb "$sz"); rows+=("image|$n|$k"); img_kb=$((img_kb+k))
    done < <(docker images --format '{{.Repository}}:{{.Tag}} {{.Size}}' 2>/dev/null)
  fi
  # repo = each checkout without its target/ (the main checkout also holds the worktrees under .claude/).
  repo_kb=$(du -sk --exclude=target --exclude=.claude "$main" 2>/dev/null | cut -f1)
  for t in "${trees[@]:1}"; do repo_kb=$((repo_kb + $(du -sk --exclude=target "$t" 2>/dev/null | cut -f1))); done
  rows+=("repo|checkouts (main + worktrees, without target)|$repo_kb")
  total_kb=$((cache_kb + img_kb + repo_kb))
}
report() {
  local r a b c
  printf '%-8s %-64s %8s\n' KIND WHAT GB
  for r in "${rows[@]}"; do IFS='|' read -r a b c <<<"$r"; printf '%-8s %-64s %8s\n' "$a" "$b" "$(gb "$c")"; done
  printf '%-8s %-64s %8s  (limit %s)\n' CACHE "targets + sccache + docker volumes" "$(gb $cache_kb)" "$LIM_CACHE"
  printf '%-8s %-64s %8s  (limit %s)\n' TOTAL "everything above" "$(gb $total_kb)" "$LIM_TOTAL"
}
over() { [ "$cache_kb" -gt $((LIM_CACHE*1048576)) ] || [ "$total_kb" -gt $((LIM_TOTAL*1048576)) ]; }
fresh() { [ -n "$(find "$1" -maxdepth 3 -type f -mtime -"$STALE" -print -quit 2>/dev/null)" ]; } # anything touched recently?

measure
if [ -z "$QUIET" ] || over; then report; fi
over || exit 0
if [ -z "$ENFORCE" ]; then
  [ -n "$QUIET" ] && ENFORCE=1 || { echo "OVER LIMIT: rerun with --enforce to trim"; exit 1; }
fi

step() { echo "-- $1"; }
done_if_ok() { measure; over || { report; exit 0; }; }

step "stale worktree targets (untouched for $STALE days; the main checkout's target is kept)"
for t in "${trees[@]:1}"; do
  [ -d "$t/target" ] && ! fresh "$t/target" && { echo "   rm $t/target"; rm -rf "$t/target"; }
done
done_if_ok

step "profiles unused for $STALE days, and incremental caches"
for t in "${trees[@]}"; do
  for p in debug release fast-release doc; do
    d=$t/target/$p; [ -d "$d" ] && ! fresh "$d" && { echo "   rm $d"; rm -rf "$d"; }
  done
  find "$t/target" -maxdepth 3 -name incremental -type d -prune -exec rm -rf {} + 2>/dev/null
done
done_if_ok

if [ -d "$SCCACHE" ]; then
  step "sccache entries older than 14 days, then 3 days"
  find "$SCCACHE" -type f -mtime +14 -delete 2>/dev/null
  measure; over && { find "$SCCACHE" -type f -mtime +3 -delete 2>/dev/null; }
  done_if_ok
fi

if have_docker; then
  step "docker volumes nocturne-* (caches, rebuilt on demand; volumes in use are skipped by docker)"
  for v in $(docker volume ls -q | grep '^nocturne-'); do
    docker volume rm "$v" >/dev/null 2>&1 && echo "   rm volume $v"
    done_if_ok
  done
  if [ -n "$PRUNE_IMG" ]; then
    step "docker images nocturne-* and base images (--prune-images)"
    while read -r n; do ours_image "$n" && docker rmi "$n" >/dev/null 2>&1 && echo "   rm image $n"; done < <(docker images --format '{{.Repository}}:{{.Tag}}')
    done_if_ok
  fi
fi

step "every worktree target (cargo clean)"
for t in "${trees[@]}"; do [ -d "$t/target" ] && { echo "   rm $t/target"; rm -rf "$t/target"; }; done
measure; report
over && { echo "STILL OVER LIMIT (repo or docker images; try --prune-images)"; exit 1; }
exit 0
