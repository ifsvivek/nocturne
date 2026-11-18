# Sourced by the build scripts (and by agents before a manual cargo build): shared compile cache for every worktree.
# sccache is optional (cargo install --locked sccache); without it nothing changes. Disable with NOCTURNE_NO_SCCACHE=1.
if [ -z "${NOCTURNE_NO_SCCACHE:-}" ] && [ -z "${RUSTC_WRAPPER:-}" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache
  export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/nocturne/sccache}"
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-30G}"
fi
