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

## Risks

- Raw-handle coupling: the video path reads the window's `wl_display` / `wl_surface` through winit. The `wayland-*` crates are pinned exactly in `crates/player/Cargo.toml`; re-test video on any Iced, winit or `wayland-*` bump.
- NVIDIA-specific behaviour was tested on one driver on a hybrid system; `hwdec=auto-safe` may pick VA-API on other GPUs, which is fine on GL but untested here.
- mpv's `background` option semantics changed in 0.40; keep the explicit clear to alpha 1.

To check live: `scripts/test-jellyfin.sh start` (or the demo library script), launch with `scripts/run-isolated.sh`, play something and check mpv's `hwdec-current`.
