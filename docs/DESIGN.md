# Design

The source of truth is `crates/app/src/theme.rs` (Iced colour constants, sizes and motion durations); raw hex values
live there and nowhere else. This page describes what it implements. Dark only.

Principle: near-black base, hairline structure, one electric-blue accent, monochrome chrome. Library artwork is the
only rich colour.

## Palette

| Token | Value | Use |
|---|---|---|
| frame | `#07080c` | window background |
| surface | `#0c0e16` | panels |
| raised | `#12151f` | hover and raised surfaces |
| hairline / strong | white 8% / 16% | decorative dividers and panel outlines only |
| edge | `#747a91` | outline of an interactive control (chip, button, switch track, input, avatar button) |
| control / hover | `#1a1e2c` / `#232838` | filled value controls |
| glass | white 4% | faint fill |
| text / secondary / tertiary | `#eceef5` / `#a4aabf` / `#8990a9` | copy hierarchy |
| accent | `#4c8dff` (hover `#6ba0ff`, text `#9cc0ff`) | the single accent; tint 12%, glow 35% |
| on-accent | `#050816` | text on accent fills |
| danger | `#f0716f` | destructive confirmations and errors only |
| warn | `#e3b341` | warnings only |

Danger and warn are never decoration.

## Contrast

WCAG 2.x, enforced by `theme::tests` (`cargo test -p nocturne-app theme`). Every text token must reach 4.5:1 on every
surface it can sit on: frame, surface and raised, bare and under the glass (hover), accent tint (selected) and danger
tint, plus the control fills. Control outlines (`edge`), the accent focus ring and danger outlines must reach 3:1 on the
same set. Semi-transparent fills are composited over the real surface before measuring. Disabled controls are exempt
and are dimmed (opacity 0.55) rather than recoloured. Hairlines are decoration, never the only boundary of a control.

| Token | Before (frame ... raised, bare) | After (all surfaces) |
|---|---|---|
| text `#eceef5` | 15.7 - 17.3 | 12.7 - 17.3 |
| secondary | 5.7 - 6.3 | 6.3 - 8.7 |
| tertiary | 3.5 - 3.8 (failed) | 4.6 - 6.3 |
| accent `#4c8dff` | 5.7 - 6.3 | 4.6 - 6.3 |
| edge (new; hairline strong was 1.5 - 1.6) | - | 3.4 - 4.7 |
| on-accent on accent | 6.2 | 6.2 |

Run `cargo test -p nocturne-app theme -- --nocapture` to print the current ranges.

Do not hard-code text colours outside `theme.rs`; new text colours must be added to the audit list in the test.

## Controls

- Value controls (Settings chips) are 32 px tall (`CONTROL_H`), 13 px sans medium text (`CONTROL_TEXT`), 12 px side
  padding, `control` fill and a 1 px `edge` outline, with a caret on drop-downs (flipped while open).
- States: hover (lighter fill, outline to secondary text), keyboard focus (accent outline and glow), pressed (accent
  tint), open (accent tint and outline, caret up).
- On / off settings are a switch: 40x22 track and knob (on = accent track, knob right) with the word ON or OFF beside
  it, so state is never colour alone.
- A selected Settings row has the accent tint and a 3 px accent bar; hover is the glass fill. Row labels are 15 px
  semibold, descriptions 13 px secondary, section captions 13 px bold tracked caps.
- Buttons, inputs and avatar buttons use `edge` for their outline.

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

Motion explains state changes (hover, focus, overlays); nothing loops or animates for decoration, apart from the loading
splash, which breathes gently while a stream is stalled so it never looks frozen. Loading uses blurhash placeholders and
skeletons rather than spinners where content shape is known.

Iced specifics: buttons are the `Press` widget (`widget/press.rs`), which cross-fades from its normal to its hovered look
over the hover duration with `iced::animation::Animation` and an ease-out curve; overlays and menus use `Popover`. A widget
asks for the next frame only while its animation runs, and the window subscribes to `window::frames()` only while
something animates, so an idle window draws nothing.

**Reduce motion** (Settings; System / On / Off): by default it follows the desktop's reduced-motion preference. When on,
every animation duration resolves to zero through `motion::dur`, so state changes are instant and no frames are requested
for animation. Colours and layout are identical either way.

## Accessibility

Keyboard-first navigation (Ctrl+K / `/` search, Esc / Backspace back, player shortcuts with an in-player help
overlay), minimum 12 px text outside mono micro labels, and visible focus. Screen-reader labels are limited by what
the UI framework exposes (Iced's accessibility support is still limited); a re-evaluation is planned.
