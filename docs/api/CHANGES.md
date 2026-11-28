# Server API changes

One line per change to `docs/api/openapi.yaml` after its first commit (bf51f3f), newest first.

- Module `discovery`: `GET /api/v1/discovery/servers` (a two-second LAN scan; the dashboard's guessed path).
- Module `tunnel`: `GET /api/v1/tunnel/status`, `POST /api/v1/tunnel/restart`, `PUT` and `DELETE /api/v1/tunnel/token`.
- Module `gateway`: `GET /api/v1/gateway/status`, `POST /api/v1/gateway/restart`, `GET /api/v1/gateway/devices`, `DELETE /api/v1/gateway/devices/{id}`, `POST /api/v1/gateway/quickconnect/{code}`, `POST /api/v1/gateway/password`, `POST /api/v1/gateway/password/reveal`, `POST /api/v1/gateway/apikeys`.
- Module `sources`: `GET` and `POST /api/v1/sources` (the dashboard's guess), `POST /api/v1/sources/quickconnect` and `/quickconnect/complete`, `GET`, `PUT` and `DELETE /api/v1/sources/{id}`, `POST /api/v1/sources/{id}/signin` and `/check`.

- Library-manager routes moved under the module prefix: `/api/v1/library/{libraries,profiles,indexers,titles,queue,history,blocklist,jobs}...` (was `/api/v1/<name>...`). Every module's routes live under `/api/v1/<module>/`.
- `/api/v1/gateway` and `/api/v1/gateway/restart` removed: the gateway is an in-process module now and its worker documents its routes.
- `ServerConfig`: the `gateway` section is no longer a fixed field; any module's section is an additional property with at least `enabled`.
- New event `{"type":"module","data":{"module","kind","data"}}` for module-specific events.
- `PUT /api/v1/auth/password`: `current` is optional for the local admin, who may reset another user's password with `user`.
- Default port 8100 instead of 8098 (8098 is the second throwaway Jellyfin of `scripts/test-jellyfin.sh start 2`).
