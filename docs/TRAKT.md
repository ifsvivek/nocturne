# Trakt (opt-in)

Settings > Trakt. Off by default; nothing is sent until you connect and switch a row on. NOCTURNE ships **no** Trakt
credentials and Trakt issues no client secret: create your own app at <https://trakt.tv/oauth/applications>, register
`http://127.0.0.1:38917/callback` as its redirect URI and paste its **Client ID** into the dialog. The client id and the
sign-in tokens are kept in the system keyring only.

## What happens

- **Sign-in**: OAuth 2.0 authorization code with PKCE (RFC 7636) for a native app (RFC 8252). NOCTURNE makes a random
  `code_verifier` (32 random bytes, base64url, 43 characters), its S256 `code_challenge` and a random `state`, starts a
  one-shot listener on `127.0.0.1:38917` (bound before the browser opens), and opens
  `https://auth.trakt.tv/oauth/authorize?response_type=code&client_id=..&redirect_uri=..&code_challenge=..&code_challenge_method=S256&state=..`
  in the system browser. The redirect to `/callback?code=..&state=..` is accepted only with the right `state`
  (strays and wrong states get an error page and are ignored); the page the user sees says "You can close this tab". The
  wait ends on success, denial, Cancel or after five minutes. The code is exchanged with
  `POST /oauth/token` `{code, client_id, redirect_uri, grant_type: authorization_code, code_verifier}` and **no client
  secret**. The token is refreshed with `{refresh_token, client_id, redirect_uri, grant_type: refresh_token}` an hour
  before it expires (Trakt returns a new refresh token each time; it is stored at once). Disconnect calls
  `POST /oauth/revoke` and deletes the keyring entries. A refused refresh means "connect again".
  The port is a constant (`REDIRECT_PORT` in `crates/core/src/trakt.rs`), not a setting.
- **Scrobble**: `POST /scrobble/start|pause|stop` with `{movie|show+episode: {ids}, progress}`. Sent when an item starts,
  when pause flips and when it stops, from the player's reporting path, in order from one queue. Movies by IMDb / TMDB /
  TVDB id; episodes by their show's ids plus season and episode number. Items without ids are not sent. A 409 (already
  scrobbled) counts as done.
- **History import** (optional): `GET /sync/watched/movies` and `/sync/watched/shows`; matching local items are marked
  watched in NOCTURNE's own state, server items only when server changes are on. Runs on connect and at launch. Imported
  state never spreads to other sources ([WRITES.md](WRITES.md)).

## What is sent, to whom

To `trakt.tv` / `api.trakt.tv` only: the sign-in exchange (client id, code, verifier, tokens), and while scrobbling the
provider ids of what you play, season / episode numbers and progress percent. Never titles, file names, paths, server
addresses or media server credentials. Every request carries `User-Agent: NOCTURNE/<version> (+https://nocturne.ifsvivek.in)`
(Trakt's Cloudflare rejects default library user agents, see api-help issue 1052), `trakt-api-version: 2`,
`trakt-api-key: <client id>` and, once signed in, `Authorization: Bearer <token>`.

## Requirements and limits relied on (verify at release)

- PKCE guide <https://developer.trakt.tv/docs/pkce>: authorize at `https://auth.trakt.tv/oauth/authorize` with
  `code_challenge` and `code_challenge_method=S256` (only S256), exchange at `POST /oauth/token` with `code` and
  `code_verifier`, "Do not send a `client_secret`". Client secret is optional and deprecated on every `/oauth` body
  (refresh, revoke included): <https://github.com/trakt/api-help/pull/974>. Token host `auth.trakt.tv`:
  <https://github.com/trakt/api-help/issues/1052>.
- **Redirect URI caveat**: that guide says to use `https://` redirect URIs and to avoid custom schemes and `localhost`
  addresses (and that existing localhost apps "will start seeing the warning. This is intended."). The loopback address
  `http://127.0.0.1:<port>/callback` is the RFC 8252 pattern for desktop apps but is **not mentioned either way** by that
  page; it must be tried with the real app. If Trakt rejects it, the fallback is a small https page of the maintainer's
  that forwards to the loopback (decision for the maintainer).
- Refresh with only `client_id` is documented as allowed (secret optional) but not confirmed in practice.
- Rate limits (<https://trakt.docs.apiary.io/>, deprecated copy of the docs): authenticated POST/PUT/DELETE 1 call per
  second, GET 1000 per 5 minutes, 429 carries `Retry-After`. NOCTURNE spaces calls 1.1 s apart and retries a 429 once
  after `Retry-After` (at most 30 s).
- Required headers (same page): `Content-Type: application/json`, `User-Agent`, `trakt-api-key`, `trakt-api-version: 2`;
  "The API should always be accessed over SSL": release builds only talk to https hosts (a debug-only
  `NOCTURNE_TRAKT_URL` points at a loopback mock; the loopback redirect itself is local).
- Images: "All images are required to be cached ... not loaded directly from our CDN": NOCTURNE loads no Trakt images.
  Comments: not used.
- Stored Trakt data: only the tokens and the client id (keyring). Watched history is read, matched in memory and not kept.
- Credit: plain text under the Trakt dialog and Settings row ("Trakt is a trademark of Trakt, Inc. ... not affiliated").
  No Trakt logo is used. The branding and terms pages (<https://trakt.tv/branding>, <https://trakt.tv/terms>, now at
  app.trakt.tv) could not be fetched by tooling here (oversized headers), so **the maintainer must read the "Trakt API
  requirements, including the branding and usage guidelines" accepted on the app form** and adjust
  `trakt::CREDIT` (or add the logo) if they ask for something else.

## Testing

Unit tests use a loopback mock of the authorize/token/scrobble/sync endpoints and the real loopback listener (state check,
strays, denial, timeout, cancel, busy port). A live run needs the maintainer's own app: debug builds prefill the client id
from `NOCTURNE_TRAKT_CLIENT_ID`; the id is never logged.

## Not verified against the real Trakt

Everything above that says "documented": the loopback redirect being accepted, refresh without a secret, the real
response shapes of the scrobble and watched endpoints (written from the docs), token lifetime (the code reads
`expires_in`), and rate behaviour.
