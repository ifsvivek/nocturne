# Security policy

## Reporting a vulnerability

Please report privately through GitHub: open the repository's **Security** tab and choose
**Report a vulnerability** (a private security advisory). Do not open a public issue or pull request for a
security problem, and do not include real tokens, server addresses or personal media in a report.

Include what you found, how to reproduce it (a throwaway server is ideal), the affected version and your
distribution. This is a volunteer project: expect an acknowledgement within about a week and a fix or a
mitigation plan as soon as practical. Reporters are credited in the advisory unless they prefer otherwise.

## Supported versions

Only the latest release receives fixes.

## Scope notes

- Login tokens are stored in the system keyring (Secret Service), never in a file; settings files hold no secrets.
- NOCTURNE is read-only toward your server unless you turn on `allow_server_writes`
  ([docs/WRITES.md](docs/WRITES.md)).
- Server certificates are validated through rustls; NOCTURNE does not disable verification.
- The app sends no telemetry and contacts only the Jellyfin server(s) you configure.
- Local media files are never modified; scanning only reads them. The local index and watch state stay on your computer.
- Release builds do not read `.env`, so a stray file cannot point an installed app at a server. Developer API-key mode
  (environment variables) is a transient, read-only source.
- Issues in Jellyfin itself, the UI framework (Iced), mpv or other dependencies should go to those projects.
