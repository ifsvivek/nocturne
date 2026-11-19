#!/usr/bin/env python3 -I
"""Helper for scripts/theme-conformance.sh (stdlib + Pillow only). Subcommands:
  themes OUT.json            fetch awesome-jellyfin THEMES.md, resolve each theme's documented @import line
  web --url U --user N --pw-file F --out DIR --chrome BIN --ids IDS.json
                             capture the original jellyfin-web screens with headless Chrome (CDP, own websocket client)
  score THEMEDIR             compare web-*.png with app-*.png, write pair-*.png and scores.json
  report ...                 see cmd_report
Nothing here stores theme CSS: only the @import line and its source URL go to /tmp."""
import argparse, base64, json, os, re, socket, struct, subprocess, sys, time, urllib.request

SCREENS = ["home", "movies", "movie", "series"]
THEMES_RAW = "https://raw.githubusercontent.com/awesome-jellyfin/awesome-jellyfin/main/THEMES.md"
# Repos that document a different import for the Jellyfin version we test (12.x).
ALT_REPO = {"ElegantFin": "mihaif7/elegantfin-jf12"}
JF_MAJOR = 12


def get(url, timeout=30):
    req = urllib.request.Request(url, headers={"User-Agent": "nocturne-theme-conformance"})
    return urllib.request.urlopen(req, timeout=timeout).read().decode("utf8", "replace")


def readme(repo):
    for fn in ("README.md", "readme.md", "Readme.md"):
        try:
            return get(f"https://raw.githubusercontent.com/{repo}/HEAD/{fn}", 20)
        except Exception:
            pass
    return ""


def pick_import(text):
    """First documented `@import url(...)` of a .css file; on Jellyfin 12 prefer a file name that says 12."""
    urls = [m.group(1) for m in re.finditer(r"@import\s+url\(\s*['\"]?([^'\")\s]+\.css)['\"]?\s*\)", text)]
    if not urls:
        return None
    urls = [u for u in urls if not re.search(r"legacy|compat", u, re.I)] or urls
    twelve = [u for u in urls if re.search(r"(?<!\d)12(?!\d)|v12", u.rsplit("/", 1)[-1])]
    return (twelve or urls)[0]


def cmd_themes(a):
    md = get(THEMES_RAW)
    hidden_at = md.find("## Themes hidden pending")
    out = []
    for m in re.finditer(r"^## \[([^\]]+)\]\(([^)]+)\)[^\n]*\n\n[^\n]*?\[` 🔵 Get this Theme `\]\(([^)]+)\)", md, re.M):
        name, _home, get_link = m.groups()
        gm = re.match(r"https://github\.com/([^/]+/[^/#?]+)", get_link)
        repo = ALT_REPO.get(name) or (gm.group(1) if gm else None)
        imp = pick_import(readme(repo)) if repo else None
        if not imp and repo and name in ALT_REPO and gm:  # alt repo had nothing: the official one
            imp = pick_import(readme(gm.group(1)))
            repo = gm.group(1)
        out.append({"name": name, "status": "hidden-pending-v12" if hidden_at != -1 and m.start() > hidden_at else "active",
                    "repo": repo, "import_url": imp,
                    "source": f"https://github.com/{repo}#readme" if repo else get_link,
                    "note": "" if imp else "no documented @import line (JS builder or other install method)"})
    json.dump(out, open(a.out, "w"), indent=1)
    print(f"{len(out)} themes, {sum(1 for t in out if t['import_url'])} with a resolvable @import")


# ---------------------------------------------------------------- minimal websocket + CDP

class WS:
    def __init__(self, url):
        m = re.match(r"ws://([^:/]+):(\d+)(/.*)", url)
        host, port, path = m.group(1), int(m.group(2)), m.group(3)
        self.s = socket.create_connection((host, port), timeout=60)
        key = base64.b64encode(os.urandom(16)).decode()
        self.s.sendall((f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                        f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            buf += self.s.recv(1)
        assert b" 101 " in buf.split(b"\r\n")[0], buf
        self.id = 0

    def _read(self, n):
        b = b""
        while len(b) < n:
            c = self.s.recv(n - len(b))
            if not c:
                raise EOFError("websocket closed")
            b += c
        return b

    def send(self, text):
        d = text.encode()
        n = len(d)
        hdr = b"\x81" + (bytes([0x80 | n]) if n < 126 else b"\xfe" + struct.pack(">H", n) if n < 65536 else b"\xff" + struct.pack(">Q", n))
        mask = os.urandom(4)
        self.s.sendall(hdr + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(d)))

    def recv(self):
        msg = b""
        while True:
            b0, b1 = self._read(2)
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._read(8))[0]
            data = self._read(n)
            op = b0 & 0x0F
            if op == 8:
                raise EOFError("websocket closed")
            if op == 9:  # ping from the browser: ignore, it only needs liveness
                continue
            msg += data
            if b0 & 0x80:
                return msg.decode()

    def call(self, method, **params):
        self.id += 1
        self.send(json.dumps({"id": self.id, "method": method, "params": params}))
        while True:
            r = json.loads(self.recv())
            if r.get("id") == self.id:
                if "error" in r:
                    raise RuntimeError(f"{method}: {r['error']}")
                return r["result"]

    def js(self, expr):
        r = self.call("Runtime.evaluate", expression=expr, awaitPromise=True, returnByValue=True)
        return r.get("result", {}).get("value")


def api(url, path, token=None, body=None):
    auth = 'MediaBrowser Client="nocturne-conf", Device="script", DeviceId="nocturne-conf-script", Version="1"'
    if token:
        auth += f', Token="{token}"'
    req = urllib.request.Request(url + path, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Authorization": auth, "Content-Type": "application/json"})
    return json.loads(urllib.request.urlopen(req, timeout=60).read() or b"null")


SETTLE = """(async()=>{await document.fonts.ready;
 for(let i=0;i<40;i++){const im=[...document.images].every(x=>x.complete);
  const bg=[...document.querySelectorAll('.cardImageContainer,.itemBackdrop,.backdropImage')].every(e=>{const b=getComputedStyle(e).backgroundImage;return b==='none'||e.dataset.loaded!=='0'});
  if(im&&bg)break;await new Promise(r=>setTimeout(r,250));}
 await new Promise(r=>setTimeout(r,2500));return document.title})()"""


def cmd_web(a):
    pw = open(a.pw_file).read().strip()
    auth = api(a.url, "/Users/AuthenticateByName", body={"Username": a.user, "Pw": pw})
    tok, uid, sid = auth["AccessToken"], auth["User"]["Id"], auth["ServerId"]
    ids = json.load(open(a.ids))
    hs = {"home": "#/home", "movies": f"#/movies?topParentId={ids['movies_lib']}&collectionType=movies",
          "movie": f"#/details?id={ids['movie']}", "series": f"#/details?id={ids['series']}"}
    os.makedirs(a.out, exist_ok=True)
    port = a.chrome_port
    prof = a.profile
    proc = subprocess.Popen([a.chrome, "--headless=new", f"--remote-debugging-port={port}", f"--user-data-dir={prof}",
                             "--window-size=1920,1080", "--hide-scrollbars", "--force-device-scale-factor=1",
                             "--no-first-run", "--disable-gpu", "--mute-audio", "about:blank"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    open(a.pidfile, "w").write(str(proc.pid))
    try:
        for _ in range(100):
            try:
                tabs = json.loads(get(f"http://127.0.0.1:{port}/json/list", 2))
                break
            except Exception:
                time.sleep(0.2)
        ws = WS([t for t in tabs if t["type"] == "page"][0]["webSocketDebuggerUrl"])
        ws.call("Page.enable")
        ws.call("Emulation.setDeviceMetricsOverride", width=1920, height=1080, deviceScaleFactor=1, mobile=False)
        base = f"{a.url}/web/index.html"
        ws.call("Page.navigate", url=base)
        time.sleep(2)
        creds = {"Servers": [{"ManualAddress": a.url, "LocalAddress": a.url, "Id": sid, "UserId": uid, "AccessToken": tok,
                              "Name": "conf", "DateLastAccessed": int(time.time() * 1000), "ManualAddressOnly": True}]}
        ws.js(f"localStorage.setItem('jellyfin_credentials',{json.dumps(json.dumps(creds))});"
              "localStorage.setItem('enableAutoLogin','true');1")
        for scr in SCREENS:
            ws.call("Page.navigate", url=base + hs[scr])
            time.sleep(4)
            if "login" in (ws.js("location.hash") or ""):  # stored credentials not taken: use the login form once
                ws.js("""(async()=>{const set=(e,v)=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,v);
                  e.dispatchEvent(new Event('input',{bubbles:true}))};
                  set(document.querySelector('#txtManualName'),%s);set(document.querySelector('#txtManualPassword'),%s);
                  document.querySelector('.manualLoginForm button[type=submit], .btnSubmit').click();1})()""" % (json.dumps(a.user), json.dumps(pw)))
                time.sleep(4)
                ws.call("Page.navigate", url=base + hs[scr])
                time.sleep(3)
            ws.js(SETTLE)
            png = ws.call("Page.captureScreenshot", format="png")["data"]
            open(f"{a.out}/web-{scr}.png", "wb").write(base64.b64decode(png))
            print("web", scr, ws.js("location.hash"))
    finally:
        proc.terminate()
        try:
            proc.wait(10)
        except Exception:
            proc.kill()


# ---------------------------------------------------------------- scoring

def luminance_ssim(a, b, win=8):
    """Mean SSIM over non-overlapping win x win blocks of two equal-size 'L' images (C1=(0.01*255)^2, C2=(0.03*255)^2)."""
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    w, h = a.size
    pa, pb = a.load(), b.load()
    tot = n = 0
    for by in range(0, h - win + 1, win):
        for bx in range(0, w - win + 1, win):
            xs = [pa[bx + i, by + j] for j in range(win) for i in range(win)]
            ys = [pb[bx + i, by + j] for j in range(win) for i in range(win)]
            m = win * win
            mx, my = sum(xs) / m, sum(ys) / m
            vx = sum((x - mx) ** 2 for x in xs) / m
            vy = sum((y - my) ** 2 for y in ys) / m
            cv = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / m
            tot += ((2 * mx * my + c1) * (2 * cv + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2))
            n += 1
    return tot / n


def hist_similarity(a, b, bins=16):
    """Histogram intersection of the per-channel RGB histograms (16 bins), averaged over R, G, B: 1 = same colours."""
    ha, hb = a.histogram(), b.histogram()
    step, s = 256 // bins, 0.0
    for ch in range(3):
        ca = [sum(ha[ch * 256 + k * step: ch * 256 + (k + 1) * step]) for k in range(bins)]
        cb = [sum(hb[ch * 256 + k * step: ch * 256 + (k + 1) * step]) for k in range(bins)]
        ta, tb = sum(ca) or 1, sum(cb) or 1
        s += sum(min(x / ta, y / tb) for x, y in zip(ca, cb))
    return s / 3


def score_pair(web, app):
    from PIL import Image
    w, p = Image.open(web).convert("RGB"), Image.open(app).convert("RGB")
    sw, sp = w.resize((256, 144), Image.BOX), p.resize((256, 144), Image.BOX)
    ssim = max(0.0, luminance_ssim(sw.convert("L"), sp.convert("L")))
    hist = hist_similarity(sw, sp)
    return {"ssim": round(ssim, 3), "hist": round(hist, 3), "score": round(100 * (0.6 * ssim + 0.4 * hist))}


def cmd_score(a):
    from PIL import Image, ImageDraw
    res = {}
    for scr in SCREENS:
        w, p = f"{a.dir}/web-{scr}.png", f"{a.dir}/app-{scr}.png"
        if not (os.path.exists(w) and os.path.exists(p)):
            res[scr] = None
            continue
        res[scr] = score_pair(w, p)
        out = Image.new("RGB", (1920, 560), (24, 24, 24))
        for i, (path, label) in enumerate(((w, "jellyfin-web (original)"), (p, "NOCTURNE"))):
            out.paste(Image.open(path).convert("RGB").resize((960, 540), Image.LANCZOS), (i * 960, 20))
            ImageDraw.Draw(out).text((8 + i * 960, 4), label, fill=(230, 230, 230))
        out.save(f"{a.dir}/pair-{scr}.png")
    json.dump(res, open(f"{a.dir}/scores.json", "w"), indent=1)
    print(json.dumps(res))


def cmd_report(a):
    """Rewrite the generated block of the doc: table from $OUT/*/result.json; per-theme differences from $OUT/notes.json if present."""
    themes = {t["name"]: t for t in json.load(open(f"{a.out}/themes.json"))}
    notes = json.load(open(f"{a.out}/notes.json")) if os.path.exists(f"{a.out}/notes.json") else {}
    rows, tot = [], {s: [] for s in SCREENS}
    for name in themes:
        f = f"{a.out}/{re.sub('[^A-Za-z0-9]', '_', name)}/result.json"
        t = themes[name]
        if not os.path.exists(f):
            why = t["note"] or ("not run (hidden pending v12)" if t["status"] != "active" else "not run")
            rows.append(f"| {name} | - | - | - | - | - | n/a | {why} |")
            continue
        sc = json.load(open(f))["scores"]
        cells = []
        for s in SCREENS:
            v = sc.get(s)
            cells.append(str(v["score"]) if v else "-")
            if v:
                tot[s].append(v["score"])
        rows.append(f"| {name} | " + " | ".join(cells) + f" | {round(sum(int(c) for c in cells if c != '-') / max(1, sum(c != '-' for c in cells)))} | n/a (no app hook yet) | {notes.get(name, '-')} |")
    avg = " | ".join(str(round(sum(v) / len(v))) if v else "-" for v in tot.values())
    body = (f"Run: {time.strftime('%Y-%m-%d %H:%M')}, NOCTURNE commit `{a.commit}`, jellyfin server {a.jf}, 1920x1080.\n\n"
            "| Theme | Home | Movies | Movie | Series | Mean | Applied / not applied | Main differences |\n|---|---|---|---|---|---|---|---|\n"
            + "\n".join(rows) + f"\n| **mean of run themes** | {avg} | | | |\n")
    doc = open(a.doc).read()
    new = re.sub(r"<!-- BEGIN GENERATED -->.*<!-- END GENERATED -->", lambda m: f"<!-- BEGIN GENERATED -->\n{body}<!-- END GENERATED -->", doc, flags=re.S)
    open(a.doc, "w").write(new)


def main():
    ap = argparse.ArgumentParser()
    sp = ap.add_subparsers(dest="cmd", required=True)
    s = sp.add_parser("themes"); s.add_argument("out"); s.set_defaults(f=cmd_themes)
    s = sp.add_parser("web")
    for k in ("url", "user", "pw-file", "out", "chrome", "ids", "profile", "pidfile"):
        s.add_argument("--" + k, required=True)
    s.add_argument("--chrome-port", type=int, required=True); s.set_defaults(f=cmd_web)
    s = sp.add_parser("score"); s.add_argument("dir"); s.set_defaults(f=cmd_score)
    s = sp.add_parser("report")
    for k in ("out", "doc", "commit", "jf"):
        s.add_argument("--" + k, required=True)
    s.set_defaults(f=cmd_report)
    a = ap.parse_args()
    a.f(a)


if __name__ == "__main__":
    main()
