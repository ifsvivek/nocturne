## What and why

<!-- One concern per pull request. -->

## Checklist

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` are clean
- [ ] `cargo test --workspace` passes (or `scripts/ci-checks.sh`)
- [ ] Anything that writes to a server was tested only on a throwaway Jellyfin, never on a server with real data
- [ ] No tokens, server addresses, usernames, media titles or personal paths in code, tests, logs or screenshots
- [ ] New dependencies pass `cargo deny check licenses`; `THIRD_PARTY_NOTICES.md` regenerated if they changed
- [ ] Docs updated if behaviour or writes changed (`docs/WRITES.md`, `docs/FEATURES.md`)
