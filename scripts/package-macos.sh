#!/usr/bin/env bash
# Build dist/NOCTURNE-<ver>-<arch>.dmg on a Mac.   Written without a Mac; its first run is the manual
# CI workflow (docs/PACKAGING.md). macOS cannot be built or tested from Linux, see docs/PLATFORMS.md.
#
#   usage: scripts/package-macos.sh
#   CI (.github/workflows/platforms.yml) runs it on a macOS runner, ad-hoc signed.
#   needs: Xcode command line tools (iconutil, sips, hdiutil, codesign, install_name_tool), Rust, and
#          `brew install mpv dylibbundler`; optional `brew install create-dmg`.
#   signing (optional, needs the maintainer's Apple Developer ID; without it the app is ad-hoc signed and Gatekeeper
#   asks the user to right-click > Open):
#     SIGN_ID="Developer ID Application: Name (TEAMID)"            codesign everything with the hardened runtime
#     NOTARY_PROFILE=name   (from `xcrun notarytool store-credentials`)   notarise and staple the .dmg
set -euo pipefail

if [ "$(uname -s)" != Darwin ]; then
  echo "package-macos.sh must run on macOS (Linux cannot build or test the app bundle); see docs/PACKAGING.md." >&2
  exit 1
fi
cd "$(dirname "$0")/.."
for t in dylibbundler iconutil sips hdiutil codesign cargo; do
  command -v "$t" >/dev/null || { echo "missing tool: $t (brew install mpv dylibbundler; Xcode command line tools)" >&2; exit 1; }
done
MPV_BIN=$(command -v mpv) || { echo "mpv not found: brew install mpv" >&2; exit 1; }
LIBMPV=$(brew --prefix mpv 2>/dev/null)/lib/libmpv.2.dylib
[ -f "$LIBMPV" ] || { echo "libmpv.2.dylib not found at $LIBMPV (brew install mpv)" >&2; exit 1; }

ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)                       # arm64 or x86_64; one dmg per architecture (Homebrew dylibs are single-arch)
app=dist/macos/NOCTURNE.app
dmg=dist/NOCTURNE-$ver-$arch.dmg
rm -rf dist/macos && mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"

# 1. binary
cargo build --release --locked -p nocturne-app
install -m755 target/release/nocturne "$app/Contents/MacOS/nocturne"
sed "s/@VERSION@/$ver/" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# 2. icon: packaging/icons/nocturne-512.png -> nocturne.icns (the .iconset sizes macOS expects)
set=dist/macos/nocturne.iconset; mkdir -p "$set"
for s in 16 32 128 256 512; do
  sips -z $s $s packaging/icons/nocturne-512.png --out "$set/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) packaging/icons/nocturne-512.png --out "$set/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o "$app/Contents/Resources/nocturne.icns"

# 3. mpv. The app loads Contents/Frameworks/libmpv.2.dylib (crates/player/src/mpv_ffi.rs) and starts Contents/MacOS/mpv
# for the external window (crates/player/src/deps.rs; macOS has no in-window video yet, so the binary is what plays).
# dylibbundler copies each dependency (ffmpeg, libass, ...) into Frameworks/ and rewrites install names to
# @executable_path/../Frameworks. Both are fixed in one run so shared dylibs are copied once.
# libmpv is staged OUTSIDE Frameworks/ because -od empties the destination directory first; it is moved in afterwards.
install -m755 "$MPV_BIN" "$app/Contents/MacOS/mpv"
stage=dist/macos/stage; mkdir -p "$stage"
cp "$LIBMPV" "$stage/libmpv.2.dylib"
chmod u+w "$stage/libmpv.2.dylib"
dylibbundler -od -b -cd \
  -x "$app/Contents/MacOS/mpv" -x "$stage/libmpv.2.dylib" \
  -d "$app/Contents/Frameworks" -p @executable_path/../Frameworks/ \
  -s "$(brew --prefix)/lib" -s "$(brew --prefix mpv)/lib"
mv "$stage/libmpv.2.dylib" "$app/Contents/Frameworks/libmpv.2.dylib"
# the dylib's own install name still points into Homebrew; the check below would (rightly) flag it
install_name_tool -id @executable_path/../Frameworks/libmpv.2.dylib "$app/Contents/Frameworks/libmpv.2.dylib"
# dylibbundler can add an LC_RPATH the binary already has; dyld on current macOS aborts on a duplicate
# ("duplicate LC_RPATH", found by the first CI run). Drop repeats so each rpath appears once.
for f in "$app/Contents/MacOS/"* "$app/Contents/Frameworks/"*.dylib; do
  otool -l "$f" | awk '/cmd LC_RPATH/{r=1} r&&/ path /{print $2; r=0}' | sort | uniq -d | while read -r rp; do
    while [ "$(otool -l "$f" | awk -v p="$rp" '/cmd LC_RPATH/{r=1} r&&/ path /{if($2==p)n++; r=0} END{print n+0}')" -gt 1 ]; do
      install_name_tool -delete_rpath "$rp" "$f"
    done
  done
done
# Fallback if dylibbundler fails: for each `otool -L` entry under the Homebrew prefix, copy it into Frameworks/ and run
#   install_name_tool -change <old> @executable_path/../Frameworks/<name> <binary>
# (and `-id @rpath/<name>` on the dylib itself), recursing through the copied dylibs.
otool -L "$app/Contents/MacOS/mpv" "$app/Contents/Frameworks/"*.dylib | grep -E "/opt/homebrew|/usr/local" \
  && { echo "unresolved Homebrew paths remain (list above); fix the bundling before shipping" >&2; exit 1; } || true

# 4. licences: GPL text, notices, and the source offer for the bundled mpv/ffmpeg (docs/PACKAGING.md, "macOS")
cp LICENSE THIRD_PARTY_NOTICES.md "$app/Contents/Resources/"
{ echo "Bundled mpv $(mpv --version | head -1) and libraries, from Homebrew (formula 'mpv'), GPL-2.0-or-later as built."
  echo "Corresponding source: https://github.com/mpv-player/mpv and the formula sources listed by 'brew info mpv'."
  echo "Dependency licences: run 'brew info' per library bundled in Contents/Frameworks."; } > "$app/Contents/Resources/BUNDLED_LIBRARIES.txt"

# 5. OPTIONAL signing. Inside-out: dylibs, then mpv, then the bundle. Ad-hoc ("-") when no Developer ID is given.
id=${SIGN_ID:--}
extra=(); [ -n "${SIGN_ID:-}" ] && extra=(--options runtime --timestamp)
for f in "$app/Contents/Frameworks/"*.dylib "$app/Contents/MacOS/mpv"; do codesign --force --sign "$id" ${extra[@]+"${extra[@]}"} "$f"; done
codesign --force --sign "$id" ${extra[@]+"${extra[@]}"} --entitlements packaging/macos/entitlements.plist "$app"
codesign --verify --deep --strict "$app"

# 6. dmg (with an /Applications link). create-dmg gives the drag-to-install window; hdiutil is the plain fallback.
rm -f "$dmg"
if [ -z "${CI:-}" ] && command -v create-dmg >/dev/null; then   # create-dmg drives Finder via AppleScript: not on headless CI
  create-dmg --volname NOCTURNE --app-drop-link 450 120 --icon NOCTURNE.app 150 120 "$dmg" "$app"
else
  dmgsrc=dist/macos/dmg; mkdir -p "$dmgsrc"; cp -R "$app" "$dmgsrc/"; ln -s /Applications "$dmgsrc/Applications"
  for _ in 1 2 3; do   # hdiutil sometimes fails with "Resource busy" on CI runners
    hdiutil create -volname NOCTURNE -srcfolder "$dmgsrc" -ov -format UDZO "$dmg" && break
    sleep 5
  done
  [ -f "$dmg" ] || { echo "hdiutil failed" >&2; exit 1; }
fi
[ -n "${SIGN_ID:-}" ] && codesign --force --sign "$SIGN_ID" --timestamp "$dmg"

# 7. OPTIONAL notarisation (Apple Developer Program, a stored notarytool profile). Without it, Gatekeeper warns.
if [ -n "${SIGN_ID:-}" ] && [ -n "${NOTARY_PROFILE:-}" ]; then
  xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$dmg"
fi
shasum -a 256 "$dmg"
