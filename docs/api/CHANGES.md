# Server API changes

One line per change to `docs/api/openapi.yaml` after its first commit (bf51f3f), newest first.

- New `GET /api/v1/logs?lines=` (admin): recent server log lines, redacted.
- New `GET /api/v1/library/calendar?from=&to=`: dated units of monitored titles (`CalendarEntry`).
- New `GET /api/v1/library/lookup?kind=&q=`: metadata search for adding a title (`LookupHit`).
- `Status.disk`: free and total bytes per file system the server writes to.
- Dashboard responses carry a strict Content-Security-Policy, `X-Frame-Options: DENY` and `Referrer-Policy: no-referrer`.
- Library-manager routes moved under the module prefix: `/api/v1/library/{libraries,profiles,indexers,titles,queue,history,blocklist,jobs}...` (was `/api/v1/<name>...`). Every module's routes live under `/api/v1/<module>/`.
- `/api/v1/gateway` and `/api/v1/gateway/restart` removed: the gateway is an in-process module now and its worker documents its routes.
- `ServerConfig`: the `gateway` section is no longer a fixed field; any module's section is an additional property with at least `enabled`.
- New event `{"type":"module","data":{"module","kind","data"}}` for module-specific events.
- `PUT /api/v1/auth/password`: `current` is optional for the local admin, who may reset another user's password with `user`.
- Default port 8100 instead of 8098 (8098 is the second throwaway Jellyfin of `scripts/test-jellyfin.sh start 2`).
