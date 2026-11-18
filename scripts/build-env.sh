# Sourced by the build scripts (and by agents before a manual cargo build): shared compile cache for every worktree.
# sccache is optional (cargo install --locked sccache); without it nothing changes. Disable with NOCTURNE_NO_SCCACHE=1.
# SCCACHE_BASEDIRS lists every worktree so the same dependency compiled from a different checkout path is a cache hit
# (without it sccache's key includes the working directory and Rust hits drop to 0 %).
if [ -z "${NOCTURNE_NO_SCCACHE:-}" ] && [ -z "${RUSTC_WRAPPER:-}" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache
  export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/nocturne/sccache}"
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-30G}"
  export SCCACHE_BASEDIRS="$(git worktree list --porcelain 2>/dev/null | sed -n 's/^worktree //p' | paste -sd: -)"
  # The server reads the list at start: restart it if it does not know this checkout yet (a build running in another worktree at that moment fails once; rerun it).
  # ponytail: one shared server; per-worktree servers (SCCACHE_SERVER_PORT) if restarts ever bite.
  top=$(git rev-parse --show-toplevel 2>/dev/null) && { sccache --show-stats 2>/dev/null | grep -qF "$top/" || sccache --stop-server >/dev/null 2>&1; }
  unset top
fi
