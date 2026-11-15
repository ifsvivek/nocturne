# SyncPlay ("Watch together")

One user on a Jellyfin server creates a group, other users of the same server join it, and play, pause, seek, stop and
next-item stay in step across everyone's player. This page records what was checked against official sources and the
real server, how NOCTURNE implements it, and its limits. Everything here was checked against Jellyfin 12.4.0
(the version `scripts/test-jellyfin.sh` runs).

## Sources read

- OpenAPI document, version 12.4.0: https://api.jellyfin.org/openapi/jellyfin-openapi-stable.json (SyncPlay tag, `GetUtcTime`,
  `UserPolicy.SyncPlayAccess`, the `SyncPlay*Update` and `SendCommand` schemas).
- Server: `Emby.Server.Implementations/SyncPlay/{Group,SyncPlayManager}.cs`,
  `MediaBrowser.Controller/SyncPlay/GroupStates/{Waiting,Playing,Paused,Abstract}GroupState.cs`,
  `Jellyfin.Api/Controllers/SyncPlayController.cs`, `tests/.../SyncPlayLostWebSocketTests.cs` (jellyfin/jellyfin, master).
- Official web client, behaviour only (its licence is GPL-2.0; no code was copied, the algorithms were re-implemented):
  `src/plugins/syncPlay/core/{Manager,PlaybackCore,QueueCore,Controller}.js` and `core/timeSync/{TimeSync,TimeSyncServer}.js`
  (jellyfin/jellyfin-web, master).

## REST (all under the signed-in user's token)

| Call | Body | Notes |
|---|---|---|
| `GET /SyncPlay/List` | | `[GroupInfoDto]`: `GroupId`, `GroupName`, `State` (Idle, Waiting, Paused, Playing), `Participants` (user names), `LastUpdatedAt`. Only groups the user may join. It does not say what is playing. |
| `GET /SyncPlay/{id}` | | One group; 404 when gone. |
| `POST /SyncPlay/New` | `{GroupName}` | 12.4 answers 200 with the group (older: 204 and a `GroupJoined` socket message); NOCTURNE accepts both. Creating also joins. |
| `POST /SyncPlay/Join`, `/Leave` | `{GroupId}` / none | |
| `POST /SyncPlay/SetNewQueue` | `{PlayingQueue:[itemId], PlayingItemPosition, StartPositionTicks}` | Replaces the group's queue for everyone. |
| `POST /SyncPlay/SetPlaylistItem`, `NextItem`, `PreviousItem` | `{PlaylistItemId}` | Next/Previous carry the current entry so many clients asking at once advance the group once. |
| `POST /SyncPlay/Pause`, `Unpause`, `Stop` | none | |
| `POST /SyncPlay/Seek` | `{PositionTicks}` | |
| `POST /SyncPlay/Buffering`, `/Ready` | `{When, PositionTicks, IsPlaying, PlaylistItemId}` | `When` is server time (ISO 8601). |
| `POST /SyncPlay/Ping` | `{Ping}` | Integer milliseconds. Fractions were rejected in the past (jellyfin-web#8201). |
| `POST /SyncPlay/SetIgnoreWait` | `{IgnoreWait}` | Stop holding the group back. |
| `POST /SyncPlay/Queue`, `RemoveFromPlaylist`, `MovePlaylistItem`, `SetRepeatMode`, `SetShuffleMode` | see OpenAPI | Wrapped in the client crate; the app uses only the ones above. |
| `GET /GetUtcTime` | | `{RequestReceptionTime, ResponseTransmissionTime}`, no auth needed. |

Access: `UserPolicy.SyncPlayAccess` is `CreateAndJoinGroups`, `JoinGroups` or `None`. Every call also needs the
caller to have a WebSocket session (below).

## WebSocket

`GET /socket` with the same `Authorization: MediaBrowser ... Token=...` header (the web client uses `?api_key=`; the
header works too) upgrades to a WebSocket (12.4 also serves `/websocket`; NOCTURNE tries `/socket` first). The server sends
`{"MessageType":"ForceKeepAlive","Data":60}` and expects `{"MessageType":"KeepAlive"}` back. SyncPlay messages:

- `SyncPlayCommand`: `Data = {GroupId, PlaylistItemId, When, PositionTicks, Command (Unpause|Pause|Stop|Seek), EmittedAt}`.
  `When` is server time at which to act; the group is at `PositionTicks` then.
- `SyncPlayGroupUpdate`: `Data = {GroupId, Type, Data}` with `Type` = `UserJoined` / `UserLeft` (Data: user name),
  `GroupJoined` (Data: group info, includes the joiner), `GroupLeft`, `StateUpdate` (`{State, Reason}`), `PlayQueue`
  (`{Reason, LastUpdate, Playlist:[{ItemId, PlaylistItemId}], PlayingItemIndex, StartPositionTicks, IsPlaying, ...}`),
  `NotInGroup`, `GroupDoesNotExist`, `LibraryAccessDenied`.

Real captures from the throwaway server (see `crates/jellyfin-user/tests/syncplay_live.rs`) are the model for the unit-test
samples. Observed on 12.4: ids are 32 hex digits without dashes; the joiner gets a `Stop` command with an all-zero
playlist item right after joining; once the first member reports Ready the server answers `Pause` at the current position,
and when everyone is ready an `Unpause` with `When` about 1000 ms ahead (twice the default 500 ms ping); `UserJoined` is
sent to the others only; a lost socket ends the session and removes the member (zombie-group fix in 12.0).

## Clock sync

Same estimator as the web client: each `GetUtcTime` round trip gives `offset = ((recvAtServer - sent) + (sentByServer -
received)) / 2` (server minus local) and `delay = (received - sent) - (sentByServer - recvAtServer)`; the last 8 samples
are kept and the one with the smallest delay is used. Web: 3 greedy samples a second apart, then every 60 s. NOCTURNE: four
samples 0.4 s apart after connecting, then every 30 s, each followed by a `Ping` with `round(delay / 2)`. On the throwaway
server over loopback the measured offset was about 1 ms and the ping 2 ms.

## Command scheduling and drift correction

Constants and rules are the web client's (`PlaybackCore`):

- A command's local time is `When - offset`. `Unpause` in the future: seek first if more than 400 ms off, then unpause at
  that moment. Already past: unpause now at `PositionTicks + (serverNow - When)`. `Pause`: pause and seek to
  `PositionTicks`. `Seek`: pause, seek, then report Ready once mpv has finished seeking. `Stop`: close the player.
- A duplicate command is ignored while scheduled; once due it is re-applied only if the local player disagrees. Commands for
  another playlist item are ignored (except Stop), as are commands emitted before we joined. A command that arrives before
  the first clock sample waits for it.
- Drift correction starts 1.5 s after an unpause and checks every 1.5 s. Group position minus ours:
  below 60 ms nothing; 60 ms to 3 s **SpeedToSync** (mpv `speed = 1 + diff / 1000 ms` for 1 s, longer when we are far ahead
  so the speed never drops below 0.2); 400 ms and more outside that range **SkipToSync** (seek). The web client ships with
  correction switched off by default; NOCTURNE has it on. Speed changes are local (the server does not synchronise them).

## Buffering and Ready handshake

The server (12.x) waits for **every** member's `Ready` after a new queue, a seek or an unpause, then starts the group; if one
member stays silent it holds the group (`Pause`) and gives up waiting after about 30 s (jellyfin#18121, jellyfin#17351).
Verified live: with one silent member the group received `Pause` at 0 s and an `Unpause` at 30.9 s (ignored test
`group_waits_for_the_silent_member_then_times_out`), so the group does not stay stuck, but everyone waits half a minute.
NOCTURNE therefore answers promptly:

- after a queue item is loaded: pause at the group's position, then `Ready` (position, `IsPlaying: false`, playlist item);
- after every `Seek`: `Ready` with the **post-seek** position (a stale position made the server schedule a pause 1804 s
  ahead, jellyfin#17797);
- after every applied `Unpause`: `Ready` again even when already ready;
- when mpv stalls for the cache for 0.4 s: `Buffering`; when it recovers: `Ready`;
- a member who closes the video stays in the group but sends `SetIgnoreWait(true)` so the others are not held back; "Play
  what the group is on" sends `SetIgnoreWait(false)` and loads the current item at the group's position.

Positions are clamped to the item's runtime. Nobody is treated as the host (`Participants[0]` means nothing; a host field is only
proposed upstream). One session per device id: sessions are keyed by app and device id, so `NOCTURNE_DEVICE_ID` overrides the
per-machine id (two accounts at once, tests).

## Late joiners, leaving, errors

- A joiner receives the queue (`PlayQueue`, `NewPlaylist`); the start position is the last command's position advanced by
  the time since it was emitted, else the queue's `StartPositionTicks` (advanced if the queue says playing).
- Leaving (pill or `L` on the screen) keeps the video running locally with normal controls; it works with server changes off.
- Turning server changes off while in a group leaves the group.
- Group gone / `NotInGroup`: local state is reset with a notice. `LibraryAccessDenied` or an item the user cannot load: a
  notice, `SetIgnoreWait(true)`, the user stays in the group without holding it back.
- Socket lost: the pill shows "reconnecting"; the socket reconnects (1, 2, 4, 8, then 15 s). On reconnect NOCTURNE asks
  `GET /SyncPlay/{id}`: still a member means leave and join again (so the server resends queue and position); otherwise the
  group state is dropped with a notice. After 45 s without a socket the group is dropped locally (the server drops the session
  at about 60 s).

## Writes

Create, join, SetNewQueue, transport requests, Ready, Buffering, Ping, SetIgnoreWait and Leave change server state. Create,
join and starting something for the group need **Allow NOCTURNE to change data on the server**; the rest only happen inside a
group, which needs the switch. Developer API-key mode has no login, so the screen only explains that it is read-only. Leave is
always allowed. The list, the group, `GetUtcTime` and the policy are reads. Capabilities (`/Sessions/Capabilities`) are not
posted: SyncPlay works without them (verified: two sessions that never posted capabilities took part in groups).

## Supported players

Embedded libmpv and the external mpv child process both work, since everything goes through the shared `Playback` seam
(pause, seek, speed, `seeking` / `paused-for-cache` properties). Groups can mix them.

## Limits

- The group list shows state and member names only: the API does not expose what a group is playing.
- A group plays its queue from the start; resume positions are not used.
- Speed, A-B loop, repeat, shuffle and queue editing are disabled while in a group (the group's queue is shared; repeat and
  shuffle modes of the group are not exposed yet). Subtitles, audio track, volume and aspect stay per member.
- Auto-skip intro is off in a group. The server-side group persists only while someone is in it.
- Transcoded members drift more (web issue jellyfin-web#6210); drift correction compensates but a transcode is not seeked
  precisely.

## What was verified live

See [QA_REPORT.md](QA_REPORT.md).
