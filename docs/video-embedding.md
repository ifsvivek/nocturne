# In-window GPU video for NOCTURNE: feasibility report

> Written against the GPUI-based 0.2.0 and kept as the record of why the video path looks like this. The mechanism
> (Wayland subsurface below a transparent window, libmpv OpenGL render API through EGL, never an in-process mpv Vulkan
> context) carries over to the Iced UI unchanged (spike `spikes/iced-embed`, [DECISIONS.md](DECISIONS.md)); the GPUI-specific
> details below (raw handles from `gpui::Window`, the paint callback, the version pin) describe the legacy `crates/app`.
> Modules mentioned as `crates/app/src/...` that moved are noted inline.

Prototype (historical): `crates/video-proto`, removed once integrated; its code lives on as `crates/player/src/embedded.rs` (see "Integrated" below).
Test environment (a single hybrid-graphics machine; other hardware is untested): Arch Linux, a Wayland compositor,
NVIDIA discrete GPU (proprietary driver) plus AMD iGPU, high-refresh monitor at scale 1.0, mpv/libmpv 0.41
(client API 2.5), GPUI 0.2.2, `libmpv2` 7.0.1 (since replaced by the runtime loader in `crates/player/src/mpv_ffi.rs`).

## Recommendation: GO (with caveats)

Approach A works. libmpv renders with its OpenGL render API (EGL) into a `wl_subsurface` placed BELOW a
transparent GPUI window. GPUI draws its controls over the video on the parent surface.

- hwdec is active (`nvdec`, zero CPU frame copies), 0 dropped frames at 1080p30, 4K30 H.264, 4K30 HEVC 10-bit
  PQ and 4K60 HEVC 10-bit.
- 20/20 start/stop cycles are clean for 1080p H.264 and for 4K HEVC (short hold), in both teardown orders.
- It is crash-safe on this NVIDIA stack provided mpv uses its OpenGL render API. The earlier SIGSEGV is
  reproduced and root-caused (below): it comes from a second Vulkan instance (mpv's `gpu-next` Vulkan context)
  living in the same process as GPUI's Vulkan renderer. The GL/EGL subsurface path does not hit it.

Caveats (details below):
1. About 300 lines of `unsafe` Wayland/EGL glue that depends on GPUI internals staying as they are (raw handles
   of `wl_display`/`wl_surface`, one shared `wl_display`). Pin `gpui = "=0.2.2"`.
2. Wayland only. GPUI's X11 backend would need a separate implementation, which is not worth it.
3. HDR is tone-mapped to SDR (no wp_color_management in GPUI), so no HDR passthrough. 4K HDR plays correctly as SDR.
4. The compositor applies its own window opacity to the whole surface tree (see "Compositing"). On
   compositors configured with window opacity below 1 this needs an opaque rule for the app id (`nocturne`), the same
   as for mpv/vlc.
5. Subsurface resize and the parent GPUI redraw are not atomic: the video can be a frame ahead of the controls.
6. No fractional-scale test was possible (the test machine ran at scale 1.0).

Fallback if A is ever rejected: keep the external mpv child (today's design). The best crash-isolated in-window
alternative is a helper process that renders into dmabufs and hands them to the app (section C), which is a much
bigger build and is not needed because A is stable.

## How A works (what was built)

`VideoSurface::new(display, parent_surface, hwdec)` (prototype; now `EmbeddedPlayer::new(display, parent)` in `crates/player/src/embedded.rs`):

1. Raw handles: `gpui::Window` implements `HasWindowHandle`/`HasDisplayHandle`
   (`WaylandWindowHandle.surface` = GPUI's `wl_surface*`, `WaylandDisplayHandle.display` = its `wl_display*`).
   They are read inside the GPUI paint callback, on the main thread.
2. Wayland: `wayland_backend::Backend::from_foreign_display(display)` wraps GPUI's display (never owned or closed by us)
   with our OWN event queue. We bind `wl_compositor`, `wl_subcompositor`, `wp_viewporter` on that queue and wrap GPUI's
   surface with `ObjectId::from_ptr`. GPUI's calloop keeps dispatching its own queue unchanged. We only dispatch our
   queue (no-ops) on the GPUI thread inside `set_bounds`. This works because libwayland supports multiple queues per display.
3. Child `wl_surface` + `wl_subsurface`: `place_below(parent)`, `set_desync()` (video presents independently of GPUI
   commits), empty input region (all clicks and hover reach GPUI), opaque region = video rect, `wp_viewport`
   destination = logical size so the buffer can be physical pixels at any scale.
4. EGL: `eglGetPlatformDisplay(WAYLAND, wl_display)`, core GL 3.3 context, `wl_egl_window` on the subsurface,
   `eglSwapInterval(0)`. libEGL is linked directly (about 12 extern fns, no binding crate).
5. One render thread owns the EGL context and the mpv `RenderContext`
   (`ADVANCED_CONTROL`, `WAYLAND_DISPLAY` for the vaapi interop, update callback -> condvar, `render(fbo 0)`, `swap`,
   `report_swap`). mpv: `vo=libmpv hwdec=auto-safe background=color`; the FBO is cleared to alpha 1 so the video
   surface is fully opaque.
6. GPUI side (`main.rs`): window with `WindowBackgroundAppearance::Transparent`, root div with NO background, a
   `canvas()` element filling the window whose paint callback reports its bounds to `set_bounds(x, y, w, h, scale)`;
   an overlay bar (63% black, button, progress fill), and a 50% red swatch to test blending.
   `set_bounds` = `set_position` + `viewport.set_destination` + atomic size for the render thread (which calls
   `wl_egl_window_resize`). Position applies at the parent's next commit, and GPUI commits right after because
   the bounds change calls `cx.notify()`.

Gotchas found:
- `loadfile` before the render context exists is fatal for `vo=libmpv` ("No render context set"). `new()` therefore
  returns only after the render context is created.
- mpv's default background for libmpv has alpha 0; set `background=color` AND clear alpha 1, else letterbox bars
  are transparent.
- With `gpu-context=wayland` + `gpu-api=vulkan` mpv fails ("Failed initializing any suitable GPU context"); the Vulkan
  context name is `waylandvk`. Matters only for the own-window repro mode.

### Teardown order (load-bearing, in `Drop for VideoSurface`)
1. set stop flag, wake and `join` the render thread. In the thread: drop mpv `RenderContext` (GL context still current),
   `eglMakeCurrent(none)`, destroy `EGLSurface`, drop `wl_egl_window`, destroy `EGLContext`.
   `eglTerminate` is intentionally not called (process-wide display, shared with other EGL users).
2. then destroy `wp_viewport`, `wl_subsurface`, `wl_surface`; flush. GPUI's parent surface is never touched.
3. `Arc<Mpv>` is dropped last (`mpv_destroy`). The render context borrows `&Mpv`, so this is enforced by the types.

## Compositing correctness
- GPUI alpha on the tested stack: blade picks `PRE_MULTIPLIED` (or `POST_MULTIPLIED`) composite alpha when the window is
  transparent. The compositor output matches straight maths: red 50% over a yellow video pixel
  (248,237,17) gave (251,127,9); expected (251,118,8)+-rounding. So translucent GPUI UI over video blends correctly.
  The text, rounded button and 63% black control bar all render properly.
- Transparent hole = simply not painting a background where the video is.
- Clicks over the video go to GPUI (empty input region), so GPUI hover/click handlers for controls work on top.
- A compositor configured with window opacity below 1 for active and inactive windows applies it to every window unless
  the app id is exempted (as is usual for mpv/vlc). This applies to the WHOLE surface tree, so
  the video (and letterbox bars) look washed out/see-through windowed and are exact in fullscreen. NOCTURNE needs
  the same exemption (app_id `nocturne`). Not a code issue.
- Observed once, also WITHOUT any video (`VP_NOVIDEO=1`): right after the compositor enlarges the window, GPUI's own
  frame lags for a moment (bottom strip not yet painted). This is GPUI or the compositor, not the subsurface.

## Resize / move / fullscreen
- Bounds come from GPUI layout every frame (canvas prepaint), so any layout (e.g. video in a card, PiP, side panel)
  is supported: `set_position`, viewport destination, `wl_egl_window_resize`.
- 25 random resizes + 3 fullscreen toggles (4K H.264 playing): alive, `rendered/s` stayed 30, final `set_bounds` equal to
  the final window size, video re-letterboxed correctly (600 -> 1000 -> 700 -> fullscreen 2560x1440 all seen).
- Not atomic: the subsurface is desync, so during an interactive resize the video can be one or two frames ahead of
  the controls. Fix if it bothers: use sync mode only while bounds are changing.

## Measurements (release build, 1280x720 floating window, 10 s average, looping clip, `ao=null`)
Machine was NOT idle (the desktop alone is about 22-25% GPU util), so GPU numbers are noisy; read them relative to that.
CPU = `top` % of one core (whole process). hwdec = `hwdec-current` from mpv.

| clip | path | hwdec | CPU | GPU util | NVDEC util | vo drops / decoder drops / delayed |
|---|---|---|---|---|---|---|
| 1080p30 H.264 | subsurface | nvdec | 5.1% | 28.6% | 9.6% | 0 / 0 / 0 |
| 4K30 H.264 (40 Mbit) | subsurface | nvdec | 5.7% | 37.0% | 30.4% | 0 / 0 / 0 |
| 4K30 HEVC 10-bit PQ/BT.2020 | subsurface (debug build) | nvdec | 10.0% | 32.1% | 11.1% | 0 / 0 / 0 |
| 4K60 HEVC 10-bit | subsurface | nvdec | 9.2% | 32.3% | 21.7% | 0 / 0 / 0 |
| 1080p30 H.264 | stock `mpv` own window (today's child) | nvdec | 5.0% | 28.0% | 11.1% | not measured |
| 4K30 H.264 | stock `mpv` own window | nvdec | 4.0% | 35.1% | 33.7% | not measured |
| 4K60 HEVC 10-bit | stock `mpv` own window | nvdec | 6.8% | 32.2% | 22.7% | not measured |

Reading: in-window costs about the same as today's child process (about 1-3 points more CPU: GPUI repainting the progress
bar plus the render thread). Decoded frames stay in GPU memory (nvdec -> CUDA-GL interop -> GL texture), there is no CPU frame copy.
Frame pacing: `vf_fps` = source fps, `rendered/s` = source fps, no drops. mpv paces by timer (no display-sync with
`swap_interval 0`); on a 180 Hz panel 24/30/60 fps content has no visible issues. Strict display-resample judder
quality was not evaluated.
HDR: the HEVC PQ clip decodes with nvdec and is tone-mapped to the SDR surface (libplacebo).

## Crash safety
### 20-cycle stability (each cycle = new VideoSurface (EGL + mpv + subsurface), play, drop, in the SAME GPUI window)
| scenario | result |
|---|---|
| 1080p H.264, hold 1.2 s, 20 cycles, surface dropped before window | 20/20 stopped, exit 0 |
| 4K HEVC 10-bit, hold 0.4 s, 20 cycles | 20/20, exit 0 |
| window removed while subsurface still alive (wrong order) | exit 0 (no crash, but do not rely on it) |
| quit app while playing, 5 runs | exit 0 x5 |
| (comparison) own-window mpv, `gpu-api=opengl`, 20 cycles; quit while playing x4 | exit 0 |
| (comparison) own-window mpv, `gpu-api=vulkan` (`waylandvk`), stop then next cycle / quit while playing | SIGSEGV (139) EVERY time |

### The earlier SIGSEGV, reproduced and explained
In-process mpv with its Vulkan GPU context (`vo=gpu-next`, `gpu-context=waylandvk`) next to GPUI's Vulkan (blade):
crash on the first stop/quit. gdb: main thread, `blade_graphics::Surface::acquire_frame` (vkAcquireNextImageKHR) or
`Context::destroy_surface` -> jump to a NULL pointer inside `libnvidia-glcore.so.615.78.08`.
Independent of hwdec (`no`, `nvdec-copy`, `vulkan`). It happens for 3/3 runs of each variant after mpv's Vulkan instance
is destroyed. Cause (best evidence, not a driver-source proof): two VkInstances in one process on NVIDIA; destroying
mpv's (libplacebo's) tears down shared driver state that GPUI's swapchain still uses. OpenGL/EGL contexts in the same process
do not do this: same driver, same GPUI, 0 crashes in 60+ create/destroy cycles. So the rule is: NEVER let mpv (libplacebo)
create a Vulkan instance inside the GPUI process; use the GL render API. `vo=libmpv` with the OpenGL render API never does.
Do not use `hwdec=vulkan` in-process either (it needs a Vulkan device).

## Other approaches (not built, with reasoning)
- B1: mpv software render (or GL FBO readback) into a GPUI image per frame. GPUI 0.2.2 has no external texture hook:
  `elements::surface` is macOS-only, no dmabuf/Vulkan external-memory import. Only CPU `RenderImage` (BGRA bytes).
  Lower-bound CPU cost measured with ffmpeg (nvdec decode, download + convert to BGRA, no GPUI upload): 1080p about 29% of a core
  (2.9 cpu-s per 10 s), 4K about 115% of a core (11.5 cpu-s per 10 s) at 30 fps, plus about 1 GB/s of 4K BGRA through GPUI's image/atlas upload per
  stream and every frame as a new texture. Not viable for 4K/HDR; marginal for 1080p. No HDR path.
- B2: dmabuf export of the mpv GL frame imported as a GPUI texture: no GPUI hook (above). Dead end without patching GPUI/blade.
- C1: child mpv into the GPUI window: mpv has no `--wid` on Wayland. Subsurface parents are per-connection (a separate process cannot
  parent a surface to GPUI's), and xdg-foreign only parents toplevels. Not possible directly.
- C2: child mpv + shared dmabuf: helper process renders with the libmpv render API (EGL), exports the frame (GBM/EGL dmabuf export), sends fd + modifier over a
  unix socket (SCM_RIGHTS) and the app attaches it to the same kind of subsurface via `zwp_linux_dmabuf_v1` (+ explicit sync). This is the
  best crash-isolated design (mpv/EGL crashes do not kill the app), at an estimated 800-1200 lines incl. buffer pool, sync and recovery. Needed only if
  in-process GL ever proves flaky. mpv's own `vo=dmabuf-wayland` makes its own window, so it does not help.
- C3: XWayland `--wid`: needs the whole GPUI app on its X11 backend, which gives scaling blur and no transparent overlay (an X child window cannot be
  alpha-composited under GPUI controls). Last resort only; rejected.

## Integrated

`crates/video-proto` is gone. What shipped:

- `nocturne_player::EmbeddedPlayer` (`crates/player/src/embedded.rs`): `new(wl_display*, wl_surface*)`,
  `load(url, headers)`, `play/pause/toggle/stop/seek_to/seek_by/set_volume`, `state()`, `events()` (same `State`/`Event`
  types as the child-process `Player`), plus `set_bounds(x, y, w, h, scale)` for the video rect. State comes from mpv
  property observers on an event thread. The auth header goes through `change-list http-header-fields append`
  (never URL or argv). The creation site carries the rule "OpenGL render API only, never gpu/gpu-next Vulkan in-process".
- `Playback` (`crates/core/src/video.rs`): a two-variant enum (`Embedded`, `External(Player)`) with the shared method
  names. The backend is chosen at the first play: `NOCTURNE_VIDEO=external|embedded` overrides, otherwise embed when
  `WAYLAND_DISPLAY` is set. If creating the embedded player fails (no wayland handles, no viewporter/subcompositor,
  EGL/mpv init), one calm line (no URLs) is logged and the app uses the child-process `Player` for the rest of the session.
- Window: `WindowBackgroundAppearance::Transparent`, `app_id = nocturne`. Every normal screen paints the opaque root
  background; the player page paints nothing except a `canvas()` whose bounds are pushed to `set_bounds` each frame
  (black behind it until the file is loaded). The now-playing bar overlays the video (translucent, `occlude()` so clicks on
  it do not toggle pause) and auto-hides after 3 s without mouse movement or keys (cursor hides with it).
- Keys on the player page: Space pause, Left/Right seek 10 s, Up/Down volume, `m` mute, `s` stop, `f` fullscreen,
  `n` next episode, Esc leaves fullscreen first and then closes playback. A click on the video toggles pause,
  a double-click also toggles fullscreen.
- Pins: `gpui = "=0.2.2"`, `libmpv2 = "=7.0.1"`, `wayland-backend/-client/-egl/-protocols` exact.

Verified live on a throwaway Jellyfin (Wayland, NVIDIA, scale 1.0): playback inside the window, keyboard and mouse
controls, scrub seek, fullscreen in/out (keys and double-click), resize while playing, 2x 20 start/stop cycles through
the app, clean exit while playing (10/10), forced external fallback (`NOCTURNE_VIDEO=external`).

Teardown findings from integration (beyond the order above):
- Closing the window while playing must not drop the player inside the compositor's close event, and freeing the mpv render
  context without stopping playback first hung intermittently while the compositor was closing the window (render thread stuck
  in `mpv_render_context_free`; reproducible about 3 of 4 runs, 0 of 18 after the fix). The fix: `stop` the mpv core before
  stopping the render thread, and on close veto the compositor event, drop the player from a task, then `cx.quit()`.
- A second `gpui` update from inside the close callback deadlocks; do the teardown from a spawned task.

Player features (both backends, through `Playback::{command, prop, set}` which map to mpv commands/properties): two-row
controls bar (transport, Ends at, title, subtitles/audio menus, volume to 130, settings, fullscreen, stop; scrub row with
elapsed, buffered segment, chapter ticks, remaining/total toggle), popovers navigable by keyboard, speed 0.25-4x, aspect
presets, repeat one, subtitle/audio delay, frame step, chapter navigation, playback info panel, key help, OSD chips,
screenshot (Ctrl+S, `nocturne-<unix>.png` in the pictures dir).

Known gaps: the first frames can show black before the first video frame; subsurface resize is not atomic with GPUI's
repaint (see Resize); AMD/Intel-only and fractional-scale machines are untested; X11 sessions use the external player.

## Stage 2 player (feature-rich)

The player page is its own entity (legacy GPUI: `crates/app/src/player_view.rs` state and `player_ui.rs` rendering; shared:
`crates/core/src/player_logic.rs` pure rules and `player_mpris.rs` D-Bus). The owner (`Root`) creates it with a `PlayerCtx { client, user, hooks, prefs }`
and drops it on `PlayerEvent::Closed`. `PlayerHooks` has five methods but the page only calls `set_favorite` /
`is_favorite`; started/progress/stopped reporting belongs to the app tick.

Shipped: skip intro/credits chip (+ auto-skip of intro/recap with `prefs.auto_skip_intro`), next-episode card with
countdown and autoplay (`prefs.autoplay_next`, Esc cancels), previous/next, queue panel (series episodes, jump),
repeat none/one/all, trickplay hover thumbnails, play-method label, quality menu (HLS transcode, auth in
`http-header-fields` only) with calm direct-play fallback, Jellyfin external subtitles via `sub-add`, friendly track
names, default audio/subtitle selection from prefs and per-series/season memory (session only), resume prompt,
subtitle style presets and position (lifted above the controls bar), picture controls (brightness, contrast,
saturation, gamma, zoom, pan, rotate, deinterlace, HDR tone mapping), normalize/downmix/output device, A-B loop,
right-click menu, favourite heart, MPRIS (play/pause/stop/next/previous/seek, metadata + poster file in
`$XDG_RUNTIME_DIR`), idle inhibit via `org.freedesktop.ScreenSaver` while playing.

Limits: the mini player resizes the window to 640x360 (the app's minimum size may clamp it) and cannot stay on top
(no GPUI/Wayland API; a compositor window rule for app id `nocturne` can pin it). The external mpv
backend shows the same controls on a black page, but video is in mpv's own window. Media segments need a server-side
provider (core Jellyfin has none); without one the skip chip never appears.

## Risks
- Raw-pointer coupling to gpui 0.2.2 internals (`wl_surface`/`wl_display` handles, shared connection). Pin the version; re-test on every gpui bump.
- NVIDIA/driver specific behaviour (tested only on driver 615.78.08 on a hybrid NVIDIA/AMD system; EGL used the NVIDIA GPU, mpv also probed VAAPI on the AMD iGPU via `drmprime`,
  without issue). Needs a pass on AMD/Intel-only machines (vaapi-egl path) before release.
- Compositor opacity rules, fractional scale (viewport design should be fine, untested), subsurface vs parent resize lag.
- No Vulkan in-process (hard rule above). `hwdec=auto-safe` may still pick vaapi/drmprime on other GPUs, which is OK on GL.
- mpv option `background`/`background-color` semantics changed in 0.40+; keep the explicit clear.

## Commands to run (historical)
The prototype binary and its `--cycles`/`--mode own` repro switches were removed with `crates/video-proto`. The Vulkan-in-process
SIGSEGV can still be reproduced with a standalone libmpv program using `vo=gpu-next gpu-context=waylandvk` next to any GPUI window.
Live app verification: `scripts/test-jellyfin.sh start`, launch the app with `NOCTURNE_JF_URL`/`NOCTURNE_JF_TOKEN` taken from the
throwaway `test.env`, press Enter on the Home hero.
