# NOCTURNE v0.5 brief (maintainer's requirements, recorded 2026-11-17)

Status: **not started.** Do not compromise the v0.4 release or start v0.5 implementation until v0.4 is complete and the maintainer approves moving forward. Open question recorded earlier: what exactly "Server UI" covers is to be settled with the maintainer when v0.5 starts (see the investigation below).

## 1. Dual native UI modes (the defining feature of v0.5)

Two interface modes, selectable independently for each connected Jellyfin server.

**NOCTURNE UI**: our existing native interface: NOCTURNE's design, navigation, layouts and customization; the unified NOCTURNE experience.

**Server UI**: recreate the connected Jellyfin server's interface natively in Rust + Iced.
- Reflect the server's branding, layout, navigation, home sections, ordering, library presentation and per-user display preferences wherever the server exposes this information.
- Changes to supported user preferences on the Jellyfin server are reflected in NOCTURNE.
- Each server can have its own interface configuration.

**Non-negotiable**
- Absolutely NO WebView, embedded browser, Chromium, HTML/CSS rendering engine or browser-based UI. Everything is rendered natively through Iced. Never display a web page inside the app.
- Preserve NOCTURNE's native playback engine and existing functionality.
- Server UI mode is isolated to its own server. Local media always uses NOCTURNE UI.

**Investigation required before implementation**
- jellyfin-web is HTML/CSS/JS. Investigate which server-side APIs and user settings (e.g. `DisplayPreferences`, home section configuration, library order, branding/`Branding/Configuration`, per-library view settings) can be reproduced natively.
- Do not pretend arbitrary Jellyfin custom CSS or JavaScript can be reproduced. Identify unsupported features, propose native equivalents, document limitations. No WebView as a workaround.
- Investigate the Jellyfin API, NOCTURNE's architecture and Iced's rendering capabilities; write a technical plan and build a small working prototype that validates the approach.

## 2. Native custom CSS compatibility (core of v0.5 customization)

- Users can paste CSS or import `.css` files, including CSS themes made for Jellyfin.
- Parse the CSS and translate supported rules into NOCTURNE's native Iced styling: backgrounds, colours, typography, spacing, borders, radii and supported layout properties. Support CSS variables and common selectors where practical.
- Live preview before applying. Save, switch, edit and reset themes. Themes per server, plus an optional global theme.

**Constraints**
- No WebView, browser engine, HTML renderer or runtime CSS rendering. Build a CSS parser and a controlled CSS-to-native-style translation layer.
- Unsupported selectors, pseudo-elements, animations and properties produce clear warnings, never silent failure.
- Never execute JavaScript or any embedded code. Sanitize and validate imported stylesheets.
- Preserve native rendering and performance.
- Investigate how much existing Jellyfin custom CSS (popular community themes) can realistically be translated; design the styling architecture so it can grow later, without claiming full browser CSS compatibility.

## Related earlier wishes (requests log #35)

UI refresh and customization comparable to Jellyfin's display settings: themes/accent colours, home sections (choose, reorder, per library), density and UI scale, backdrops/theme music, per-library default views, date/time formats, possibly layout choices. To be merged into the v0.5 design with the maintainer.

## 3. Updates (maintainer decision 2026-11-18)

A signed in-app updater for per-user installs (Windows per-user installer, macOS app, a new per-user Linux install mode under `~/.local`): checks once a day, downloads the new version, verifies its signature with NOCTURNE's signing key, swaps it in and offers "Restart to update" without a password. Installs managed by pacman, apt or dnf never modify themselves: they show "Update available" with the exact command or a button that opens the software centre. A setting turns update checks off; a check sends only the current version (and, unavoidably, the IP address).

Also for 0.5: Linux becomes Wayland-only (X11 code and dependencies removed, a clear message on X11 sessions); every text field supports selection, copy, cut and paste (needed for pasting CSS and CSS URLs); NOCTURNE's Trakt client id ships built in.
