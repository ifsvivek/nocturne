# Security policy

## Reporting a vulnerability

Please report privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability** (a private security advisory). Do not open a public issue or pull request for a security problem, and do not include real tokens, server addresses or personal media in a report.

Include what you found, how to reproduce it (a throwaway server is ideal), the affected version and your distribution. This is a volunteer project: expect an acknowledgement within about a week and a fix or a mitigation plan as soon as practical. Reporters are credited in the advisory unless they prefer otherwise.

## Supported versions

Only the latest release receives fixes.

## Scope notes

- Login tokens are stored in the system keyring (Secret Service), never in a file; settings files hold no secrets.
- NOCTURNE is read-only toward your server unless you turn on `allow_server_writes` ([docs/WRITES.md](docs/WRITES.md)).
- Server certificates are validated through rustls; NOCTURNE does not disable verification.
- The app sends no telemetry. It contacts the servers you configure, the update manifest above, and only the optional services you switch on yourself (online metadata, subtitle search, Trakt, a theme address you type).
- Local media files are never modified; scanning only reads them. The local index and watch state stay on your computer.
- Updates: the app checks one small manifest on the NOCTURNE website at most once a day (Settings > Updates turns it off; the request carries only the version in the User-Agent). Before a per-user install replaces anything it verifies the download's announced size, sha256 and a detached ed25519 signature over the file bytes against public keys compiled into the app, and a failed check leaves the running version untouched; a compromised website can therefore only point at files that fail verification. Package-managed installs never update themselves. The private update key is separate from the package repositories' GPG key and is kept only on the maintainer's machine, never in CI or the repository. Windows and macOS preview builds are not code-signed.
- Themes are read as data: imported CSS is mapped to colours, sizes and layout and nothing in it is executed or fetched; a theme address you type is fetched once over https.
- The Linux package repositories (pacman, apt, dnf) are signed with a GPG key whose fingerprint is published on the website and in the README.
- Release builds do not read `.env`, so a stray file cannot point an installed app at a server. Developer API-key mode (environment variables) is a transient, read-only source.
- Issues in Jellyfin itself, the UI framework (Iced), mpv or other dependencies should go to those projects.
