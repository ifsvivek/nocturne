# Roadmap

Honest list of what is missing in 0.1.0 and what is intentionally out of scope. Implemented features are in
[FEATURES.md](FEATURES.md); verification gaps are in [QA_REPORT.md](QA_REPORT.md). No dates are promised.

## Known gaps

**Admin**
- Networking: base URL, HTTPS and certificates, proxies and IP filters stay view only on purpose
- Per-user access schedules, "block unrated" kinds and per-folder deletion rights; subtitle, lyric and segment provider ordering (the throwaway server has none installed)

**Media types and playback**
- Books and audiobooks are not playable (detail page only)
- Music: album-art views, a persistent queue across launches, queue editing while browsing (the player owns the screen)
- Collection create, add, remove and reorder; sharing playlists with other users

**Accounts and settings**
- A user picker on the sign-in screen (switching is in the account menu)
- Theme and accent choice, subtitle appearance presets
- Ending other sessions of your own account (the server only allows it for administrators)

**Platform**
- Packages are tested on Arch Linux only; other distributions, Flatpak and AppImage are unverified or not provided
- Screen-reader support is limited by GPUI
- Windows and macOS: possible future work, not planned or supported

**Verification still open**
- Several newer player features on a real server (resume prompt, A-B loop, audio device list, external subtitles,
  subtitle presets), multi-season shows and the photo viewer on a real library (both checked on generated
  throwaway data), 401 mid-session and the offline banner against a real server
- Cache size cap on a very large library; multi-day soak

## Dropped (will not be built)

- Live TV and DVR
- SyncPlay
- DLNA setup
- Acting as a cast target
- Gapless music playback
