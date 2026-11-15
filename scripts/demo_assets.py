#!/usr/bin/env python3
"""Generates the fully synthetic NOCTURNE demo library (media, artwork, NFO files) under one directory.

Everything is original: invented titles and people, procedurally drawn artwork (PIL + numpy), ffmpeg
lavfi clips with tone audio. Nothing here is a real film, show, album or person; any resemblance is
coincidental. Called by scripts/make-demo-library.sh; outputs only under the directory given as argv[1]
(which that script restricts to /tmp). Prints the catalog counts as JSON on stdout.
"""
import json
import os
import random
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from xml.sax.saxutils import escape

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont


def font(pattern, size):
    try:
        path = subprocess.run(["fc-match", "-f", "%{file}", pattern], capture_output=True, text=True).stdout
        return ImageFont.truetype(path, size)
    except Exception:
        return ImageFont.truetype("DejaVuSans.ttf", size)


# ----------------------------------------------------------------------------------------- palettes
# (top, mid, glow) of the sky, as RGB 0-255. Dark indigo/navy family with one warm accent per title.
PAL = {
    "indigo": ((8, 9, 28), (38, 40, 110), (120, 110, 210)),
    "navy": ((5, 10, 24), (18, 44, 90), (70, 140, 200)),
    "violet": ((12, 6, 28), (58, 30, 108), (190, 110, 200)),
    "ember": ((10, 8, 22), (50, 30, 80), (235, 140, 80)),
    "teal": ((4, 12, 24), (14, 56, 82), (80, 190, 190)),
    "dusk": ((10, 10, 30), (70, 50, 120), (240, 170, 120)),
    "steel": ((8, 10, 20), (34, 48, 80), (150, 170, 220)),
}

# ----------------------------------------------------------------------------------------- catalog
PEOPLE = ["Sela Marrow", "Joren Pike", "Ilka Dunmore", "Caspian Rhee", "Wren Halloway", "Odessa Finch",
          "Tamsin Verre", "Lucan Ashby", "Mirela Quist", "Anselm Dray", "Perrin Oakhurst", "Yuna Castell",
          "Bram Telfair", "Isolde Marchetti"]
DIRECTORS = ["Halden Voss", "Corvin Aldous", "Neve Tarrant", "Rosalind Keene"]

MOVIES = [
    dict(t="Hollow Meridian", y=2019, g=["Science Fiction", "Mystery"], r=7.8, cr=84, mpaa="PG-13", studio="Northfold Pictures",
         kind="mountains", pal="indigo", tag="Some signals are meant to be missed.",
         plot="A radio astronomer in a remote valley picks up a transmission that arrives a little earlier every night. Following it leads her to a place the maps leave blank."),
    dict(t="The Lanternkeeper", y=2021, g=["Fantasy", "Drama"], r=8.1, cr=88, mpaa="PG", studio="Ashwood Films",
         kind="sea", pal="ember", tag="Keep the light, keep the sea honest.",
         plot="On a lonely island, the last lanternkeeper guards a flame that is said to guide lost ships home. When a stranger washes ashore, the old rules begin to bend."),
    dict(t="Saltglass", y=2018, g=["Drama"], r=7.2, cr=79, mpaa="R", studio="Greywater Studio",
         kind="sea", pal="steel", tag="What the tide takes, it keeps polished.",
         plot="Three siblings return to a fading coastal town to sell their family's glassworks. Each shard of the past they uncover cuts a little deeper."),
    dict(t="Ember Tide", y=2022, g=["Thriller", "Drama"], r=7.5, cr=81, mpaa="R", studio="Ashwood Films",
         kind="city", pal="ember", tag="The city burns slow.",
         plot="A night-shift harbour inspector notices one cargo manifest that does not add up. By dawn, someone is making sure she never files the report."),
    dict(t="Paper Cathedrals", y=2017, g=["Romance", "Drama"], r=7.0, cr=74, mpaa="PG-13", studio="Marlowe & Dunn",
         kind="city", pal="dusk", tag="Built from letters, kept by hand.",
         plot="Two strangers correspond for a decade by leaving folded notes inside an abandoned library. Meeting in person turns out to be the hardest chapter."),
    dict(t="Midnight at Orrin Station", y=2020, g=["Mystery", "Thriller"], r=7.9, cr=86, mpaa="PG-13", studio="Northfold Pictures",
         kind="city", pal="navy", tag="The last train never leaves empty.",
         plot="A stationmaster counts one passenger too many on the final train of the night. As the hours pass, the platform clocks begin to disagree."),
    dict(t="Quiet Orbit", y=2023, g=["Science Fiction", "Drama"], r=8.3, cr=91, mpaa="PG-13", studio="Northfold Pictures",
         kind="mountains", pal="teal", tag="Home is a long way down.",
         plot="Two engineers on a decaying research station must decide whether to answer a message from Earth or to keep the silence they have grown to love."),
    dict(t="The Fox of Vellum Hill", y=2016, g=["Animation", "Family"], r=7.6, cr=83, mpaa="G", studio="Little Kiln Animation",
         kind="mountains", pal="dusk", tag="Every story starts with a small footprint.",
         plot="A curious fox cub follows a trail of ink through the hills and discovers that the valley's stories are written by the animals who live there."),
    dict(t="Static Bloom", y=2024, g=["Science Fiction", "Horror"], r=6.8, cr=70, mpaa="R", studio="Greywater Studio",
         kind="mountains", pal="violet", tag="It grows where the signal fails.",
         plot="After a storm knocks out every antenna in the county, a strange flowering static begins to spread across the fields and the people who walk through them."),
    dict(t="Northbound Lights", y=2015, g=["Adventure"], r=7.4, cr=77, mpaa="PG", studio="Marlowe & Dunn",
         kind="mountains", pal="teal", tag="Follow the glow past the last road.",
         plot="A retired pilot and her grand-niece drive the length of a frozen continent to witness a once-in-a-lifetime sky. Nothing on the road goes to plan."),
    dict(t="Velvet Cartography", y=2014, g=["Comedy", "Drama"], r=7.1, cr=75, mpaa="PG-13", studio="Marlowe & Dunn",
         kind="city", pal="indigo", tag="Maps are just opinions with borders.",
         plot="A disgraced mapmaker takes a job charting a city that keeps rearranging itself. His assistant insists it is the city that is right."),
    dict(t="Low Tide Choir", y=2022, g=["Documentary", "Music"], r=8.0, cr=89, mpaa="PG", studio="Little Kiln Animation",
         kind="sea", pal="navy", tag="Sing while the water is away.",
         plot="A documentary following a village choir that only rehearses when the tide is out, and the one winter they decide to perform on the exposed seabed."),
]

SERIES = [
    dict(t="The Ashen Coast", y=2020, g=["Drama", "Mystery"], r=8.4, mpaa="TV-14", studio="Greywater Studio", kind="sea", pal="steel",
         plot="When a lighthouse goes dark for the first time in a century, a small coastal community begins to unravel its own secrets. Each season peels back another layer of the shoreline.",
         seasons=[
             ("Low Water", 2020, [("The Dark Beacon", "The lighthouse fails, and the harbour master is the first to notice."),
                                  ("Tidemarks", "A stranger's boat is found with no one aboard."),
                                  ("Salt in the Ledger", "Old accounts point to a debt nobody wants to name."),
                                  ("What the Harbour Keeps", "The town gathers for a vigil, and someone is missing.")]),
             ("Rising", 2021, [("Spring Tide", "A new keeper arrives with a letter nobody can read."),
                               ("Undertow", "A diver brings up something that should have stayed buried."),
                               ("The Quiet Fleet", "Fishing boats return with empty nets and full secrets."),
                               ("Lantern Oil", "A fire in the old warehouse changes the investigation.")]),
             ("High Water", 2022, [("Storm Warning", "The coast braces for the worst storm in memory."),
                                   ("Ghost Light", "The beacon flickers back on, and nobody lit it."),
                                   ("Breakwater", "Alliances crack as the sea reaches the high street."),
                                   ("First Light", "The final tide uncovers the truth about the first failure.")]),
         ]),
    dict(t="Signal Garden", y=2023, g=["Science Fiction", "Comedy"], r=7.9, mpaa="TV-PG", studio="Little Kiln Animation", kind="mountains", pal="teal",
         plot="A ragtag team of botanists tends a greenhouse that happens to be the best radio receiver on the planet. Strange broadcasts are only the beginning.",
         seasons=[
             ("Season 1", 2023, [("Seedlings", "The team inherits a greenhouse and an unexplained hum."),
                                 ("Cross-Pollination", "A message arrives in the language of the ferns."),
                                 ("Root Systems", "An audit threatens to shut down the project."),
                                 ("Photosynthesis", "A sudden eclipse changes what the plants say."),
                                 ("Harvest Moon", "The garden answers back, and the team has to decide what to say.")]),
         ]),
]

ALBUMS = [
    dict(artist="Vesper Lowe", album="Glasswork", year=2021, genre="Ambient", pal="indigo", kind="mountains",
         review="Slow, glassy synth studies recorded over one winter. Written for late evenings and long drives.",
         tracks=["Opening Frost", "Pane", "Refraction", "Low Sun Through Glass", "Winter Lantern"]),
    dict(artist="The Tin Lanterns", album="Night Bus Atlas", year=2019, genre="Indie", pal="dusk", kind="city",
         review="A late-night road record: sparse guitars, soft drums and a map of the city's last bus routes.",
         tracks=["Route 9", "Transfer at Midnight", "Window Seat", "Terminus"]),
]

PHOTO_FOLDERS = [("Aurora Field Trip", "teal", "mountains", 8), ("Harbour at Dusk", "dusk", "sea", 6)]

LINES = ["The tide keeps its own counsel.", "Did you hear that?", "We should not be here.", "Hold the light steady.",
         "It was never on any map.", "Say it again, slowly.", "Nothing out here forgets.", "Then we begin at dawn.",
         "I counted twice.", "Stay close to the wall."]


# ----------------------------------------------------------------------------------------- drawing
def lerp(a, b, t):
    return a + (b - a) * t


def scene(w, h, seed, pal, kind):
    """A dark cinematic landscape as a PIL image. Deterministic per seed."""
    rng = np.random.default_rng(seed)
    top, mid, glow = (np.array(c, np.float32) for c in PAL[pal])
    ys = np.linspace(0, 1, h, dtype=np.float32)[:, None, None]
    hor = 0.62 + 0.1 * rng.random()  # horizon fraction
    t = np.clip(ys / hor, 0, 1)
    sky = np.where(t < 0.55, lerp(top, mid, t / 0.55), lerp(mid, glow, np.clip((t - 0.55) / 0.45, 0, 1) ** 1.7))
    img = np.broadcast_to(sky, (h, w, 3)).copy()
    xs = np.arange(w, dtype=np.float32)[None, :]
    # moon / sun with soft glow
    mx, my, mr = w * (0.2 + 0.6 * rng.random()), h * (0.16 + 0.2 * rng.random()), h * (0.035 + 0.03 * rng.random())
    yy = np.arange(h, dtype=np.float32)[:, None]
    d2 = (xs - mx) ** 2 + (yy - my) ** 2
    img += (np.exp(-d2 / (2 * (mr * 5) ** 2))[..., None] * glow * 0.55)
    img += (np.exp(-d2 / (2 * (mr * 1.6) ** 2))[..., None] * glow * 0.5)
    im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
    # stars
    star = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    sd = ImageDraw.Draw(star)
    for _ in range(int(w * h / 6000)):
        x, y = rng.integers(0, w), int(rng.random() ** 1.6 * h * hor)
        a, r = int(rng.integers(60, 230)), float(rng.choice([1, 1, 1.5, 2]))
        sd.ellipse((x - r, y - r, x + r, y + r), fill=(235, 240, 255, a))
    im = Image.alpha_composite(im.convert("RGBA"), star.filter(ImageFilter.GaussianBlur(0.8)))
    dd = ImageDraw.Draw(im)
    dd.ellipse((mx - mr, my - mr, mx + mr, my + mr), fill=(*(np.clip(glow * 0.4 + 170, 0, 255).astype(int)), 255))
    img = np.asarray(im.convert("RGB"), np.float32).copy()

    haze = glow * 0.35 + mid * 0.4
    if kind == "mountains":
        for i in range(4):
            x = np.linspace(0, 1, w)
            y = np.zeros(w)
            for o in range(6):
                y += np.sin(x * rng.uniform(1.2, 2.6) * 2 ** o * np.pi + rng.uniform(0, 6.3)) * h * 0.1 / 2 ** o
            ridge = h * (hor - 0.06 + 0.07 * i) + y
            dark = top * (0.9 - 0.2 * i) + 2
            dep = np.clip((yy - ridge[None, :]) / (0.3 * h), 0, 1)[..., None]
            col = (haze * (0.6 - 0.15 * i)) * (1 - dep) + dark * dep
            img = np.where((yy >= ridge[None, :])[..., None], col, img)
    elif kind == "sea":
        y0 = int(h * hor)
        rows = np.arange(h - y0, dtype=np.float32)[:, None]
        base = lerp(mid * 0.5, top * 0.6, np.clip(rows / (h - y0), 0, 1)[..., None])
        sea = np.broadcast_to(base, (h - y0, w, 3)).copy()
        sigma = mr * 0.9 + rows * 0.22
        wob = 0.55 + 0.45 * np.sin(rows * 0.9 + rng.random() * 6)
        sea += (np.exp(-((xs - mx) / sigma) ** 2) * wob * np.clip(1 - rows / (h - y0), 0.15, 1))[..., None] * glow * 0.9
        img[y0:] = sea
        # distant headland
        x = np.linspace(0, 1, w)
        ridge = y0 - h * 0.04 * np.exp(-((x - rng.random()) / 0.18) ** 2) - h * 0.01
        img = np.where(((yy >= ridge[None, :]) & (yy < y0))[..., None], top * 0.6 + 4, img)
    im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
    if kind == "city":
        dd = ImageDraw.Draw(im)
        for layer in range(3):
            x, shade = -20, top * (0.7 - 0.18 * layer) + 3
            base_y = int(h * (hor + 0.1 + 0.05 * layer))
            while x < w:
                bw = int(rng.integers(w // 40, w // 14))
                bh = int(rng.integers(h // 14, int(h * (0.34 - 0.07 * layer))))
                dd.rectangle((x, base_y - bh, x + bw, h), fill=tuple(int(c) for c in shade))
                if layer < 2:
                    for wy in range(base_y - bh + 14, base_y - 6, 22):
                        for wx in range(x + 8, x + bw - 8, 18):
                            if rng.random() < 0.28:
                                a = int(rng.integers(110, 255))
                                dd.rectangle((wx, wy, wx + 6, wy + 9), fill=(min(255, 200 + a // 10), min(255, 150 + a // 4), 90))
                x += bw + int(rng.integers(0, 10))
    out = np.asarray(im, np.float32) + rng.normal(0, 1.6, (h, w, 1))  # dither so gradients do not band
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8))


def spaced(draw, xy, text, fnt, fill, tracking):
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += draw.textlength(ch, font=fnt) + tracking


def spaced_w(draw, text, fnt, tracking):
    return sum(draw.textlength(c, font=fnt) + tracking for c in text) - tracking


def wrap(title):
    words = title.upper().split()
    if len(words) <= 2:
        return words if len(title) > 12 else [title.upper()]
    half = len(words) // 2 + len(words) % 2
    return [" ".join(words[:half]), " ".join(words[half:])]


def title_block(draw, lines, cx, y, width, fill, serif):
    size = int(width / max(7, max(len(l) for l in lines)) * 1.5)
    fnt = font(serif, size)
    trk = size * 0.16
    for l in lines:
        while spaced_w(draw, l, fnt, trk) > width and size > 20:
            size -= 2
            fnt = font(serif, size)
            trk = size * 0.16
    for l in lines:
        wd = spaced_w(draw, l, fnt, trk)
        spaced(draw, (cx - wd / 2, y), l, fnt, fill, trk)
        y += size * 1.25
    return y


def poster(sc, title, sub, seed, size=(1000, 1500)):
    w, h = size
    sw, sh = sc.size
    cw = int(sh * w / h)
    x0 = int((sw - cw) * (0.25 + 0.5 * ((seed * 37) % 100) / 100))
    p = sc.crop((x0, 0, x0 + cw, sh)).resize((w, h), Image.LANCZOS).convert("RGBA")
    g = np.linspace(0, 1, h, dtype=np.float32)[:, None]
    a = (np.clip((g - 0.4) / 0.6, 0, 1) ** 1.4 * 235).astype(np.uint8)
    shade = Image.fromarray(np.dstack([np.zeros((h, w, 3), np.uint8) + np.array([4, 5, 14], np.uint8),
                                       np.broadcast_to(a, (h, w))]), "RGBA")
    p = Image.alpha_composite(p, shade)
    d = ImageDraw.Draw(p)
    end = title_block(d, wrap(title), w / 2, h * 0.70, w * 0.78, (238, 240, 252, 255), "Noto Serif:bold")
    mono = font("Noto Sans Mono", 26)
    wd = spaced_w(d, sub, mono, 6)
    spaced(d, (w / 2 - wd / 2, min(h - 90, end + 18)), sub, mono, (150, 160, 205, 255), 6)
    return p.convert("RGB")


def logo(title):
    img = Image.new("RGBA", (1600, 600), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    title_block(d, wrap(title), 800, 60, 1400, (240, 242, 255, 245), "Noto Serif:bold")
    img = img.crop(img.getbbox())
    # The app shows logos in a 110 px tall box and crops anything much taller than ~3.4:1, so pad to that.
    w, h = img.size
    cw, ch = (w, round(w / 3.4)) if w / h > 3.4 else (round(h * 3.4), h)
    out = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    out.paste(img, (0, (ch - h) // 2))
    return out


def portrait(name, seed):
    sc = scene(400, 600, seed, list(PAL)[seed % len(PAL)], "mountains")
    d = ImageDraw.Draw(sc)
    ini = "".join(p[0] for p in name.split()[:2])
    f = font("Noto Serif:bold", 150)
    d.text((200, 300), ini, font=f, fill=(235, 238, 252), anchor="mm")
    return sc


def square(sc, title, sub, seed, n=700):
    s = poster(sc, title, sub, seed, (n, n))
    return s


def save_jpg(im, path, q=84):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    im.convert("RGB").save(path, "JPEG", quality=q, optimize=True, progressive=True)


# ----------------------------------------------------------------------------------------- nfo / media
def tag(name, val):
    return f"  <{name}>{escape(str(val))}</{name}>\n"


def cast_xml(names, roles):
    out = ""
    for i, (n, r) in enumerate(zip(names, roles)):
        out += f"  <actor>\n    <name>{escape(n)}</name>\n    <role>{escape(r)}</role>\n    <type>Actor</type>\n    <order>{i}</order>\n  </actor>\n"
    return out


ROLES = ["Maren Idris", "The Keeper", "Dr. Voss", "Ansel", "Captain Rook", "Elowen", "Inspector Hale", "Tovah", "Orrin", "Juniper"]


def cast_for(i, n=4):
    names = [PEOPLE[(i * 3 + k * 2) % len(PEOPLE)] for k in range(n)]
    return names, [ROLES[(i + k * 3) % len(ROLES)] for k in range(n)]


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def ffmpeg_clip(scene_png, out, dur, seed, title_lang_lines, chapters, hz):
    """One synthetic clip: slow push-in over the scene, grain, tone pad audio, chapters and an English subtitle."""
    d = os.path.dirname(out)
    srt, meta = out + ".srt.tmp", out + ".meta.tmp"
    step = dur / (len(title_lang_lines) + 1)
    write(srt, "".join(f"{i + 1}\n{fmt(step * (i + .4))} --> {fmt(step * (i + 1.0))}\n{l}\n\n" for i, l in enumerate(title_lang_lines)))
    cs = dur / len(chapters)
    write(meta, ";FFMETADATA1\n" + "".join(
        f"[CHAPTER]\nTIMEBASE=1/1000\nSTART={int(i * cs * 1000)}\nEND={int((i + 1) * cs * 1000)}\ntitle={c}\n" for i, c in enumerate(chapters)))
    frames = int(dur * 24)
    z = f"zoompan=z='1.0+0.09*on/{frames}':x='iw/2-(iw/zoom/2)+{(seed % 5 - 2) * 12}*on/{frames}':y='ih/2-(ih/zoom/2)':d=1:s=1280x720:fps=24"
    vf = f"{z},noise=alls=7:allf=t,vignette=PI/4.5,fade=t=in:d=0.8,fade=t=out:st={dur - 0.8}:d=0.8,format=yuv420p"
    af = f"amix=inputs=3:normalize=0,volume=0.5,tremolo=f=0.4:d=0.25,afade=t=in:d=1,afade=t=out:st={dur - 1.2}:d=1.2"
    cmd = ["ffmpeg", "-v", "error", "-y", "-loop", "1", "-framerate", "24", "-t", str(dur), "-i", scene_png,
           "-f", "lavfi", "-i", f"sine=f={hz}:d={dur}", "-f", "lavfi", "-i", f"sine=f={hz * 1.5:.1f}:d={dur}",
           "-f", "lavfi", "-i", f"sine=f={hz * 0.5:.1f}:d={dur}", "-i", srt, "-i", meta,
           "-filter_complex", f"[0:v]{vf}[v];[1:a][2:a][3:a]{af}[a]", "-map", "[v]", "-map", "[a]", "-map", "4:0",
           "-map_metadata", "5", "-c:v", "libx264", "-preset", "veryfast", "-crf", "27", "-c:a", "aac", "-b:a", "96k",
           "-c:s", "srt", "-metadata:s:s:0", "language=eng", "-metadata:s:a:0", "language=eng", "-t", str(dur), out]
    subprocess.run(cmd, check=True)
    os.remove(srt)
    os.remove(meta)


def fmt(s):
    ms = int(s * 1000)
    return f"00:00:{ms // 1000:02d},{ms % 1000:03d}"


def rlines(seed, n=3):
    r = random.Random(seed)
    return [r.choice(LINES) for _ in range(n)]


def main(root):
    media = os.path.join(root, "media")
    tmp = os.path.join(root, "scenes")
    os.makedirs(tmp, exist_ok=True)
    jobs, counts = [], dict(movies=0, series=len(SERIES), episodes=0, albums=len(ALBUMS), tracks=0, photos=0)

    def scene_png(name, seed, pal, kind, w=2560, h=1440):
        p = os.path.join(tmp, name + ".png")
        if not os.path.exists(p):
            scene(w, h, seed, pal, kind).save(p)
        return p

    # ---- logos are cheap and shape-sensitive: always rewrite them
    for m in MOVIES:
        d = os.path.join(media, "Movies", f"{m['t']} ({m['y']})")
        if os.path.isdir(d):
            logo(m["t"]).save(os.path.join(d, "logo.png"), optimize=True)
    for s_ in SERIES:
        d = os.path.join(media, "Shows", f"{s_['t']} ({s_['y']})")
        if os.path.isdir(d):
            logo(s_["t"]).save(os.path.join(d, "logo.png"), optimize=True)

    # ---- movies
    for i, m in enumerate(MOVIES):
        folder = os.path.join(media, "Movies", f"{m['t']} ({m['y']})")
        base = os.path.join(folder, f"{m['t']} ({m['y']})")
        counts["movies"] += 1
        if os.path.exists(base + ".mkv"):
            continue
        sp = scene_png(f"m{i}", 100 + i, m["pal"], m["kind"])
        sc = Image.open(sp)
        save_jpg(sc.resize((1920, 1080), Image.LANCZOS), os.path.join(folder, "backdrop.jpg"), 80)
        save_jpg(poster(sc, m["t"], f"{m['y']}  ·  {m['g'][0].upper()}", i), os.path.join(folder, "poster.jpg"), 82)
        logo(m["t"]).save(os.path.join(folder, "logo.png"), optimize=True)
        names, roles = cast_for(i)
        write(base + ".nfo",
              "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<movie>\n" + tag("title", m["t"]) + tag("sorttitle", m["t"]) + tag("year", m["y"])
              + tag("premiered", f"{m['y']}-0{1 + i % 9}-1{i % 9}") + tag("plot", m["plot"]) + tag("outline", m["plot"]) + tag("tagline", m["tag"])
              + tag("mpaa", m["mpaa"]) + tag("rating", m["r"]) + tag("criticrating", m["cr"]) + tag("studio", m["studio"])
              + "".join(tag("genre", g) for g in m["g"]) + tag("director", DIRECTORS[i % len(DIRECTORS)])
              + tag("credits", DIRECTORS[(i + 1) % len(DIRECTORS)]) + cast_xml(names, roles) + "</movie>\n")
        dur = 12 + (i * 5) % 9
        jobs.append(lambda sp=sp, o=base + ".mkv", d=dur, i=i: ffmpeg_clip(sp, o, d, i, rlines(i), ["Opening", "Descent", "Return"], 200 + 25 * i))

    # ---- series
    for si, s in enumerate(SERIES):
        sroot = os.path.join(media, "Shows", f"{s['t']} ({s['y']})")
        sp0 = scene_png(f"s{si}", 300 + si, s["pal"], s["kind"])
        sc0 = Image.open(sp0)
        if not os.path.exists(os.path.join(sroot, "tvshow.nfo")):
            save_jpg(sc0.resize((1920, 1080), Image.LANCZOS), os.path.join(sroot, "backdrop.jpg"), 80)
            save_jpg(poster(sc0, s["t"], f"{s['y']}  ·  SERIES", si + 40), os.path.join(sroot, "poster.jpg"), 82)
            logo(s["t"]).save(os.path.join(sroot, "logo.png"), optimize=True)
            names, roles = cast_for(si + 5, 5)
            write(os.path.join(sroot, "tvshow.nfo"),
                  "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<tvshow>\n" + tag("title", s["t"]) + tag("year", s["y"]) + tag("plot", s["plot"])
                  + tag("mpaa", s["mpaa"]) + tag("rating", s["r"]) + tag("studio", s["studio"]) + tag("status", "Continuing")
                  + "".join(tag("genre", g) for g in s["g"]) + cast_xml(names, roles) + "</tvshow>\n")
        for sn, (stitle, syear, eps) in enumerate(s["seasons"], 1):
            sdir = os.path.join(sroot, f"Season {sn:02d}")
            if len(s["seasons"]) > 1 and not os.path.exists(os.path.join(sdir, "season.nfo")):
                write(os.path.join(sdir, "season.nfo"), "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<season>\n" + tag("title", f"Season {sn}: {stitle}")
                      + tag("plot", f"{s['t']}, season {sn}: {stitle}.") + tag("year", syear) + tag("seasonnumber", sn) + "</season>\n")
                save_jpg(poster(sc0, f"{stitle}", f"SEASON {sn}", si * 7 + sn), os.path.join(sdir, "poster.jpg"), 80)
            for en, (etitle, eplot) in enumerate(eps, 1):
                counts["episodes"] += 1
                base = os.path.join(sdir, f"{s['t']} S{sn:02d}E{en:02d}")
                if os.path.exists(base + ".mkv"):
                    continue
                seed = 500 + si * 100 + sn * 10 + en
                sp = scene_png(f"e{si}_{sn}_{en}", seed, s["pal"] if en % 2 else ["indigo", "navy", "dusk", "violet"][(sn + en) % 4], s["kind"])
                save_jpg(Image.open(sp).resize((960, 540), Image.LANCZOS), base + "-thumb.jpg", 78)
                names, roles = cast_for(seed % 11 + si, 3)
                write(base + ".nfo", "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<episodedetails>\n" + tag("title", etitle) + tag("season", sn) + tag("episode", en)
                      + tag("plot", eplot) + tag("aired", f"{syear}-0{en}-1{en}") + tag("rating", round(7.2 + ((seed * 7) % 17) / 10, 1))
                      + cast_xml(names, roles) + "</episodedetails>\n")
                dur = 8 + (seed % 4)
                jobs.append(lambda sp=sp, o=base + ".mkv", d=dur, sd=seed: ffmpeg_clip(sp, o, d, sd, rlines(sd), ["Cold Open", "Act One", "Act Two"], 180 + sd % 90))

    # ---- music
    for ai, a in enumerate(ALBUMS):
        adir = os.path.join(media, "Music", a["artist"], a["album"])
        sc = Image.open(scene_png(f"a{ai}", 700 + ai, a["pal"], a["kind"], 1600, 1600 * 9 // 16))
        if not os.path.exists(os.path.join(adir, "folder.jpg")):
            save_jpg(square(sc, a["album"], a["artist"].upper(), ai + 3), os.path.join(adir, "folder.jpg"), 84)
            write(os.path.join(adir, "album.nfo"), "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<album>\n" + tag("title", a["album"]) + tag("year", a["year"])
                  + tag("review", a["review"]) + tag("genre", a["genre"]) + tag("artist", a["artist"]) + "</album>\n")
        for ti, tt in enumerate(a["tracks"], 1):
            counts["tracks"] += 1
            f = os.path.join(adir, f"{ti:02d} - {tt}.mp3")
            if os.path.exists(f):
                continue
            hz = 196 * 2 ** ((ti * 2 % 12) / 12)
            jobs.append(lambda f=f, a=a, tt=tt, ti=ti, hz=hz: subprocess.run(
                ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", f"sine=f={hz:.1f}:d=20", "-f", "lavfi", "-i", f"sine=f={hz * 1.5:.1f}:d=20",
                 "-filter_complex", "amix=inputs=2:normalize=0,volume=0.4,tremolo=f=3:d=0.3,afade=t=in:d=1,afade=t=out:st=18:d=2", "-c:a", "libmp3lame", "-b:a", "128k",
                 "-metadata", f"title={tt}", "-metadata", f"artist={a['artist']}", "-metadata", f"album_artist={a['artist']}", "-metadata", f"album={a['album']}",
                 "-metadata", f"track={ti}/{len(a['tracks'])}", "-metadata", f"date={a['year']}", "-metadata", f"genre={a['genre']}", f], check=True))

    # ---- photos
    for pi, (name, pal, kind, n) in enumerate(PHOTO_FOLDERS):
        for k in range(n):
            counts["photos"] += 1
            f = os.path.join(media, "Photos", name, f"IMG_{pi + 1}{k + 1:03d}.jpg")
            if not os.path.exists(f):
                save_jpg(scene(1600, 1067, 900 + pi * 20 + k, pal if k % 3 else ["indigo", "violet", "navy"][k % 3], kind), f, 82)

    # ---- people portraits (uploaded by the shell script through the API, kept outside the library)
    for i, n in enumerate(PEOPLE):
        p = os.path.join(root, "people", n + ".jpg")
        if not os.path.exists(p):
            save_jpg(portrait(n, i), p, 82)

    cp = os.path.join(root, "collection.jpg")
    if not os.path.exists(cp):
        save_jpg(poster(Image.open(scene_png("m6", 106, "teal", "mountains")), "Cold Frequencies", "COLLECTION", 9), cp, 82)
        save_jpg(Image.open(scene_png("m6", 106, "teal", "mountains")).resize((1920, 1080), Image.LANCZOS), os.path.join(root, "collection-backdrop.jpg"), 80)

    logo("Cold Frequencies").save(os.path.join(root, "collection-logo.png"), optimize=True)

    with ThreadPoolExecutor(4) as ex:
        for f in [ex.submit(j) for j in jobs]:
            f.result()
    print(json.dumps(counts))


if __name__ == "__main__":
    main(sys.argv[1])
