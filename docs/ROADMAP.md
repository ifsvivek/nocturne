# Roadmap

Honest list of what is missing in 0.1.0 and what is intentionally out of scope. Implemented features are in
[FEATURES.md](FEATURES.md); verification gaps are in [QA_REPORT.md](QA_REPORT.md). No dates are promised.

## Known gaps

**Admin**
- Editable Branding and Networking (view only today)
- Scheduled task triggers (add, edit, delete)
- Plugin install, enable, disable and repositories
- Library add, remove and edit (paths, type, metadata providers, image fetchers, schedule)
- Trickplay settings editing
- User policy details: library access, parental rating, feature and bitrate limits, profile image

**Media types and playback**
- Books and audiobooks are not playable (detail page only)
- Music player depth: queue and now-playing UI, album-art views
- Playlist and collection create, add, remove and reorder
- Shuffle and repeat across a queue

**Accounts and settings**
- User picker / profile switching, display language, own profile image
- Theme and accent choice, subtitle appearance presets
- Quick Connect authorisation of other devices, list of own sessions

**Platform**
- Packages are tested on Arch Linux only; other distributions, Flatpak and AppImage are unverified or not provided
- Screen-reader support is limited by GPUI
- Windows and macOS: possible future work, not planned or supported

**Verification still open**
- Several newer player features on a real server (resume prompt, A-B loop, audio device list, external subtitles,
  subtitle presets), multi-season shows, 401 mid-session and the offline banner against a real server
- Cache size cap on a very large library; multi-day soak

## Dropped (will not be built)

- Live TV and DVR
- SyncPlay
- DLNA setup
- Acting as a cast target
- Gapless music playback
