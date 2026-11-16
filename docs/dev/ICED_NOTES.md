# Iced 0.14 notes for the port (verified against the 0.14 sources)

Paths are under `~/.cargo/registry/src/*/`. Check the source before using anything not listed here; do not use
0.12/0.13 APIs from memory.

## Video embedding (spike `spikes/iced-embed`, result: GO on Wayland)

- `EmbeddedPlayer` subsurface below a `transparent: true` Iced window works; Iced draws controls on top; resize
  stays aligned; clean close (`drop(player)` then `window::close(id)`). Default wgpu backend (Vulkan) composites
  correctly; `WGPU_BACKEND=gl` darkens the video, so never force GL.
- Wayland pins in `crates/player` (wayland-client 0.31.15, -backend 0.3.17, -egl 0.32.11, -protocols 0.32.13) match
  what iced 0.14 / winit 0.30.13 resolve to. wgpu 27.
- Raw handles: `window::run(id, |w: &dyn Window| ...)` (`iced_runtime/src/window.rs:196`); `w.window_handle()` /
  `w.display_handle()` -> `RawWindowHandle::Wayland(s)` (`s.surface`) and `RawDisplayHandle::Wayland(d)`
  (`d.display`). Pass pointers out as `usize`. On X11/XWayland the handle is `Xlib`/`Xcb` -> external `Player`.
- Window id: `window::oldest()` in the boot task. Scale: `window::scale_factor(id)`.
- Transparency: `window::Settings { transparent: true, .. }` + app `.style(|_, _| iced::theme::Style {
  background_color: Color::TRANSPARENT, text_color })`. Every non-player screen paints its own opaque background.
  wgpu prefers PostMultiplied then PreMultiplied alpha (`iced_wgpu/src/window/compositor.rs:127`).
- Video bounds: a tiny custom `advanced::Widget` ("Hole") that draws nothing and, in `update`, on
  `Event::Window(window::Event::RedrawRequested(_))` publishes `layout.bounds()` when it changed. `update` then
  calls `set_bounds(x, y, w, h, scale)` (Iced logical coords == subsurface logical coords). Never draw Iced panels
  underneath the hole. (`iced_runtime/src/widget/selector.rs` `find` is an untested alternative.)

## Application

- `iced::application(boot, update, view)` (`iced/src/application.rs:79`); builder: `.title`, `.subscription`,
  `.theme`, `.style`, `.scale_factor`, `.font(bytes)`, `.default_font`, `.antialiasing`, `.window(Settings)`,
  `.window_size`, `.exit_on_close_request(bool)`, `.executor::<E>()`, `.run()`.
- Features off by default that we need: `tokio`, `image`, `svg`, `canvas`, `lazy`, `advanced`.
- Window settings: `size, min_size, transparent, decorations, icon, platform_specific.application_id,
  exit_on_close_request`. Tasks in `iced_runtime/src/window.rs`: `close_requests()`, `resize_events()`,
  `frames()`, `set_mode` (fullscreen), `close`, `drag`, `screenshot`. `iced::exit()`.
- `Task`: `none, done, perform(fut, f), run(stream, f), batch, map, then, chain, discard, abortable() -> (Task,
  Handle)`; `Handle::abort / abort_on_drop`.
- Subscriptions: `Subscription::run(fn)`, `run_with(data, fn)`, `batch`, `map`, `filter_map`;
  `event::listen_with(fn(Event, Status, window::Id) -> Option<M>)` takes fn pointers, not closures;
  `keyboard::listen()` yields `keyboard::Event` (**no `keyboard::on_key_press` in 0.14**);
  `iced::time::every(Duration)` (needs `tokio`); `window::frames()` for animation ticks.
- `Widget` trait in 0.14: `update(tree, event, layout, cursor, renderer, clipboard, shell, viewport)` replaces
  `on_event`; `draw` takes a viewport.

## Widgets

- `image`: `Handle::from_bytes / from_rgba(w, h, px) / from_path`; `.content_fit .filter_method .opacity
  .border_radius .crop .scale`.
- `scrollable`: `.direction`, `.horizontal()`, `.id`, `.on_scroll(Fn(Viewport))`, `.spacing`, `.anchor`;
  `Viewport::{absolute_offset, relative_offset, bounds, content_bounds}`; tasks `scroll_to(id, AbsoluteOffset)`,
  `snap_to(id, RelativeOffset)`, `snap_to_end` (`iced_runtime/src/widget/operation.rs`).
- Focus tasks: `focus(id)`, `focus_next()`, `focus_previous()`, `select_all`, `move_cursor_to_end`.
  `widget::Id::unique()` or `From<&'static str>` / `From<String>` (no `Id::new`).
- Helpers (`iced_widget/src/helpers.rs`): `lazy`, `responsive`, `stack`, `mouse_area` (`on_press, on_release,
  on_double_click, on_right_press, on_scroll, on_enter, on_move(Point), on_exit`), `tooltip`, `float`, `pin`,
  `opaque`, `center`, `hover`, `sensor`, `themer`, `canvas`, `shader`, `rich_text` + `span`, `space()`, `grid`,
  `keyed_column`, `combo_box`, `pick_list`, `slider`, `progress_bar`, `svg`.
- Container style: `text_color, background, border, shadow, snap`; gradients via
  `Background::Gradient(Gradient::Linear(gradient::Linear::new(angle).add_stop(..)))`.
- `iced::animation::Animation`: `new(state)`, `.easing`, `.quick/.slow/.duration/.delay`, `.go(v, now)`,
  `.is_animating(now)`, `.interpolate(a, b, now)`, `.value()`. Drive redraws with `window::frames()` only while
  something animates.
- `Theme::custom(name, palette)`.

## Added in M3

- `widget::Id` has no `new`: `Id::from("name")` (a `&'static str`). `Task::run(stream, f)` takes any `Stream`, so a
  blocking thread reports progress through `iced::futures::channel::mpsc::unbounded`. `gen` is a reserved word in
  edition 2024 (name things `run_id`).
- `keyboard::listen()` only yields **ignored** events: a focused `text_input` keeps its typed keys (and Esc once) away from the
  root, so the global shortcuts need no "typing" flag.
- `scrollable` with `Direction::Horizontal(Scrollbar::new().width(0).scroller_width(0))` is a link row that scrolls
  with the wheel and shows no bar (header nav).
- Folder chooser: `rfd` 0.17 with only `xdg-portal` (MIT; pulls `pollster`, `log`, `percent-encoding`; no GTK). It
  `dlopen`s `libdbus-1` for the portal call and falls back to `zenity`; neither present returns `None` (the typed path
  field still works). Add `libdbus-1` to the optional runtime list at the cut-over.
