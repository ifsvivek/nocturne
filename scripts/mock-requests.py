#!/usr/bin/env python3
"""Mock services with invented demo data, for headless screenshots of the Requests page (no Docker needed).

One process, four loopback ports (127.0.0.1 only):
  TMDB   (default 5871)  /3/...      the parts of TMDB's v3 API that Discover reads, and /img/<size>/<path> posters
  Seerr  (default 5872)  /api/v1/... status, sign-in, discover, search, details, requests
  Radarr (default 5873)  /api/v3/... status, profiles, root folders, lookup, add, progress
  Sonarr (default 5874)  /api/v3/...
The Radarr / Sonarr API key is `mock-demo-key` (override with MOCK_ARR_KEY); it is not a secret. Titles are invented; posters are
generated gradients with the title. An added title goes queued -> downloading -> has a file over about 30 seconds.
Usage: scripts/mock-requests.py [--log FILE]   (stop it with Ctrl+C or by killing the pid you started)
"""
import hashlib
import io
import json
import os
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from PIL import Image, ImageDraw, ImageFont

PORTS = {k: int(os.environ.get(f"MOCK_{k.upper()}_PORT", p)) for k, p in
         [("tmdb", 5871), ("seerr", 5872), ("radarr", 5873), ("sonarr", 5874)]}
ARR_KEY = os.environ.get("MOCK_ARR_KEY", "mock-demo-key")
LOG = None

# (kind, tmdb id, tvdb id, title, year, rating, genre ids, overview)
TITLES = [
    ("movie", 9101, 0, "Ember Tide", 2022, 7.8, [18, 878], "A lighthouse keeper charts a tide that burns."),
    ("movie", 9102, 0, "Quiet Orbit", 2023, 8.1, [878, 53], "Four engineers drift past the last relay."),
    ("movie", 9103, 0, "Paper Harbour", 2019, 6.9, [35, 10749], "Two rival kite makers share one pier."),
    ("movie", 9104, 0, "Last Train to Sundown", 2023, 7.4, [28, 12], "A conductor must outrun the dusk."),
    ("movie", 9105, 0, "Glass Meridian", 2025, 7.0, [9648, 53], "A cartographer finds a street that is not on any map."),
    ("movie", 9106, 0, "Velvet Static", 2026, 0.0, [27, 53], "A radio drama begins to answer back."),
    ("movie", 9107, 0, "Harvest of Lanterns", 2018, 8.4, [16, 10751], "A village floats its wishes downriver."),
    ("movie", 9109, 0, "Hollow Meridian", 2019, 7.8, [878, 9648], "A radio astronomer picks up a transmission that arrives earlier each night."),
    ("movie", 9108, 0, "Salt Road", 2022, 7.2, [12, 18], "A caravan crosses the white flats."),
    ("tv", 9201, 880001, "Night Shift Radio", 2024, 8.3, [18, 9648], "The late host hears callers from tomorrow."),
    ("tv", 9202, 880002, "Copper Valley", 2022, 7.6, [18, 80], "A mining town keeps one very old secret."),
    ("tv", 9203, 880003, "Orbit Kitchen", 2025, 7.9, [35, 10751], "A diner on a space station feeds everyone."),
    ("tv", 9204, 880004, "The Long Thaw", 2023, 8.0, [18, 10765], "Spring arrives a century late."),
    ("tv", 9205, 880005, "Tidewater Detectives", 2021, 6.8, [80, 9648], "Two harbour inspectors, one stubborn case."),
    ("tv", 9207, 910002, "Signal Garden", 2023, 7.9, [35, 10765], "Botanists tend a greenhouse that is also a radio receiver."),
    ("tv", 9206, 880006, "Paper Moons", 2026, 0.0, [16, 10751], "A shadow-puppet troupe tours the coast."),
]
SEASONS = {9207: 1, 9201: 3, 9202: 2, 9203: 1, 9204: 4, 9205: 2, 9206: 1}
TVMAP = {t[1]: t for t in TITLES}
# Seerr media status of a few titles: 2 pending, 3 processing, 5 available
SEERR_STATE = {9103: 2, 9204: 3, 9108: 5}
SEERR_REQUESTS = []  # {id, tmdb, kind, status, seasons}
RADARR = {}  # tmdb id -> added time
SONARR = {}  # tvdb id -> added time
CURSOR = {"seerr_id": 1}


def log(*a):
    line = " ".join(str(x) for x in a)
    if LOG:
        with open(LOG, "a") as f:
            f.write(line + "\n")


def poster(title, key):
    h = hashlib.sha256(key.encode()).digest()
    a, b = (h[0] % 160 + 30, h[1] % 160 + 30, h[2] % 160 + 60), (h[3] % 120 + 10, h[4] % 120 + 10, h[5] % 120 + 30)
    w, ht = 342, 513
    img = Image.new("RGB", (w, ht))
    px = img.load()
    for y in range(ht):
        t = y / ht
        for x in range(w):
            px[x, y] = tuple(int(a[i] * (1 - t) + b[i] * t) for i in range(3))
    d = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 34)
    except Exception:
        font = ImageFont.load_default()
    words, lines, cur = title.split(), [], ""
    for wd in words:
        if len(cur + " " + wd) > 14 and cur:
            lines.append(cur)
            cur = wd
        else:
            cur = (cur + " " + wd).strip()
    lines.append(cur)
    y = ht // 2 - 24 * len(lines)
    for ln in lines:
        d.text((24, y), ln, fill=(255, 255, 255), font=font)
        y += 46
    out = io.BytesIO()
    img.save(out, "PNG")
    return out.getvalue()


def card(t, media_type=True):
    kind, tmdb, _tvdb, title, year, rating, genres, overview = t
    o = {"id": tmdb, "overview": overview, "poster_path": f"/p{tmdb}.png", "vote_average": rating, "genre_ids": genres}
    if kind == "movie":
        o.update(title=title, release_date=f"{year}-06-01")
    else:
        o.update(name=title, first_air_date=f"{year}-09-01")
    if media_type:
        o["media_type"] = kind
    return o


def pick(kind=None, genre=None, year=None, query=None):
    out = []
    for t in TITLES:
        if kind and t[0] != kind:
            continue
        if genre and genre not in t[6]:
            continue
        if year and t[4] != year:
            continue
        if query and query.lower() not in t[3].lower():
            continue
        out.append(t)
    return out


def seerr_hit(t):
    o = card(t)
    o["mediaType"] = t[0]
    o["posterPath"] = o.pop("poster_path")
    o["voteAverage"] = o.pop("vote_average")
    if t[0] == "movie":
        o["releaseDate"] = o.pop("release_date")
    else:
        o["firstAirDate"] = o.pop("first_air_date")
    st = SEERR_STATE.get(t[1])
    reqs = [r for r in SEERR_REQUESTS if r["tmdb"] == t[1]]
    if reqs:
        st = 2
    if st:
        o["mediaInfo"] = {"tmdbId": t[1], "status": st, "requests": [], "seasons": []}
    return o


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, code, body, ctype="application/json", extra=()):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(n) or b"{}") if n else {}

    def do_GET(self):
        self.route("GET")

    def do_POST(self):
        self.route("POST")

    def do_DELETE(self):
        self.route("DELETE")

    def route(self, method):
        u = urllib.parse.urlparse(self.path)
        q = {k: v[0] for k, v in urllib.parse.parse_qs(u.query).items()}
        kind = self.server.kind
        log(kind, method, u.path, json.dumps(q) if q else "")
        try:
            getattr(self, "h_" + kind)(method, u.path, q)
        except Exception as e:  # keep the demo alive
            log("error", repr(e))
            self.send(500, {"message": repr(e)})

    # ---- TMDB ----
    def h_tmdb(self, m, p, q):
        if p.startswith("/img/"):
            name = p.rsplit("/", 1)[1]
            tid = int(name[1:].split(".")[0])
            t = TVMAP.get(tid) or next(x for x in TITLES if x[1] == tid)
            return self.send(200, poster(t[3], str(tid)), "image/png")
        if p == "/3/configuration":
            return self.send(200, {"images": {}})
        pg = int(q.get("page", 1))
        res = lambda items: self.send(200, {"page": pg, "total_pages": 1, "results": items if pg == 1 else []})
        if p.startswith("/3/trending/"):
            kind = {"movie": "movie", "tv": "tv"}.get(p.split("/")[3])
            return res([card(t) for t in pick(kind)])
        if p in ("/3/discover/movie", "/3/discover/tv"):
            kind = "movie" if p.endswith("movie") else "tv"
            g = int(q["with_genres"]) if "with_genres" in q else None
            y = int(q.get("primary_release_year") or q.get("first_air_date_year") or 0) or None
            items = pick(kind, g, y)
            if "primary_release_date.gte" in q or "first_air_date.gte" in q:
                items = [t for t in items if t[4] >= 2025]
            return res([card(t, False) for t in items])
        if p.startswith("/3/search/"):
            return res([card(t) for t in pick(None, None, None, q.get("query"))])
        if p.startswith("/3/tv/"):
            tid = int(p.split("/")[3])
            t = TVMAP.get(tid)
            if not t:
                return self.send(404, {})
            seasons = [{"season_number": 0, "episode_count": 2}] + [
                {"season_number": n, "episode_count": 6 + n} for n in range(1, SEASONS.get(tid, 1) + 1)]
            return self.send(200, {"id": tid, "name": t[3], "seasons": seasons,
                                   "external_ids": {"tvdb_id": t[2], "imdb_id": f"tt{tid}"}})
        self.send(404, {})

    # ---- Seerr ----
    def authed(self):
        return "connect.sid=mock" in (self.headers.get("Cookie") or "")

    def h_seerr(self, m, p, q):
        p = p.removeprefix("/api/v1")
        if p == "/status":
            return self.send(200, {"version": "3.5.2"})
        if p == "/settings/public":
            return self.send(200, {"initialized": True, "localLogin": True, "mediaServerLogin": True,
                                   "mediaServerType": 2, "partialRequestsEnabled": True, "applicationTitle": "Demo Seerr"})
        if p == "/auth/jellyfin" and m == "POST":
            return self.send(200, {"id": 2, "permissions": 32, "displayName": "demo"},
                             extra=[("Set-Cookie", "connect.sid=mock; Path=/; HttpOnly")])
        if not self.authed():
            return self.send(403, {"status": 403, "error": "You do not have permission to access this endpoint"})
        if p == "/auth/me":
            return self.send(200, {"id": 2, "permissions": 32, "displayName": "demo"})
        if p.endswith("/quota"):
            return self.send(200, {"movie": {"restricted": False, "used": 0}, "tv": {"restricted": False, "used": 0}})
        pg = int(q.get("page", 1))
        page = lambda items: self.send(200, {"page": pg, "totalPages": 1, "totalResults": len(items),
                                             "results": [seerr_hit(t) for t in items] if pg == 1 else []})
        if p == "/discover/trending":
            return page(TITLES)
        if p in ("/discover/movies", "/discover/tv"):
            kind = "movie" if p.endswith("movies") else "tv"
            g = int(q["genre"]) if "genre" in q else None
            y = None
            for k in ("primaryReleaseDateGte", "firstAirDateGte"):
                if k in q:
                    y = int(q[k][:4])
            return page(pick(kind, g, y))
        if p in ("/discover/movies/upcoming", "/discover/tv/upcoming"):
            return page([t for t in pick("movie" if "movies" in p else "tv") if t[4] >= 2025])
        if p == "/search":
            return page(pick(None, None, None, q.get("query")))
        if p.startswith("/movie/") or p.startswith("/tv/"):
            tid = int(p.split("/")[2])
            t = TVMAP.get(tid) or next(x for x in TITLES if x[1] == tid)
            d = card(t, False)
            d.update(posterPath=d.pop("poster_path"), overview=t[7])
            if t[0] == "tv":
                d["seasons"] = [{"seasonNumber": n, "episodeCount": 6 + n} for n in range(1, SEASONS.get(tid, 1) + 1)]
            reqs = [r for r in SEERR_REQUESTS if r["tmdb"] == tid]
            st = SEERR_STATE.get(tid)
            if reqs:
                st = 2
            if st:
                d["mediaInfo"] = {"tmdbId": tid, "status": st, "requests": [{"id": r["id"], "status": 1, "seasons": [
                    {"seasonNumber": s} for s in r["seasons"]]} for r in reqs], "seasons": []}
            return self.send(200, d)
        if p.startswith("/user/") and p.endswith("/requests"):
            return self.send(200, {"pageInfo": {"pages": 1, "pageSize": 50, "results": len(SEERR_REQUESTS), "page": 1},
                                   "results": [{"id": r["id"], "type": r["kind"], "status": 1, "createdAt": "2026-11-27T10:00:00.000Z",
                                                "seasons": [{"seasonNumber": s} for s in r["seasons"]],
                                                "media": {"tmdbId": r["tmdb"], "mediaType": r["kind"], "status": 2}}
                                               for r in SEERR_REQUESTS]})
        if p == "/request" and m == "POST":
            b = self.body()
            log("seerr-request-body", json.dumps(b))
            CURSOR["seerr_id"] += 1
            seasons = b.get("seasons") if isinstance(b.get("seasons"), list) else []
            SEERR_REQUESTS.append({"id": CURSOR["seerr_id"], "tmdb": b["mediaId"], "kind": b["mediaType"], "seasons": seasons})
            return self.send(201, {"id": CURSOR["seerr_id"], "type": b["mediaType"], "status": 1, "media": {"tmdbId": b["mediaId"]}})
        self.send(404, {})

    # ---- Radarr / Sonarr ----
    def arr(self, store, p, q, m):
        """Key check and the three read calls both services share; True when the request is answered."""
        if self.headers.get("X-Api-Key") != ARR_KEY:
            self.send(401, {"message": "Unauthorized"})
            return True
        p = p.removeprefix("/api/v3")
        if p == "/system/status":
            self.send(200, {"appName": "Radarr" if store is RADARR else "Sonarr", "version": "5.0.0-mock"})
        elif p == "/qualityprofile":
            self.send(200, [{"id": 1, "name": "Any"}, {"id": 4, "name": "HD - 1080p"}])
        elif p == "/rootfolder":
            self.send(200, [{"id": 1, "path": "/media/movies" if store is RADARR else "/media/tv"}])
        else:
            return False
        return True

    def phase(self, store, key):
        age = time.time() - store[key]
        return "queued" if age < 10 else "downloading" if age < 25 else "done"

    def h_radarr(self, m, p, q):
        if self.arr(RADARR, p, q, m):
            return
        p = p.removeprefix("/api/v3")
        if p == "/movie/lookup":
            term = q.get("term", "")
            items = [t for t in TITLES if t[0] == "movie" and (term.lower() in t[3].lower() or term == f"tmdb:{t[1]}")]
            return self.send(200, [{"title": t[3], "year": t[4], "tmdbId": t[1], "overview": t[7],
                                    "images": [{"coverType": "poster", "remoteUrl": f"http://127.0.0.1:{PORTS['tmdb']}/img/w342/p{t[1]}.png"}],
                                    "ratings": {"tmdb": {"value": t[5]}}} for t in items])
        if p == "/movie" and m == "GET":
            k = int(q.get("tmdbId", 0))
            if k in RADARR:
                return self.send(200, [{"id": 7, "hasFile": self.phase(RADARR, k) == "done"}])
            return self.send(200, [])
        if p == "/movie" and m == "POST":
            b = self.body()
            log("radarr-add-body", json.dumps(b))
            if b["tmdbId"] in RADARR:
                return self.send(400, [{"errorMessage": "This movie has already been added"}])
            RADARR[b["tmdbId"]] = time.time()
            return self.send(201, {"id": 7})
        if p == "/queue":
            recs = [{"movieId": 7, "status": self.phase(RADARR, k)} for k in RADARR if self.phase(RADARR, k) != "done"]
            return self.send(200, {"records": recs})
        self.send(404, {})

    def h_sonarr(self, m, p, q):
        if self.arr(SONARR, p, q, m):
            return
        p = p.removeprefix("/api/v3")
        if p == "/series/lookup":
            term = q.get("term", "")
            items = [t for t in TITLES if t[0] == "tv" and (term.lower() in t[3].lower() or term == f"tvdb:{t[2]}")]
            return self.send(200, [{"title": t[3], "year": t[4], "tvdbId": t[2], "tmdbId": t[1], "overview": t[7],
                                    "seasons": [{"seasonNumber": n, "monitored": True} for n in range(0, SEASONS[t[1]] + 1)],
                                    "images": [{"coverType": "poster", "remoteUrl": f"http://127.0.0.1:{PORTS['tmdb']}/img/w342/p{t[1]}.png"}],
                                    "ratings": {"value": t[5]}} for t in items])
        if p == "/series" and m == "GET":
            k = int(q.get("tvdbId", 0))
            if k in SONARR:
                return self.send(200, [{"id": 9, "statistics": {"episodeFileCount": 3 if self.phase(SONARR, k) == "done" else 0}}])
            return self.send(200, [])
        if p == "/series" and m == "POST":
            b = self.body()
            log("sonarr-add-body", json.dumps(b))
            if b["tvdbId"] in SONARR:
                return self.send(400, [{"errorMessage": "This series has already been added"}])
            SONARR[b["tvdbId"]] = time.time()
            return self.send(201, {"id": 9})
        if p == "/queue":
            recs = [{"seriesId": 9, "status": self.phase(SONARR, k)} for k in SONARR if self.phase(SONARR, k) != "done"]
            return self.send(200, {"records": recs})
        self.send(404, {})


def main():
    global LOG
    if "--log" in sys.argv:
        LOG = sys.argv[sys.argv.index("--log") + 1]
    servers = []
    for kind, port in PORTS.items():
        s = ThreadingHTTPServer(("127.0.0.1", port), H)
        s.kind = kind
        servers.append(s)
        threading.Thread(target=s.serve_forever, daemon=True).start()
    print("mock services on", {k: f"127.0.0.1:{v}" for k, v in PORTS.items()}, flush=True)
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
