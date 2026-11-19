# Updater plan

Phase 1 (done, `crates/core/src/update.rs`): find out that a newer version exists and say so, for every install kind. Phase 2 (done, `crates/core/src/update_install.rs`, see "What shipped in phase 2"): the signed in-app update for per-user installs. Decision source: docs/dev/V05_BRIEF.md section 3.

## Manifest

One small JSON file served by the website Worker at `https://nocturne.ifsvivek.in/update/stable.json` (a `beta.json` can follow the same shape). Written by the maintainer at release time, never by the app.

```json
{
  "version": "0.5.0",
  "date": "2026-12-01",
  "notes_url": "https://github.com/ifsvivek/nocturne/releases/tag/v0.5.0",
  "platforms": {
    "linux-x86_64": { "url": "https://.../nocturne-0.5.0-x86_64-linux.tar.gz", "size": 25000000, "sha256": "hex", "signature": "base64" },
    "windows-x86_64": { "url": "...", "size": 0, "sha256": "hex", "signature": "base64" },
    "macos-aarch64": { "url": "...", "size": 0, "sha256": "hex", "signature": "base64" }
  }
}
```

The app accepts at most 64 KiB, requires `version` (semver, pre-releases allowed) and an https `notes_url`, ignores unknown fields. `platforms` is optional in phase 1 and read only in phase 2 (a platform entry missing means "no self-update, show Download").

## Signature scheme (phase 2)

- A dedicated **ed25519 update key**, separate from the GPG key of the pacman repository (different blast radius: a leaked repo key must not sign binaries pushed to Windows and macOS users, and the other way round).
- Detached signature over the **artifact bytes** (not the manifest): `signature` in the manifest is base64 of the 64-byte signature of the file. The manifest itself is unsigned, so a compromised website can only point at artifacts, which then fail verification. The app also checks `size` and `sha256` before it hashes for the signature, to fail early and give a clear message.
- Public key compiled into the app as a constant (32 bytes), with room for two keys (current and next) so a rotation ships in a normal release before the old key retires.
- Verification: `ed25519-dalek` (or `ring`/`aws-lc-rs` if already in the tree: check `cargo tree` first, avoid a new dependency; `cargo deny check licenses` and `scripts/gen-notices.py` if one is added). Format: raw ed25519 signature, base64; `minisign` files are an acceptable alternative if the maintainer prefers the tool, the verification code is the same size.
- The private key is handled like `keys/`: only on the maintainer's machine, never in CI, the repository or the website; signing is a local release step (`scripts/sign-update.sh`, to be written in phase 2, produces the manifest) after the artifacts are built. Revocation: ship a release with the next key; clients on a stolen key are protected only by an out-of-band notice, which is why two compiled-in keys matter.
- Downgrade protection: the app refuses a manifest whose `version` is not newer than the running one (already in `decide`), so replaying an old signed manifest does nothing.

## Install-kind detection

`update::detect` runs once per launch, without a subprocess.

| Kind | How detected | Action |
|---|---|---|
| Dev | debug build, or a `target` directory in the exe path | never checks |
| System package | exe under `/usr` (not `/usr/local`) and one of `/var/lib/pacman/local`, `/var/lib/dpkg/status`, `/var/lib/rpm` or `/usr/lib/sysimage/rpm` exists | never touches itself. Pacman: notice with the copyable command `sudo pacman -Syu`. Apt: notice with `sudo apt update && sudo apt upgrade`; Dnf: `sudo dnf upgrade` (both repositories exist now) |
| Per-user | everything else: tarball (`install.sh` defaults to `/usr/local`; a per-user mode under `~/.local` is part of 0.5), Windows per-user NSIS (`%LOCALAPPDATA%\Programs\NOCTURNE`), macOS `.app` | phase 1: Download (release page). Phase 2: in-app update when the install location is writable by the user, otherwise Download |

Why a path test and not `pacman -Qo` / `dpkg -S` / `rpm -qf`: those cost a fork and a database lock, can stall on a busy database or a system under `pacman -Syu`, and answer a question the path already answers (`/usr/bin` is only ever written by a package manager; our own installer uses `/usr/local` or `~`). The known gap is a user running `PREFIX=/usr ./install.sh`, which is unsupported and mis-labelled as package-owned (the result is a harmless "run pacman" hint, never a self-modification). If that ever matters, a one-time `pacman -Qo` with a 2 s timeout off the UI thread can confirm before showing the command.

## Throttle, privacy, failures

- At most one GET per 24 h: the time of the last successful check is stored as unix seconds in `$XDG_DATA_HOME/nocturne/update-check`; a clock set back counts as due. Checked at startup only (no timer, no background polling: PLATFORMS.md footprint rules).
- The request carries only `User-Agent: NOCTURNE/<version>`, no identifier, no cookies, no query string, and the IP address that any HTTP request reveals. 10 s timeout, 64 KiB cap, runs through `bridge::spawn`.
- The setting `check_updates` (Settings, section "Updates") turns it off. Developer builds never check.
- Failures are silent: no network, a non-200, bad JSON or an oversized body all end as "nothing to show", and a failure does not stamp, so the next launch tries again. Phase 2 download failures surface as a toast and leave the staging file deleted; the running install is never touched before the signature verified.

## Phase 2 swap mechanics

Common: download to a staging directory next to the install (same filesystem so renames are atomic), stream into a hash, verify size, sha256 and the ed25519 signature, only then touch the install. Keep the old version as `.old` until the new one has started once; offer "Restart to update" (the app re-executes itself). A failed start of the new version restores `.old`.

- **Linux per-user** (`~/.local`): the tarball unpacks into staging; `rename()` the new `bin/nocturne` over the old one (atomic; the running process keeps its mapped inode, which is why replacing a running binary works on Linux), same for the data files from `install.sh`; restart via `exec` of the new path. If the user ran it from `/usr/local` (root-owned), not writable: Download.
- **Windows** (`%LOCALAPPDATA%\Programs\NOCTURNE`, `RequestExecutionLevel user`): a running exe cannot be overwritten but can be renamed. Rename `nocturne.exe` to `nocturne.exe.old`, move the new one in, start it, delete `.old` on the next launch. The simplest robust variant is to download the signed NSIS installer and run it silently (`/S`), which already handles the registry and shortcuts; it needs the app to exit first (small helper launch then quit), so the rename path is preferred for the exe and the NSIS path for anything that changes more files. Open point: SmartScreen reputation for an unsigned (no Authenticode) installer downloaded by the app (files written by our own process get no Mark-of-the-Web, which actually avoids the prompt; a browser download would show it).
- **macOS** (`.app`, drag-installed to `/Applications` or `~/Applications`): replace the whole bundle: unpack the new `.app` next to the old one, swap with `rename` (or `renamex_np` with `RENAME_SWAP`), relaunch with `open`. Files our own process writes carry no quarantine attribute, so Gatekeeper does not re-evaluate the downloaded bundle; without a Developer ID signature and notarisation the first install from a browser still needs the right-click Open step. If the bundle is in `/Applications` and not writable by the user, Download. An unsigned bundle replaced in place keeps working but Apple Silicon requires at least an ad-hoc signature: the release script already signs ad hoc when `SIGN_ID` is unset, keep that.
- **System packages**: never. They show the notice and the command (or the release page) and leave everything to the package manager.

## What shipped in phase 2

Decisions taken: raw ed25519 detached signature over each artifact (base64 of 64 bytes in the manifest), Windows rename-and-replace of the running exe, system packages never self-update, update checks default on.

- **Verification** uses `aws-lc-rs` (already in the tree through rustls; added to `nocturne-core` with `default-features = false`, so no new crate and no new code in the binary). `KEYS` in `update_install.rs` holds the compiled-in public key (`include_str!` of `packaging/update/nocturne-update-key.pub`) and an empty second slot for a rotation; `verify(keys, bytes, sig)` takes the keys as a parameter so tests inject their own, production passes `KEYS`.
- **Flow**: `fetch_verified` streams the artifact through `bridge::spawn` into `$XDG_DATA_HOME/nocturne/update-staging`, capped at the manifest `size` (checked against `Content-Length` and while streaming), hashes while writing, compares sha256, reads the file back, checks the signature, and only then renames `artifact.part` to `artifact`. Any failure deletes the staging directory. Only `https` URLs (plus loopback http for tests). Nothing unverified is ever unpacked or executed. `install` then unpacks next to the target (same filesystem) and swaps. `update` chains both for the app.
- **Gating**: `decide` yields `Action::Install` only for `PerUser` installs whose directory the user can write (`can_self_update`) and whose manifest has an entry for `<os>-<arch>`; otherwise Download. `fetch_verified` and `install` refuse `Dev` and `SystemPackage` themselves, so no caller can reach them with those kinds.
- **Swap per platform**: Linux per-user (`~/.local/bin/nocturne`): the new `nocturne` is taken from the tarball's `<name>/nocturne`, the old file is hard-linked to `nocturne.old`, then `rename` over the target (atomic; the running process keeps its inode). Windows: the exe is renamed to `nocturne.exe.old`, the new one moved in, undone if the move fails (the artifact is the portable zip, holding `nocturne.exe`). macOS: the whole `.app` is renamed to `NOCTURNE.app.old` and the new bundle moved in (the artifact is a tar.gz of `NOCTURNE.app`; a `.dmg` stays the first-install format). The `.old` file or bundle is deleted at the next start (`cleanup`, run once from `App::boot` for non-dev builds, which also wipes the staging and work directories). Unpacking uses the system `tar` (present on Linux, macOS and Windows 10+), so there is no archive crate. Only the executable (or bundle) is replaced; desktop entry, icons and metainfo stay as `install.sh` wrote them.
- **App**: the banner shows Download and install, a percentage while downloading, then Restart to update (or the error with Try again). Restart calls `request_restart` and the normal `quit` path (player shutdown, final report, settings save); `main` calls `restart_if_requested` after the UI loop and all guards are gone: `exec` on Linux and macOS, spawn then exit on Windows.
- **Release side**: `scripts/sign-update.sh <artifact>...` (private key only through `openssl pkeyutl -sign -rawin`, `UPDATE_KEY` overrides the default `keys/nocturne-update-key.pem`) and `scripts/make-update-manifest.sh <version> <notes_url> <artifact>...` (platform from the file name, sha256 and size computed, `OUT` and `RELEASE_BASE` overridable). See docs/PACKAGING.md "Releasing".
- **Tests**: unit tests in `update_install.rs` (accept, wrong key, rotation slot, tampered byte, truncated data and signature, swap and `.old` cleanup, failed swap untouched, directory swap, tarball install, Dev and System refused) and `crates/core/tests/update_install.rs` (loopback server, real release scripts with a temporary openssl key: download, verify, swap, new binary runs, `.old` cleaned; sha mismatch, size cap with and without Content-Length, short body, wrong key, forged artifact with matching sha, non-https, 404, staging cleaned, no request for System and Dev kinds).
- **Not run**: the Windows and macOS swaps (only their shared directory-rename logic runs on Linux; the first real run is the CI preview build), the GUI banner flow, and the final `exec` itself (the test runs the swapped path with the original arguments, which is what `exec` starts). Gatekeeper: files written by our own process carry no quarantine attribute, so a replaced bundle is not re-assessed; a browser-downloaded first install still needs the right-click Open step until the app is notarised.

## Open questions for the maintainer

(Answered for phase 2: 1 on, 3 repositories exist, 4 raw ed25519, 6 rename-and-replace. Still open: 2 the Worker serves the manifest at release time, 5 key custody is in `keys/README.md`.)


1. **Default for `check_updates`: on or off?** Phase 1 ships it **on** (the brief: "a setting turns update checks off"; users of package-managed installs also benefit, and the request is one tiny GET a day carrying only the version). The privacy-first alternative is off by default with a one-time question in onboarding. Recommendation: on, as built, disclosed in the setting's hint and the README.
2. **Manifest host path**: `https://nocturne.ifsvivek.in/update/stable.json` served by the website Worker (static file with `Cache-Control: max-age=3600`). The website agent creates it at release time; the first release that contains the check should be published together with the file.
3. **apt / dnf repositories**: until the .deb / .rpm are served from a repository, `apt upgrade` finds nothing, so those installs show Download instead of a command. Do we want repositories (like the pacman one), or keep Download?
4. **Signature format**: raw ed25519 over the artifact (recommended) or minisign files?
5. **Key custody**: generate the update key in `keys/` next to the GPG backup (same rules), and decide how many keys are compiled in (recommended: two).
6. **Windows self-update path**: rename-and-replace the exe (recommended) or run the signed installer silently.
