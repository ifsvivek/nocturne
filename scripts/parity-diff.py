#!/usr/bin/env python3
"""Compare two window captures pixel by pixel (used by scripts/headless-parity.sh).

usage: parity-diff.py a.png b.png diff.png [tolerance=16] [max-mean=1.0] [max-over=1.5]
Prints the mean per-channel difference (0..255), the largest difference and the share of pixels that differ by more than the tolerance in any channel; writes an amplified (x6) difference image. Exits 1 when the mean or the share is above its limit (percent of pixels for max-over).
"""
import sys

from PIL import Image, ImageChops

a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
out = sys.argv[3]
tol = int(sys.argv[4]) if len(sys.argv) > 4 else 16
max_mean = float(sys.argv[5]) if len(sys.argv) > 5 else 1.0
max_over = float(sys.argv[6]) if len(sys.argv) > 6 else 1.5
w, h = min(a.width, b.width), min(a.height, b.height)
a, b = a.crop((0, 0, w, h)), b.crop((0, 0, w, h))
d = ImageChops.difference(a, b)
px = d.load()
total, mx, over = 0.0, 0, 0
for y in range(h):
    for x in range(w):
        r, g, bl = px[x, y]
        m = max(r, g, bl)
        total += (r + g + bl) / 3
        mx = max(mx, m)
        over += m > tol
n = w * h
mean, share = total / n, 100 * over / n
print(f"{w}x{h} mean {mean:.3f}/255 max {mx} over {tol}: {over} px ({share:.2f}%)")
d.point(lambda v: min(255, v * 6)).save(out)
sys.exit(0 if mean <= max_mean and share <= max_over else 1)
