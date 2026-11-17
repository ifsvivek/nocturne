# What NOCTURNE changes on your server

By default NOCTURNE only reads. One setting, **Settings > Allow NOCTURNE to change data on the
server** (`allow_server_writes`, default off), gates every change. Turn it on and the features below
work, each with its existing confirmation dialog where one exists. With several servers, every change goes only to the server
the item came from (its **origin**) and uses that server's own account; no change is ever sent to another server. Developer API-key mode
(`NOCTURNE_JF_TOKEN`) refuses all writes whatever the setting says (it also cannot take part in a group).

| Feature | Needs server changes on | Also needs |
|---|---|---|
| Browse, search, details, playback, resume position | no | |
| Report playback progress (Continue Watching follows you) | yes | **Sync watch progress** on |
| Mark watched / unwatched (card chip, detail button, `w`) | yes | |
| Same watched state on the title's copies on other servers (`sync_watched`, default off) | yes | **Keep watched status in step across sources** on. Each copy is written only on its own server with that server's own account; resume positions of copies also need **Sync watch progress**. Local copies follow without the master switch. Details below |
| Trakt: scrobble, history import (`trakt_scrobble`, `trakt_history`, default off) | no for Trakt itself; yes to mark imported items watched on a server | your own Trakt app, connected in its dialog. Nothing is written to a server unless server changes are on; imported state is never copied on to other sources |
| Favourite (card chip, detail button, player heart, `f`) | yes | |
| Change own password | yes | the current password |
| Own profile image (set, remove), display language | yes | a confirmation dialog |
| Playlists you own: create, add items, remove, reorder, rename, delete | yes | a confirmation dialog; delete needs the typed name |
| Authorize another device with a Quick Connect code | yes | the code shown on that device |
| Watch together (SyncPlay): create a group, join one, start something for the group, play / pause / seek / skip, Ready and Buffering reports, latency Ping | yes | an explicit click or key (create, join); the rest only happens while in a group. Leaving is always allowed. Details in [syncplay.md](syncplay.md) |
| Remote control from other Jellyfin apps (control this computer's playback) | yes | **Allow remote control from other devices** on (default off). A session-level call: it posts this app's capabilities (`POST /Sessions/Capabilities/Full`) and listens on the session WebSocket; it changes no library data. Details below |
| Admin: scan library, run/stop tasks, users, sessions, server settings, transcoding settings, restart, API keys | yes | an administrator account and a confirmation dialog per action |
| Admin: branding, task triggers, trickplay settings, libraries (add, remove, folders, options), plugins (enable, disable, uninstall, install) | yes | an administrator account and a confirmation dialog per action |
| Admin: plugin repositories (add, enable, disable, remove), library rename, metadata / image fetcher order and enabling, local readers, per-user access and limits, user profile image (upload, remove) | yes | an administrator account and a confirmation dialog per action |
| Admin: networking (ports, bind addresses, LAN networks, remote access, UPnP, discovery, IPv4 / IPv6) | yes | an administrator account, a confirmation dialog and the typed word NETWORK; wrong values can lock everyone out |
| Admin: dashboards, activity, devices, logs, libraries (with display/NFO/trickplay flags), plugins and the plugin catalog, backups list, transcoding, trickplay and networking views, own sessions list | no (view only) | an administrator account |
| Offline copies: "Download to this computer" (GET `Items/{id}/Download`, only reads the server) | no | writes only new files into the local folder you choose, never overwrites; needs the server to allow downloads for your account |
| Local library: mark watched, resume position, manual match, source preference, folders and sources list | no | nothing leaves your computer: stored in NOCTURNE's own files (`$XDG_DATA_HOME/nocturne/local/`, `sources.toml`) |

**Remote control** (`remote_control`, default off; needs server changes on; never in developer API-key mode). When on, each
signed-in server gets `POST /Sessions/Capabilities/Full` (`SupportsMediaControl`, playable types Video and Audio, the
commands below), re-posted whenever the socket reconnects, and one WebSocket per server (separate from SyncPlay's). Other
Jellyfin apps of the same account then see this computer as a device. They can send: Play (PlayNow / PlayNext / PlayLast
with item ids, resolved through that server's own catalog), Playstate (PlayPause, Pause, Unpause, Stop, Seek, NextTrack,
PreviousTrack, Rewind, FastForward) and GeneralCommand (SetVolume, Mute, Unmute, ToggleMute, SetAudioStreamIndex,
SetSubtitleStreamIndex, DisplayMessage shown as a notice). Everything else is ignored. A command only drives the player: it
never writes, never runs an admin action and never answers a confirmation dialog. Switching the setting off posts
"not controllable" again (when server changes are on), closes the socket and ignores commands. With it off nothing is posted at
all: the app does not announce itself as controllable.

**Watched state in step** (`sync_watched`). Only a user action starts it: marking watched / unwatched, or finishing an item
(90% of its length, as Jellyfin counts). The title's other copies are looked up by strong identity only (a shared IMDb /
TMDB / TVDB id; an episode by its series' ids plus season and number); "possible duplicates" that share only a title and
year are never touched. A server copy is written only with server changes on and a real login (never in developer API-key
mode, skipped silently otherwise), through that server's own hooks; a local copy is written to the local state file. The
setters it calls never start another cascade, and state that arrives from elsewhere (the Trakt import) is applied without
spreading. Resume positions follow only with Sync watch progress on, scaled by runtime when the copies differ in length.

Local media files are never modified, moved, renamed or deleted by NOCTURNE, whatever the setting says; scanning only reads.

With the switch off, the controls above stay visible but dimmed with the tooltip "Turn on server
changes in Settings", the keyboard shortcuts show a short notice, and the Admin section is view
only with a banner linking to the setting.

Signing in and out is separate: it creates and ends this app's own session on the server, which any
client must do to authenticate. Switching between remembered accounts only uses stored sign-ins (it reads `/Users/Me` once) and works with server changes off. The music queue never leaves the app. Collections are read only in NOCTURNE.

Settings files written before this switch existed (version 1) had progress sync on by default;
on first load that value is reset to off and the master switch starts off.
