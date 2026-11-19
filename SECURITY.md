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
- Updates: the app checks one small manifest on the NOCTURNE website at most once a day (Settings > Updates turns it off; the request carries only the version in the User-Agent). Before a per-user install replaces anything it verifies the download's announced size, sha256 and a detached ed25519 signature over the file bytes against public keys compiled into the app, and a failed check leaves the running version untouched; a compromised website can therefore only point at files that fail verification. Package-managed installs never update themselves: Update now asks PackageKit over the system D-Bus to update only the `nocturne` package (trusted packages only), so the change goes through PackageKit and the desktop's polkit prompt; NOCTURNE runs no package manager and never handles a password. The private update key is separate from the package repositories' GPG key and is kept only on the maintainer's machine, never in CI or the repository. Windows and macOS preview builds are not code-signed.
- Themes are read as data: imported CSS is mapped to colours, sizes and layout and nothing in it is executed; `url()` in a declaration is never fetched.
- Remote theme parts: a theme (or the branding CSS of a server you use) can `@import` other stylesheets and load `@font-face` fonts. NOCTURNE fetches them only over https (a redirect must be https too), without cookies, credentials or a `Referer`, with a generic User-Agent, a 10 s timeout, and hard limits (stylesheets 256 KB each and 1 MB together, 16 imports, depth 3; fonts 2 MB each and 8 MB together, 12 files). Addresses with a user name or password and literal loopback or private network addresses are refused, so a stylesheet cannot make the app probe your network (a public name that resolves to a private address is not detected). Stylesheets must be `text/css` or `text/plain`; fonts must carry a font signature and are decoded by memory-safe Rust and parsed before use. Downloads are cached on disk by address and refreshed only when you press Refresh or the text changes; nothing polls. The one thing a remote host can learn is your IP address and that a theme is in use. Settings has a switch that turns all of this off.
- The Linux package repositories (pacman, apt, dnf) are signed with a GPG key whose fingerprint is published on the website and in the README.
- Release builds do not read `.env`, so a stray file cannot point an installed app at a server. Developer API-key mode (environment variables) is a transient, read-only source.
- Issues in Jellyfin itself, the UI framework (Iced), mpv or other dependencies should go to those projects.
