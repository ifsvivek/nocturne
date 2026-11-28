# Server API changes

One line per change to `docs/api/openapi.yaml` after its first commit (bf51f3f), newest first.

- New module `storage`: `/api/v1/storage/roots` (GET, POST), `/roots/{id}` (GET, PUT, DELETE), `/roots/{id}/mount`, `/roots/{id}/browse`, `/mounts`; module events `mount_lost` and `mount_back`. The storage agent (`nocturne-server agent`, a separate process) serves `/agent/v1/roots...` with its own key.
- Library-manager routes moved under the module prefix: `/api/v1/library/{libraries,profiles,indexers,titles,queue,history,blocklist,jobs}...` (was `/api/v1/<name>...`). Every module's routes live under `/api/v1/<module>/`.
- `/api/v1/gateway` and `/api/v1/gateway/restart` removed: the gateway is an in-process module now and its worker documents its routes.
- `ServerConfig`: the `gateway` section is no longer a fixed field; any module's section is an additional property with at least `enabled`.
- New event `{"type":"module","data":{"module","kind","data"}}` for module-specific events.
- `PUT /api/v1/auth/password`: `current` is optional for the local admin, who may reset another user's password with `user`.
- Default port 8100 instead of 8098 (8098 is the second throwaway Jellyfin of `scripts/test-jellyfin.sh start 2`).
