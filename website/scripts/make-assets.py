#!/usr/bin/env python3
"""Maintainer tool: regenerate responsive WebP sizes and the OG image from src/lib/screens/NN-name.webp.
Run from website/:  python3 scripts/make-assets.py   (needs Pillow)"""
import glob, os
from PIL import Image, ImageDraw, ImageFilter, ImageFont

SRC = "src/lib/screens"
OUT = f"{SRC}/sized"
os.makedirs(OUT, exist_ok=True)
for f in glob.glob(f"{OUT}/*.webp"):
    os.remove(f)
for f in sorted(glob.glob(f"{SRC}/*.webp")):
    im = Image.open(f).convert("RGB")
    base = os.path.basename(f)[:-5]
    for w in (480, 960, 1440):
        im.resize((w, round(im.height * w / im.width)), Image.LANCZOS).save(f"{OUT}/{base}-{w}.webp", quality=78, method=6)

# Close-ups cut from the same captures (box in 1920x1200 pixels), so small screens and detail tiles stay readable.
CROPS = {
    "search-crop": ("05-search", (560, 60, 1360, 620)),
    "settings-crop": ("07-settings", (20, 180, 1900, 500)),
    "video-crop": ("06-playback", (0, 60, 1920, 1100)),
    "controls-crop": ("06-playback", (0, 1100, 1920, 1200)),
}
for key, (src, box) in CROPS.items():
    im = Image.open(f"{SRC}/{src}.webp").convert("RGB").crop(box)
    for w in sorted({min(x, im.width) for x in (480, 960, 1440)}):
        im.resize((w, round(im.height * w / im.width)), Image.LANCZOS).save(f"{OUT}/{key}-{w}.webp", quality=80, method=6)

# OG image 1200x630: indigo night, real player + home screenshots, wordmark.
W, H = 1200, 630
bg = Image.new("RGB", (W, H), (7, 8, 12))
glow = Image.new("RGB", (W, H), (7, 8, 12))
d = ImageDraw.Draw(glow)
d.ellipse((560, -260, 1360, 340), fill=(52, 60, 170))
d.ellipse((-200, -300, 440, 200), fill=(30, 70, 150))
bg = Image.blend(bg, glow.filter(ImageFilter.GaussianBlur(140)), 0.9)

def card(path, w, pos, angle=0):
    im = Image.open(path).convert("RGB")
    im = im.resize((w, round(im.height * w / im.width)), Image.LANCZOS)
    m = Image.new("L", im.size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, *im.size), 14, fill=255)
    pad = Image.new("RGBA", (im.width + 4, im.height + 4), (255, 255, 255, 40))
    pm = Image.new("L", pad.size, 0)
    ImageDraw.Draw(pm).rounded_rectangle((0, 0, *pad.size), 16, fill=255)
    bg.paste(pad.convert("RGB"), (pos[0] - 2, pos[1] - 2), Image.composite(pm, Image.new("L", pad.size, 0), pm).point(lambda v: v // 6))
    bg.paste(im, pos, m)

card(f"{SRC}/01-home.webp", 700, (440, 190))
card(f"{SRC}/06-playback.webp", 520, (150, 360))
fade = Image.new("L", (W, H), 0)
ImageDraw.Draw(fade).rectangle((0, 520, W, H), fill=255)
bg.paste(Image.new("RGB", (W, H), (7, 8, 12)), (0, 0), fade.filter(ImageFilter.GaussianBlur(40)))

def font(size):
    for p in ("/usr/share/fonts/noto/NotoSans-Light.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf"):
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    return ImageFont.load_default()

def spaced(xy, text, f, fill, gap):
    x, y = xy
    dr = ImageDraw.Draw(bg)
    for ch in text:
        dr.text((x, y), ch, font=f, fill=fill)
        x += dr.textlength(ch, font=f) + gap

spaced((64, 56), "NOCTURNE", font(64), (236, 238, 245), 14)
spaced((68, 138), "YOUR MEDIA. YOUR NIGHT.", font(20), (156, 192, 255), 7)
spaced((68, 560), "A native cinematic media experience for Linux", font(22), (144, 150, 171), 1)
bg.save("static/og.jpg", quality=82, optimize=True, progressive=True)
