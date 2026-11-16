#!/usr/bin/env python3
"""Regenerate THIRD_PARTY_NOTICES.md body from `cargo tree` + `cargo metadata`.

Usage: [PKG=nocturne-ui] scripts/gen-notices.py > THIRD_PARTY_NOTICES.md   (run from the repo root)
PKG is the shipped app package (default nocturne-app; same variable as scripts/package.sh). Statements that only
hold for some dependency sets (GPUI, vendored xattr, self_cell, option-ext) are emitted only when that crate is
in the shipped tree, so the cut-over needs no edit here.
Lists every crate compiled into the `nocturne` binary (normal deps, linux target),
grouped by SPDX expression, then every distinct license/notice file text once
with the crates that carry it. Review the output; do not paste blindly.
"""

import hashlib, json, os, re, subprocess, sys
from collections import defaultdict

def run(*a):
    return subprocess.run(a, check=True, capture_output=True, text=True).stdout

PKG = os.environ.get("PKG", "nocturne-app")
tgt = "x86_64-unknown-linux-gnu"
tree = run("cargo", "tree", "-p", PKG, "-e", "normal", "--prefix", "none", "--target", tgt, "--format", "{p}")
shipped = set()
for line in tree.splitlines():
    m = re.match(r"(\S+) v(\S+)", line)
    if m:
        shipped.add((m[1], m[2].split(" ")[0]))
names = {n for n, _ in shipped}
has = lambda *ns: any(n in names or any(x.startswith(n) for x in names) for n in ns)

INTRO = """# Third-party notices

Copyright (C) 2026 ifsvivek

NOCTURNE is licensed under the GNU General Public License, version 3 or (at your option) any later version
(`GPL-3.0-or-later`, see `LICENSE`). It is an independent project, not affiliated with or endorsed by the
Jellyfin project. This file lists the third-party software the released `nocturne` binary contains or loads,
and carries the license and copyright notices those licenses require. Regenerate the crate sections with
`scripts/gen-notices.py` after any dependency change.

## Compatibility summary

Audit date 2026-11-16, over the normal (non-dev, non-build) dependency tree of the shipped package for
`x86_64-unknown-linux-gnu`. `cargo deny check licenses` passes. No GPL-incompatible license was found: no
GPL-2.0-only, proprietary, SSPL, or unknown license""" + (
    ", and no GPL-licensed Zed crate (every GPUI crate in the tree, `gpui` and `gpui_*`, is `Apache-2.0`; the `zed-*` "
    "forks of font-kit, reqwest, xim and async-tar are MIT or MIT/Apache-2.0)" if has("gpui") else "") + """.

Points that needed a decision:

""" + (
    """- `libmpv2` and `libmpv2-sys` (Rust bindings, compiled into the binary) are `LGPL-2.1`. A GPLv3+
  work may use and distribute them under LGPL-2.1 section 6 (work that uses a library), which allows any license
  that permits modification for the user's own use and reverse engineering for debugging; GPL-3.0 does. The
  complete corresponding source is the pinned crates.io release (`Cargo.lock`) plus this repository, so a user
  can rebuild and relink. The LGPL-2.1 text is reproduced below.
""" if has("libmpv2") else "- libmpv is not linked: it is loaded at run time (see below), so no libmpv code is in the binary.\n") + """"""
if has("self_cell"):
    INTRO += ("- `self_cell` is `Apache-2.0 OR GPL-2.0-only`. We take it under `Apache-2.0` (GPL-2.0-only is not "
              "compatible with GPLv3).\n")
if has("option-ext"):
    INTRO += ("- `option-ext` is `MPL-2.0` (file-level copyleft, unmodified, text reproduced below; compatible with "
              "GPLv3 via MPL-2.0 section 3.3).\n")
INTRO += ("- Apache-2.0 code is combined under GPLv3 (compatible; not with GPLv2). Any Apache `NOTICE` file shipped by "
          "a crate is reproduced below by the generator.\n")
if has("xattr"):
    INTRO += ("- `vendor/xattr` is xattr 0.2.3 (`MIT OR Apache-2.0`) with a one-line patch (`ENOATTR` replaced by "
              "`ENODATA`) so it builds against current libc; its two license files are in `vendor/xattr/` and "
              "reproduced below.\n")

GPU = "UI rendering (wgpu)" if has("wgpu") else "UI rendering"
INTRO += f"""
## System libraries (loaded at run time, not distributed with NOCTURNE)

| Library | Needed for | License of the Arch Linux build |
|---|---|---|
| libmpv.so.2 (mpv 0.41.0) | playback (loaded at run time with dlopen) | GPL-2.0-or-later AND LGPL-2.1-or-later (a GPL-enabled build makes the loaded library GPL-2.0-or-later, which is compatible with GPLv3 or later) |
| libvulkan (vulkan-icd-loader) + a Vulkan driver | {GPU} | Apache-2.0 / driver specific |
| libEGL (libglvnd) | video embedding, GL fallback | BSD-like (MIT) |
| libxkbcommon, libxkbcommon-x11, libxcb, libX11, libXcursor, libXi, libwayland-client, libwayland-egl, libdbus-1 | windowing, input, keyring | MIT / permissive / AFL-or-GPL (dbus) |
| glibc, libgcc_s | runtime | LGPL-2.1+ / GPL-3.0+ with GCC runtime exception |

If a distribution builds libmpv as GPL-2.0-only (not the case on Arch) the combination would not be redistributable;
distribution packages declare mpv as an external dependency, so nothing here is relicensed by NOCTURNE.

## Fonts, icons and artwork

- No font files are bundled. The UI asks the system for "Noto Sans" (SIL OFL-1.1) by name and falls back to
  whatever fontconfig provides.
- The UI icons are plain geometric glyphs authored in this repository (GPL-3.0-or-later like the rest of the
  code). `packaging/nocturne.svg` and the PNGs derived from it are an original mark (crescent and play
  triangle), GPL-3.0-or-later. The Jellyfin name and logo are not used.

"""
HEADER = INTRO

meta = json.loads(run("cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", tgt))
pk = {(p["name"], p["version"]): p for p in meta["packages"]}

def norm(e):
    return (e or "UNKNOWN").replace("/", " OR ").replace(" / ", " OR ")

groups = defaultdict(list)
texts = defaultdict(lambda: [None, []])
for key in sorted(shipped):
    p = pk[key]
    if p["source"] is None:  # workspace crates: GPL-3.0-or-later (this project)
        continue
    groups[norm(p["license"])].append(f"{key[0]} {key[1]}")
    d = os.path.dirname(p["manifest_path"])
    files = sorted(f for f in os.listdir(d) if re.match(r"(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE)", f, re.I)
                   and os.path.isfile(os.path.join(d, f)))
    if not files:
        texts["(no license file shipped in the crate; see its repository " + str(p["repository"]) + ")"][1].append(f"{key[0]} {key[1]}")
        continue
    for f in files:
        body = open(os.path.join(d, f), errors="replace").read().strip()
        h = hashlib.sha256(re.sub(r"\s+", " ", body).encode()).hexdigest()
        texts[h][0] = body
        texts[h][1].append(f"{key[0]} {key[1]}")

out = [HEADER, "## Rust crates compiled into the binary\n",
       f"{sum(len(c) for c in groups.values())} third-party crates (generated by `scripts/gen-notices.py`).\n",
       "### By license expression\n", "| License expression | Crates |", "|---|---|"]
for e, c in sorted(groups.items(), key=lambda x: -len(x[1])):
    out.append(f"| `{e}` | {len(c)} |")
out += ["", "### Crate list\n"]
for e, c in sorted(groups.items()):
    out += [f"**{e}**", "", ", ".join(c), ""]
out += ["## License and notice texts\n",
        "Each distinct text is printed once, followed by the crates it applies to.\n"]
for h, (body, crates) in sorted(texts.items(), key=lambda x: sorted(x[1][1])[0]):
    if body is None:
        out += [f"### {h}", "", "Crates: " + ", ".join(crates), ""]
        continue
    out += ["---", "", "Crates: " + ", ".join(crates), "", "```text", body, "```", ""]
print("\n".join(out))
