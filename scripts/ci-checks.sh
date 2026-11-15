#!/usr/bin/env bash
# Release gate: fmt, clippy, tests, licence policy, secret scan. Run from anywhere; exits non-zero on first failure.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== fmt";    cargo fmt --all -- --check
echo "== clippy"; cargo clippy --workspace --all-targets --locked -- -D warnings
echo "== test";   cargo test --workspace --locked

echo "== licences"
if command -v cargo-deny >/dev/null; then cargo deny check licenses bans sources
else echo "cargo-deny not installed: skipped (install: cargo install cargo-deny --locked)"; fi

echo "== secret scan (matches are reported as file/commit only, never printed)"
fail=0
# Token-shaped strings: 32-hex Jellyfin tokens, bearer/api_key assignments, private keys, common cloud/VCS tokens.
pat='(^|[^0-9a-fA-F])[0-9a-f]{32}([^0-9a-fA-F]|$)|(api_?key|token|password|secret)["'"'"' ]*[:=]["'"'"' ]*[A-Za-z0-9_-]{16,}|-----BEGIN [A-Z ]*PRIVATE KEY|gh[pousr]_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|Bearer [A-Za-z0-9._-]{20,}'
if git grep -InEc "$pat" -- . ':!Cargo.lock' | grep -v ':0$' ; then echo "^ token-like strings in tracked files (review; Cargo.lock excluded)"; fail=1; fi
revs=$(git rev-list --all)
# shellcheck disable=SC2086
if git grep -InEc "$pat" $revs -- . ':!Cargo.lock' | grep -v ':0$' | sed 's/^\([0-9a-f]\{8\}\)[0-9a-f]*:/\1:/' | sort -u | head -50 | grep . ; then
  echo "^ token-like strings in history (commit:path:count)"; fail=1; fi
# Any secret-looking value present in this environment must not occur in tracked files or history.
while IFS='=' read -r k v; do
  [ "${#v}" -ge 8 ] || continue
  if git grep -qIF -e "$v" -- . || git grep -qIF -e "$v" $revs -- .; then echo "env value of $k occurs in the repository"; fail=1; fi
done < <(env | grep -E '^[A-Za-z0-9_]*(TOKEN|SECRET|PASSWORD|PASSWD|API_?KEY|NOCTURNE_JF_[A-Z_]*)=' || true)
[ "$fail" = 0 ] && echo "secret scan: clean" || { echo "secret scan: REVIEW NEEDED"; exit 1; }
echo "== all checks passed"
