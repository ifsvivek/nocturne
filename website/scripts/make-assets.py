#!/usr/bin/env python3
"""Maintainer tool: regenerate the site's responsive images from the full-size captures.
Run from website/:  python3 scripts/make-assets.py   (needs Pillow; ImageMagick `magick` adds sharp-YUV chroma)

Input : src/lib/screens/*.webp, 3840x2160 (16:9) captures. Never served directly.
Output: src/lib/screens/sized/NAME-W.webp (WebP q92, method 6, sharp YUV when magick exists), never upscaled,
        and static/og.jpg (1200x630, supersampled from 2400x1260). Idempotent: sized/ is wiped and rebuilt.
AVIF was measured (q80, speed 6) against WebP q92 and is only about 5-15 percent smaller at about equal PSNR, with softer UI
text, so it is not served: WebP only keeps the markup simple and the CSP (img-src 'self') unchanged."""
import glob, io, os, shutil, subprocess
from PIL import Image, ImageDraw, ImageFilter, ImageFont

SRC, OUT = "src/lib/screens", "src/lib/screens/sized"
QUALITY = 92
WIDTHS = (640, 1280, 1920, 2560, 3840)
# Widest variant worth shipping per image, from its slot on the page (see Tour.svelte, Hero.svelte). Default 2560:
# the two-up and framed stills are at most ~750 css px wide (2560 covers 3x phones and 2x laptops).
# The full-bleed stills are up to 1680 css px (3360 px at 2x), so they keep the native 3840.
MAX_W = {"01-home": 3840, "06-playback": 3840}
DEFAULT_MAX = 2560
SKIP_FULL = {"picker", "05-search", "03-movie-detail", "smart-menu", "downloads", "hdr", "trakt", "sections"}  # close-ups only

# Close-ups, as fractions (x0, y0, x1, y1) of the capture so they do not depend on its resolution, plus the widest
# variant. Framed on the 3840x2160 captures; check by eye after a recapture (open sized/*-crop-*.webp).
CROPS = {
    # command palette with its results list (one title found on several sources)
    "search-crop": ("05-search", (0.30, 0.01, 0.70, 0.41), 1280),
    # "Server changes" paragraph and the "Allow NOCTURNE to change data" row with its OFF switch (.reticle in app.css)
    "settings-crop": ("07-settings", (0.02, 0.30, 0.98, 0.48), 2560),
    # "Who's watching?" tiles, centred, 16:9 like the account-menu still beside it
    "picker-crop": ("picker", (0.25, 0.25, 0.75, 0.75), 1920),
    # video picture without the control bar (about 24:13, like the layer stack in How.svelte)
    "video-crop": ("06-playback", (0.025, 0.0, 0.975, 0.905), 1280),
    # the player control bar and seek line (stops above the mouse pointer at the bottom edge)
    "controls-crop": ("06-playback", (0.0, 0.92, 1.0, 1.0), 1280),
    # the Versions and sources picker on the movie detail page, 16:9 like the series still beside it
    "alternates-crop": ("03-movie-detail", (0.30, 0.30, 0.70, 0.70), 1920),
    # 0.4 captures (3840x2160 headless, scale 2, see README)
    # the header's Smart menu with the five starter collections
    "smart-menu-crop": ("smart-menu", (0.31, 0.0, 0.65, 0.34), 1280),
    # the Downloads panel with one transfer in progress, 16:9
    "downloads-crop": ("downloads", (0.30, 0.30, 0.70, 0.70), 1536),
    # the HDR question in the player, 16:9
    "hdr-crop": ("hdr", (0.30, 0.30, 0.70, 0.70), 1536),
    # the default Trakt dialog over Settings (not signed in): Connect Trakt, the advanced own-Client-ID option
    "trakt-crop": ("trakt", (0.342, 0.288, 0.658, 0.712), 1536),
    # 0.5 captures (same setup)
    # the Sections editor of one server (Settings, Sources), view-only while server changes are off
    "sections-crop": ("sections", (0.355, 0.19, 0.645, 0.81), 1280),
}


def save_webp(im, path):
    """WebP q92 method 6. ImageMagick (libwebp, use-sharp-yuv) when present, else Pillow (same, without sharp YUV)."""
    if shutil.which("magick"):
        ppm = io.BytesIO()
        im.save(ppm, "PPM")
        r = subprocess.run(["magick", "ppm:-", "-define", "webp:method=6", "-define", "webp:use-sharp-yuv=1",
                            "-quality", str(QUALITY), f"webp:{path}"], input=ppm.getvalue(), capture_output=True)
        if r.returncode == 0:
            return
    im.save(path, "WEBP", quality=QUALITY, method=6)


def variants(im, name, cap):
    ws = sorted({w for w in WIDTHS if w <= min(cap, im.width)} | ({im.width} if im.width <= cap else set()))
    if not ws:  # crop narrower than the smallest step: ship it as it is
        ws = [im.width]
    for w in ws:
        r = im if w == im.width else im.resize((w, round(im.height * w / im.width)), Image.LANCZOS)
        save_webp(r, f"{OUT}/{name}-{w}.webp")


os.makedirs(OUT, exist_ok=True)
for f in glob.glob(f"{OUT}/*.webp"):
    os.remove(f)
for f in sorted(glob.glob(f"{SRC}/*.webp")):
    base = os.path.basename(f)[:-5]
    if base in SKIP_FULL:
        continue
    variants(Image.open(f).convert("RGB"), base, MAX_W.get(base, DEFAULT_MAX))
for key, (src, frac, cap) in CROPS.items():
    if not os.path.exists(f"{SRC}/{src}.webp"):
        print(f"skip {key}: no capture")
        continue
    x0, y0, x1, y1 = frac
    im = Image.open(f"{SRC}/{src}.webp").convert("RGB")
    box = (round(x0 * im.width), round(y0 * im.height), round(x1 * im.width), round(y1 * im.height))
    variants(im.crop(box), key, cap)

# OG image 1200x630 (drawn at 2x and reduced, so edges and text are clean): indigo night, home + loading overlay, wordmark.
S = 2
W, H = 1200 * S, 630 * S
BG = (7, 8, 12)
bg = Image.new("RGB", (W, H), BG)
glow = Image.new("RGB", (W, H), BG)
d = ImageDraw.Draw(glow)
d.ellipse((560 * S, -260 * S, 1360 * S, 340 * S), fill=(52, 60, 170))
d.ellipse((-200 * S, -300 * S, 440 * S, 200 * S), fill=(30, 70, 150))
bg = Image.blend(bg, glow.filter(ImageFilter.GaussianBlur(140 * S)), 0.9)


def card(path, w, pos):
    w, pos = w * S, (pos[0] * S, pos[1] * S)
    im = Image.open(path).convert("RGB")
    im = im.resize((w, round(im.height * w / im.width)), Image.LANCZOS)
    m = Image.new("L", im.size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, *im.size), 14 * S, fill=255)
    pad = Image.new("L", (im.width + 4 * S, im.height + 4 * S), 0)
    ImageDraw.Draw(pad).rounded_rectangle((0, 0, *pad.size), 16 * S, fill=255 // 6)
    bg.paste(Image.new("RGB", pad.size, (255, 255, 255)), (pos[0] - 2 * S, pos[1] - 2 * S), pad)
    bg.paste(im, pos, m)


card(f"{SRC}/01-home.webp", 700, (440, 170))
card(f"{SRC}/12-loading.webp", 520, (150, 350))
fade = Image.new("L", (W, H), 0)
ImageDraw.Draw(fade).rectangle((0, 535 * S, W, H), fill=255)
bg.paste(Image.new("RGB", (W, H), BG), (0, 0), fade.filter(ImageFilter.GaussianBlur(40 * S)))


def font(size):
    for p in ("/usr/share/fonts/noto/NotoSans-Light.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf"):
        if os.path.exists(p):
            return ImageFont.truetype(p, size * S)
    return ImageFont.load_default()


def spaced(xy, text, f, fill, gap):
    x, y = xy[0] * S, xy[1] * S
    dr = ImageDraw.Draw(bg)
    for ch in text:
        dr.text((x, y), ch, font=f, fill=fill)
        x += dr.textlength(ch, font=f) + gap * S


spaced((64, 56), "NOCTURNE", font(64), (236, 238, 245), 14)
spaced((68, 138), "YOUR MEDIA. YOUR NIGHT.", font(20), (156, 192, 255), 7)
spaced((68, 572), "A native cinematic media center for Linux", font(22), (144, 150, 171), 1)
bg.resize((1200, 630), Image.LANCZOS).save("static/og.jpg", quality=90, optimize=True, progressive=True, subsampling=0)
