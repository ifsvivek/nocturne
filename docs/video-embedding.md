# In-window video: how it works and why

Playing video inside an Iced window needs a surface the UI framework does not provide. This page records the design, the measurements that justified it and the rules that must not be broken. It was first built and measured under GPUI (0.2.0); the mechanism carried over to Iced unchanged after a spike (`spikes/iced-embed`, [DECISIONS.md](DECISIONS.md)).

Test environment (one hybrid-graphics machine; other hardware is untested): Arch Linux, a Wayland compositor, NVIDIA discrete GPU (proprietary driver) plus an AMD iGPU, high-refresh monitor at scale 1.0, mpv / libmpv 0.41.

## The design

libmpv renders with its **OpenGL render API through EGL** into a `wl_subsurface` placed **below** a transparent Iced window. The player page leaves a transparent hole (the `Hole` widget) and draws its controls over it as ordinary widgets.

1. **Handles.** `window::run` gives the raw `wl_display*` and `wl_surface*` of the Iced window (`crates/app/src/platform.rs`). The embedded player is created from them on the event-loop thread.
2. **Own event queue.** `crates/player/src/embedded.rs` wraps the existing `wl_display` (never owned or closed by us) with its own queue, binds `wl_compositor`, `wl_subcompositor` and `wp_viewporter` on it and wraps the window's surface. winit keeps dispatching its queue unchanged; libwayland allows several queues per display.
3. **Child surface.** A `wl_surface` + `wl_subsurface`, `place_below(parent)`, desynchronised (video presents independently of the UI's commits), with an empty input region (every click reaches the UI) and a `wp_viewport` destination equal to the logical size so the buffer can be physical pixels at any scale.
4. **EGL.** `eglGetPlatformDisplay(WAYLAND, wl_display)`, a core GL 3.3 context, a `wl_egl_window` on the subsurface, swap interval 0. libEGL is loaded directly (a dozen functions, no binding crate).
5. **Render thread.** One thread owns the EGL context and the mpv render context (`vo=libmpv`, `hwdec=auto-safe`, `background=color`, framebuffer cleared to alpha 1 so the surface is opaque, letterbox bars included).
6. **Bounds.** The `Hole` publishes its layout bounds when they change; `set_bounds(x, y, w, h, scale)` sets the subsurface position, the viewport destination and the render size. Any layout works (full window, mini player, side panel).
7. **Window.** Transparent, app id `nocturne`. Every normal screen paints an opaque background; the player page paints only the hole (black until the first frame, and for the external player).

Rules that follow from this:

- **Never create an mpv Vulkan / `gpu-next` context in the app process** (below).
- libmpv is `dlopen`ed (`crates/player/src/mpv_ffi.rs`); `load` before the render context exists is fatal for `vo=libmpv`, so `EmbeddedPlayer::new` returns only after the render context is created.
- The auth header goes through `change-list http-header-fields append`, never the URL or argv.
- Teardown order is load-bearing: stop the mpv core, stop and join the render thread (free the render context while the GL context is current, then destroy the EGL surface, `wl_egl_window` and context; `eglTerminate` is not called because the display is shared), then destroy the viewport, subsurface and surface, and drop mpv last. On window close the app vetoes the close, drops the player first and then closes (`exit_on_close_request(false)`); freeing the render context without stopping playback first hung intermittently while the compositor closed the window.

## Why GL and not Vulkan: the NVIDIA crash

mpv's own Vulkan context (`vo=gpu-next`, `gpu-context=waylandvk`) inside the app process, next to the UI renderer's Vulkan instance, crashed on the first stop or quit (3 of 3 runs for each variant, independent of the hwdec mode). gdb showed the main thread jumping to a NULL pointer inside the NVIDIA GL core library from the UI's swapchain code. Best evidence, not a driver-source proof: two VkInstances in one process on NVIDIA, where destroying mpv's tears down state the UI's swapchain still uses. The OpenGL / EGL path in the same process, same driver, had 0 crashes in 60+ create / destroy cycles. Do not use `hwdec=vulkan` in-process either; it needs a Vulkan device.

The Iced renderer is kept on its Vulkan backend: with Iced's GL backend the video came out darker, so the backend is not forced to GL.

## Measurements

Release build, 1280x720 floating window, 10 s average, looping clips, `ao=null`. The desktop alone used 22-25% of the GPU, so read GPU numbers relative to that. CPU is % of one core for the whole process.

| clip | hwdec | CPU | drops (vo / decoder / delayed) |
|---|---|---|---|
| 1080p30 H.264 | nvdec | 5.1% | 0 / 0 / 0 |
| 4K30 H.264 (40 Mbit) | nvdec | 5.7% | 0 / 0 / 0 |
| 4K60 HEVC 10-bit | nvdec | 9.2% | 0 / 0 / 0 |
| (comparison) 1080p30 H.264, stock `mpv` window | nvdec | 5.0% | not measured |

In-window costs about the same as the external mpv process. Decoded frames stay in GPU memory (no CPU copy). Stability: 20/20 start / stop cycles for 1080p H.264 and for 4K HEVC in both teardown orders, 25 random resizes and three fullscreen toggles while playing, 10/10 clean quits during playback. Under Iced the app was re-verified: `hwdec-current` is `nvdec` for H.264 1080p and HEVC 10-bit with 0 dropped frames, and 25 open / close cycles showed no leak.

## Alternatives that were rejected

- **Software render into a UI image per frame**: about 29% of a core for 1080p and 115% for 4K just to download and convert, plus roughly 1 GB/s of texture uploads for 4K; no HDR path.
- **dmabuf export into a UI texture**: no hook in the UI framework's renderer.
- **Child mpv into the window (`--wid`)**: not possible on Wayland; subsurface parents are per connection and xdg-foreign only parents toplevels.
- **Helper process rendering into dmabufs** (crash-isolated): the best alternative, an estimated 800-1200 lines; needed only if in-process GL ever proves flaky.
- **XWayland `--wid`**: no transparent overlay, scaling blur; dropped together with X11 support (Linux is Wayland-only).

## Compositing notes

- Translucent UI over the video blends correctly (checked numerically on an alpha-blended swatch).
- A compositor configured with window opacity below 1 applies it to the whole surface tree, so the video looks washed out when floating and is exact in fullscreen. Exempt the app id `nocturne`, as is usual for mpv and vlc.
- The subsurface is desynchronised, so during an interactive resize the video can be a frame or two ahead of the controls.
- HDR is tone-mapped to SDR in the window. HDR needs `gpu-next` and a swapchain that can signal HDR, which libmpv's render API does not offer; the plan is an HDR pop-out window ([HDR.md](HDR.md)).

## Fallback and known gaps

With `NOCTURNE_VIDEO=external`, or when embedded setup fails (no Wayland handles, no viewporter or subcompositor, EGL or mpv init errors), `Player` runs `mpv` as a child process with its own window over a private unix socket, using `NOCTURNE_VO` (default `gpu-next`, fine in a separate process). One calm log line says why; no URLs. Choice logic: `crates/core/src/video.rs`.

Known gaps: the first frames can show black before the first video frame; AMD- or Intel-only machines and fractional scales are untested; the mini player cannot stay on top (no Wayland API; a compositor window rule for app id `nocturne` can pin it); mpv's own key bindings stay active in the external window.

## Windows: a child window

The Windows build runs the bundled `mpv.exe` as a child process like the external fallback, but draws it into a child window of the app's HWND (`crates/player/src/surface.rs`, `--wid`, created on the UI thread with `CreateWindowExW`, `WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN`). Differences from Linux that matter:

- The UI toolkit cannot draw over a child window, so the video rectangle is the hole minus the controls band while the controls show (`logic::video_rect`), and the window is hidden only while a full-window overlay needs the pixels (loading and stall overlays, prompts, errors, the HDR pop-out). Menus and panels that sit over the picture (settings and track menus, queue, context menu, stats, help, OSD, notes, the next-episode card, the skip chip, the scrub tooltip) keep the video visible around them: each is wrapped in `widget::probe`, which reports its laid-out bounds (`Msg::Cover`), and `VideoSurface::set_cutouts` turns those into a window region (the window rectangle minus the panels) so the toolkit's own pixels show through the holes. The Iced window is painted black under the video, so a panel's slightly translucent fill sits on black there. Why not an owned transparent second Iced window: it needs a multi-window `daemon` rewrite of the app plus input and focus forwarding that cannot be tested without a Windows machine; the region needs no extra windows and no input forwarding, because pixels outside the holes belong to mpv's window as before and pixels inside belong to the app window.
- The picture's window belongs to another process, so the toolkit never sees the pointer over it. mpv runs with `--input-conf` bindings that forward move, click, double click and right click with the position as `script-message nocturne-in ...`; the reader turns them into `Event::Input` and the player page handles them like its own mouse events. Keyboard focus is given back to the app window after a click. mpv's own OSD, OSC and default bindings are off.
- Resizing posts to mpv's window with `SWP_ASYNCWINDOWPOS` so a stuck mpv cannot freeze the UI. `NOCTURNE_VO` defaults to `gpu-next,gpu,` there (falls back to `gpu` when `gpu-next` cannot start, e.g. on a virtual GPU).
- Failure degrades: if the child window cannot be created, or mpv exits before the first frame, the separate mpv window opens with a one-line notice. HDR is tone-mapped to SDR in the window; the HDR pop-out (a separate fullscreen mpv) works as on Linux.

Windows checklist for the cut-outs (untested until run on Windows): open Settings, Subtitles, Audio and the queue during playback and confirm the video keeps playing around the panel with no black flash or flicker; right-click for the context menu; toggle stats (`i`) and help (`?`); hover the seek bar for the tooltip; let an OSD or a toast appear; move, resize, maximise and fullscreen the window with a menu open (the hole must follow the panel); change the display scale; click a menu row and click on the picture beside an open menu (both must work); Esc closes the menu; clicking the picture with a menu open must not leave a stale hole.

The IPC never blocks the UI thread (`crates/player/src/external.rs`): a session thread starts mpv, connects (up to 30 s), then writes queued lines; a reader thread parses output into the state, the property cache (`observe_property`), and events. `Player::prop` is a cache read; `Loaded` is emitted after the track and chapter lists are cached.

## Risks

- Raw-handle coupling: the video path reads the window's `wl_display` / `wl_surface` through winit. The `wayland-*` crates are pinned exactly in `crates/player/Cargo.toml`; re-test video on any Iced, winit or `wayland-*` bump.
- NVIDIA-specific behaviour was tested on one driver on a hybrid system; `hwdec=auto-safe` may pick VA-API on other GPUs, which is fine on GL but untested here.
- mpv's `background` option semantics changed in 0.40; keep the explicit clear to alpha 1.

To check live: `scripts/test-jellyfin.sh start` (or the demo library script), launch with `scripts/run-isolated.sh`, play something and check mpv's `hwdec-current`.
