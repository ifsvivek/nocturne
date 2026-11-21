# Themes gallery

Every folder here is one theme in the gallery on Settings > Themes (tabs Official and Community). `scripts/themes-index.py` checks the folders and writes `website/static/themes/index.json` and a copy of the files, which the website serves; the app downloads from there. A theme from the gallery is saved like any other theme, so it can be edited, applied per server or deleted. Themes you paste, import from a file or load from a web address keep working and are shown as not from the gallery.

## Folder layout

```
themes/<id>/
  theme.css      the stylesheet, at most 256 KB, no secrets, nothing that needs a server
  theme.toml     name, author, licence, description, kind, min_version
  preview.webp   1280 px wide, under 150 KB
```

`<id>` is lowercase letters, digits and dashes. `theme.toml`:

```toml
name = "My theme"
author = "Your name or handle"
licence = "MIT"
description = "One sentence on what it looks like."
kind = "community"
min_version = "0.5.2"
```

`kind = "official"` is for themes the NOCTURNE project maintains; yours is `community`. `min_version` is the oldest NOCTURNE that can show the theme.

## Submitting a community theme

1. Open a pull request that adds one folder under `themes/`.
2. The licence must be an SPDX identifier that is compatible with GPL-3.0-or-later (the script lists them: for example MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, MPL-2.0, CC0-1.0, GPL-3.0-or-later). You must be the author or hold a licence that lets the project redistribute it; keep any credit the licence requires in the CSS header comment. Do not copy a theme you do not have the right to share.
3. The preview must come from demo data (`scripts/make-demo-library.sh`), never a personal library, personal titles, server addresses or a desktop with other windows. Capture the app window only.
4. Keep the CSS self-contained. `@import` of other sites and `url()` pictures are fetched only when the user has remote styles on, and a gallery theme should not depend on them.
5. Run `scripts/themes-index.py` and commit the generated `website/static/themes/` too (`scripts/themes-index.py --check` must pass).

The maintainer reads the CSS, tries the theme and merges. The gallery is a curated list, not a gate: nobody needs to be in it to use a theme.
