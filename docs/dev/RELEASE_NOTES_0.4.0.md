## Highlights

- **Playback depth.** A secondary subtitle on screen with the main one, audio and subtitle languages remembered per show across restarts (including "Original audio"), and player menus reachable by keyboard and gamepad.
- **HDR videos: you choose.** Play in the window in SDR, or open an HDR pop-out window that the app keeps controlling. Real HDR output needs an HDR display, a compositor with colour management and mpv 0.42 or newer.
- **Your local media, filled in.** Optional online details from TMDB (your own key), TVmaze and MusicBrainz, and optional subtitle search and download from OpenSubtitles (your own key). Off by default; only titles, years and ids are sent, never file names or paths.
- **Offline copies.** "Download to this computer" saves a server's movie, episode or whole show into a local folder; the copy appears as a second source of the same title. Resumable, never overwrites.
- **Smart collections** over all your sources, such as "Unwatched 4K movies" or "Added this week".
- **Across services (all opt-in):** one watched status across your sources, Trakt scrobbling and history import (your own Trakt app), and remote control from other Jellyfin apps.
- **Couch mode** for a TV: a larger layout, strong focus and full gamepad or remote control.
- **Server details:** truthful playback reports (direct play vs transcode), a Latest row with new episodes, Play on a show starts the next episode, "Included in" collections, and an optional extra HTTP header per server for reverse proxies.
- **Packages:** native `.deb` (Debian 13, Ubuntu 24.04+) and `.rpm` (Fedora, with RPM Fusion) next to the Arch repository and the tarball.

## Upgrading

- From the NOCTURNE pacman repository: `sudo pacman -Syu`. Settings, sources and sign-ins carry over.
- Nothing changes on your servers unless you allow it; every new online service is off until you turn it on.

## Known limitations

- HDR output has not been tried on an HDR display yet; couch mode has not been tried with a real gamepad or TV.
- macOS and Windows: preview builds only (built by CI, never run on a real Mac or PC), video in mpv's own window there.
- No AppImage (it would be 107 MB and clash over libmpv / glibc); Flatpak is being evaluated.
- The .deb and .rpm were install-tested in clean containers, not on desktops; there is no apt or dnf repository yet.

Full list: [CHANGELOG](https://github.com/ifsvivek/nocturne/blob/v0.4.0/CHANGELOG.md) · verification per feature: [FEATURES](https://github.com/ifsvivek/nocturne/blob/v0.4.0/docs/FEATURES.md)

TMDB: This product uses the TMDB API but is not endorsed or certified by TMDB. TVmaze data is licensed CC BY-SA 4.0. Trakt is a trademark of Trakt, Inc.; NOCTURNE is not affiliated with or endorsed by Trakt.

> Unofficial. Not affiliated with, endorsed by or sponsored by the Jellyfin project.
