# What NOCTURNE changes on your server

By default NOCTURNE only reads. One setting, **Settings > Allow NOCTURNE to change data on the
server** (`allow_server_writes`, default off), gates every change. Turn it on and the features below
work, each with its existing confirmation dialog where one exists. Developer API-key mode
(`NOCTURNE_JF_TOKEN`) refuses all writes whatever the setting says.

| Feature | Needs server changes on | Also needs |
|---|---|---|
| Browse, search, details, playback, resume position | no | |
| Report playback progress (Continue Watching follows you) | yes | **Sync watch progress** on |
| Mark watched / unwatched (card chip, detail button, `w`) | yes | |
| Favourite (card chip, detail button, player heart, `f`) | yes | |
| Change own password | yes | the current password |
| Own profile image (set, remove), display language | yes | a confirmation dialog |
| Playlists you own: create, add items, remove, reorder, rename, delete | yes | a confirmation dialog; delete needs the typed name |
| Authorize another device with a Quick Connect code | yes | the code shown on that device |
| Admin: scan library, run/stop tasks, users, sessions, server settings, transcoding settings, restart, API keys | yes | an administrator account and a confirmation dialog per action |
| Admin: branding, task triggers, trickplay settings, libraries (add, remove, folders, options), plugins (enable, disable, uninstall, install) | yes | an administrator account and a confirmation dialog per action |
| Admin: plugin repositories (add, enable, disable, remove), library rename, metadata / image fetcher order and enabling, local readers, per-user access and limits, user profile image (upload, remove) | yes | an administrator account and a confirmation dialog per action |
| Admin: networking (ports, bind addresses, LAN networks, remote access, UPnP, discovery, IPv4 / IPv6) | yes | an administrator account, a confirmation dialog and the typed word NETWORK; wrong values can lock everyone out |
| Admin: dashboards, activity, devices, logs, libraries (with display/NFO/trickplay flags), plugins and the plugin catalog, backups list, transcoding, trickplay and networking views, own sessions list | no (view only) | an administrator account |

With the switch off, the controls above stay visible but dimmed with the tooltip "Turn on server
changes in Settings", the keyboard shortcuts show a short notice, and the Admin section is view
only with a banner linking to the setting.

Signing in and out is separate: it creates and ends this app's own session on the server, which any
client must do to authenticate. Switching between remembered accounts only uses stored sign-ins (it reads `/Users/Me` once) and works with server changes off. The music queue never leaves the app. Collections are read only in NOCTURNE.

Settings files written before this switch existed (version 1) had progress sync on by default;
on first load that value is reset to off and the master switch starts off.
