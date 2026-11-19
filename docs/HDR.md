# HDR playback

Status: shipped in 0.4 (detection, prompt, setting and pop-out tested with generated HDR10/HLG clips; real HDR output untested: no HDR display yet). macOS and Windows use the same pop-out path ([PLATFORMS.md](PLATFORMS.md)).

## Why in-window video is SDR today

In-window video uses libmpv's render API, which only offers an OpenGL target and renders with the `gpu` renderer. HDR passthrough needs `gpu-next` (libplacebo) driving a swapchain that can signal HDR to the compositor. Upstream: `gpu-next` in the render API is open work (mpv #10810, PRs #16818 / #18607, unmerged) and a caller-owned Vulkan render API was declined (mpv #18343, #18258 closed). Until one of those ships, embedded playback tone-maps HDR to SDR, which is correct and looks good, but is not HDR.

## The HDR mode

When the content is HDR (from the stream info: PQ / HLG transfer, BT.2020) **and** the display path can show HDR, NOCTURNE can play it in mpv's own window instead of in the app window:

- external `Player` (separate mpv process, already our fallback player) with `vo=gpu-next`, `target-colorspace-hint=yes`, fullscreen, hardware decoding as today;
- Linux: needs a Wayland compositor with the colour-management protocol (mpv 0.42 supports it; e.g. KDE Plasma 6, recent Hyprland, GNOME with HDR enabled); detection via mpv's reported target colourspace, never guessed;
- Windows (later): `gpu-next` on D3D11 with HDR swapchains;
- macOS (later): `gpu-next` EDR output (verify the mpv version and context at port time).

**The viewer decides, and the limitation is stated, not hidden.** When an HDR video starts, NOCTURNE asks:

> This video is HDR. Video inside the NOCTURNE window is shown in SDR (a limitation of mpv's embedding API).
> **Play here in SDR** (tone-mapped) · **Open in HDR pop-out window** · [ ] Remember my choice

If the display path does not report HDR, the prompt says so and recommends Play here; the pop-out would also be SDR. The remembered choice becomes the setting **HDR video: Ask (default) / Play in the window in SDR / Open in HDR pop-out window**, changeable in Settings. SyncPlay groups never block on the prompt: they start in the window and the viewer can pop out from the player. The app keeps controlling playback over IPC (play, pause, seek, tracks, progress reporting, SyncPlay), so features stay; what changes is that the controls are mpv's own while the HDR window is up, and the app shows a "Playing in HDR" panel with the same transport controls. Dolby Vision profile 5 / 8 follow whatever `gpu-next` supports on the platform; profile 7 (mpv 0.42) is opportunistic.

## Later: HDR in the window

- Linux: switch the embedded path to `gpu-next` once the render API supports it (watch mpv #10810).
- Windows/macOS: investigate libmpv `wid` embedding (mpv renders HDR into a native child view) with the app's controls in a transparent overlay window; decide per platform after a spike, the same way the Iced spike was done.

## Verification

Needs an HDR-capable display. Test matrix per platform: HDR10 and HLG samples (generated test patterns, no personal media), detection on an SDR display (must tone-map in the window, no HDR window), switching tracks and seeking in HDR mode, progress reporting, SyncPlay, quitting from the HDR window. Results go in [QA_REPORT.md](QA_REPORT.md).
