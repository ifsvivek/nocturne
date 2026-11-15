# Design

The source of truth is `crates/app/src/theme.rs`; raw hex values live there and nowhere else. This page describes
what it implements. Dark only.

Principle: near-black base, hairline structure, one electric-blue accent, monochrome chrome. Library artwork is the
only rich colour.

## Palette

| Token | Value | Use |
|---|---|---|
| frame | `#07080c` | window background |
| surface | `#0c0e16` | panels |
| raised | `#12151f` | hover and raised surfaces |
| hairline / strong | white 8% / 16% | borders |
| glass | white 4% | faint fill |
| text / secondary / tertiary | `#eceef5` / `#8a90a6` / `#666c82` | copy hierarchy |
| accent | `#4c8dff` (hover `#6ba0ff`, text `#9cc0ff`) | the single accent; tint 12%, glow 35% |
| on-accent | `#050816` | text on accent fills |
| danger | `#f0716f` | destructive confirmations and errors only |
| warn | `#e3b341` | warnings only |

Danger and warn are never decoration.

## Typography

- Sans: Noto Sans (system font; not bundled)
- Mono: JetBrainsMono Nerd Font, for micro labels and annotations
- Scale (px): micro 11 (mono annotations only), caption 12, body 15, body-large 16, heading 22, display 56.
  Everything except mono micro labels is at least 12.

## Spacing and shape

4 px scale (4, 8, 12, 16, 24, 32), page padding 48, header height 56. Radii: 10 for panels, 3 for small elements;
cells are nearly square.

## Motion

Short, ease-out, never gratuitous.

- Hover: 140 ms
- Base transitions: 220 ms
- Slow (page-level): 300 ms

Motion explains state changes (hover, focus, overlays); nothing loops or animates for decoration. Loading uses
blurhash placeholders and skeletons rather than spinners where content shape is known.

## Accessibility

Keyboard-first navigation (Ctrl+K / `/` search, Esc / Backspace back, player shortcuts with an in-player help
overlay), minimum 12 px text outside mono micro labels, and visible focus. Screen-reader labels are limited by what
GPUI exposes.
