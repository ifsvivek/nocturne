#!/usr/bin/env python3
"""Validate themes/<id>/ and publish the gallery: website/static/themes/index.json plus a copy of each theme's files.

usage: scripts/themes-index.py [--check]
  (none)   rewrite website/static/themes/ from themes/
  --check  fail if website/static/themes/ is not what themes/ would produce (CI, before a deploy)

Each folder needs theme.css (at most 256 KB), theme.toml and preview.webp (1280 px wide, under 150 KB). The index holds relative addresses (`<id>/theme.css`); the app resolves them against the index address and refuses other hosts.
"""
import hashlib, json, pathlib, re, shutil, sys, tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC, OUT = ROOT / "themes", ROOT / "website/static/themes"
KINDS = {"official", "community"}
# GPL-3.0-or-later compatible licences; extend by pull request.
LICENCES = {"GPL-3.0-or-later", "GPL-3.0-only", "LGPL-3.0-or-later", "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MPL-2.0", "CC0-1.0", "Unlicense"}
MAX_CSS, MAX_PREVIEW, PREVIEW_W = 256 * 1024, 150 * 1024, 1280


def fail(msg):
    sys.exit(f"themes-index: {msg}")


def webp_width(b):
    from PIL import Image
    import io
    return Image.open(io.BytesIO(b)).size[0]


def build():
    entries, files = [], {}
    for d in sorted(p for p in SRC.iterdir() if p.is_dir()):
        id_ = d.name
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,39}", id_):
            fail(f"{id_}: the folder name must be lowercase letters, digits and dashes")
        for f in ("theme.css", "theme.toml", "preview.webp"):
            if not (d / f).is_file():
                fail(f"{id_}: {f} is missing")
        meta = tomllib.loads((d / "theme.toml").read_text())
        for k in ("name", "author", "licence", "description", "kind", "min_version"):
            if not isinstance(meta.get(k), str) or not meta[k].strip():
                fail(f"{id_}: theme.toml needs `{k}`")
        if meta["kind"] not in KINDS:
            fail(f"{id_}: kind must be official or community")
        if meta["licence"] not in LICENCES:
            fail(f"{id_}: licence {meta['licence']!r} is not an accepted SPDX identifier")
        if not re.fullmatch(r"\d+\.\d+\.\d+", meta["min_version"]):
            fail(f"{id_}: min_version must look like 0.5.2")
        if len(meta["name"]) > 48 or len(meta["description"]) > 200:
            fail(f"{id_}: name is at most 48 and description 200 characters")
        css, prev = (d / "theme.css").read_bytes(), (d / "preview.webp").read_bytes()
        if len(css) > MAX_CSS:
            fail(f"{id_}: theme.css is over {MAX_CSS // 1024} KB")
        if len(prev) > MAX_PREVIEW:
            fail(f"{id_}: preview.webp is over {MAX_PREVIEW // 1024} KB")
        if prev[:4] != b"RIFF" or prev[8:12] != b"WEBP" or webp_width(prev) != PREVIEW_W:
            fail(f"{id_}: preview.webp must be a WebP {PREVIEW_W} px wide")
        entries.append({
            "id": id_, "name": meta["name"], "description": meta["description"], "author": meta["author"],
            "licence": meta["licence"], "kind": meta["kind"], "min_version": meta["min_version"],
            "css": f"{id_}/theme.css", "preview": f"{id_}/preview.webp", "size": len(css),
            "sha256": hashlib.sha256(css).hexdigest(),
        })
        files[f"{id_}/theme.css"], files[f"{id_}/preview.webp"] = css, prev
    index = json.dumps({"version": 1, "themes": entries}, indent=2) + "\n"
    files["index.json"] = index.encode()
    return files


def main():
    files = build()
    if sys.argv[1:] == ["--check"]:
        have = {str(p.relative_to(OUT)) for p in OUT.rglob("*") if p.is_file()} if OUT.exists() else set()
        if have != set(files) or any((OUT / k).read_bytes() != v for k, v in files.items()):
            fail("website/static/themes is out of date: run scripts/themes-index.py")
        print(f"themes-index: {len(files) // 2} themes, up to date")
        return
    if OUT.exists():
        shutil.rmtree(OUT)
    for k, v in files.items():
        (OUT / k).parent.mkdir(parents=True, exist_ok=True)
        (OUT / k).write_bytes(v)
    print(f"themes-index: wrote {len(files) // 2} themes to {OUT.relative_to(ROOT)}")


main()
