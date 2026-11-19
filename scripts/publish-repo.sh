#!/usr/bin/env bash
# Build the Arch package in a clean container and (re)generate the signed pacman repository under
# website/static/repo/ (generated, git-ignored; shipped by deploying the website).   usage: scripts/publish-repo.sh [VERSION]
# VERSION must match pkgver in packaging/PKGBUILD (its sha256 is for that tag's source archive). Needs docker, gpg, repo-add. Also builds the apt and dnf repositories (scripts/publish-repo-deb-rpm.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/cache-budget.sh --quiet || true
FPR=5204B42AF768DE210D4979E397FA02EFCF1A4E3B
ver=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
pkgver=$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)
[ "$ver" = "$pkgver" ] || { echo "VERSION $ver != PKGBUILD pkgver $pkgver: update packaging/PKGBUILD (version + sha256) first" >&2; exit 1; }
gpg --list-secret-keys "$FPR" >/dev/null 2>&1 || { echo "signing key $FPR not in this keyring" >&2; exit 1; }

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
cp packaging/PKGBUILD "$work/"
chmod 777 "$work"
docker run --rm -v "$work:/build" archlinux:base-devel bash -euxc '
  pacman -Syu --noconfirm
  cd /build
  # runtime + build deps as root; any vulkan-driver satisfies the dependency in the container
  deps=$(bash -c "source PKGBUILD; printf \"%s\n\" \"\${depends[@]}\" \"\${makedepends[@]}\"" | sed "s/^vulkan-driver\$/vulkan-swrast/")
  pacman -S --noconfirm --needed $deps
  useradd -m builder
  su builder -c "cd /build && makepkg --noconfirm --nocheck"
'
pkg=$(ls "$work"/nocturne-"$ver"-*-x86_64.pkg.tar.zst)

repo=website/static/repo/arch/x86_64
mkdir -p "$repo"
cp "$pkg" "$repo/"
cd "$repo"
p=$(basename "$pkg")
gpg --batch --yes --use-agent -u "$FPR" --detach-sign --no-armor "$p"
repo-add --sign --key "$FPR" --remove nocturne.db.tar.gz "$p"
# keep the newest two package versions
ls -1 nocturne-*.pkg.tar.zst | sort -V | head -n -2 | while read -r old; do rm -f "$old" "$old.sig"; done
# static hosting cannot serve symlinks
for n in db files; do rm -f "nocturne.$n" "nocturne.$n.sig"; cp "nocturne.$n.tar.gz" "nocturne.$n"; cp "nocturne.$n.tar.gz.sig" "nocturne.$n.sig"; done
cd - >/dev/null
cp packaging/repo/ARCH-GPG-KEY website/static/repo/ARCH-GPG-KEY
cat > website/static/repo/index.txt <<EOT
NOCTURNE pacman repository (Arch Linux, x86_64)
Signing key fingerprint: $FPR

curl -fsSL https://nocturne.ifsvivek.in/repo/ARCH-GPG-KEY | sudo pacman-key --add -
sudo pacman-key --lsign-key $FPR

Add to /etc/pacman.conf:
[nocturne]
SigLevel = Required TrustedOnly
Server = https://nocturne.ifsvivek.in/repo/arch/\$arch

Then: sudo pacman -Syu nocturne
EOT

# apt (deb/) and dnf (rpm/) repositories from the same key; SKIP_DEB_RPM=1 leaves them out
[ -n "${SKIP_DEB_RPM:-}" ] || scripts/publish-repo-deb-rpm.sh

# verify
cd "$repo"
for f in nocturne-*.pkg.tar.zst nocturne.db.tar.gz nocturne.files.tar.gz nocturne.db nocturne.files; do
  gpg --verify "$f.sig" "$f" 2>&1 | grep -q "Good signature" || { echo "BAD signature: $f" >&2; exit 1; }
  gpg --verify --status-fd 1 "$f.sig" "$f" 2>/dev/null | grep -q "VALIDSIG $FPR" || { echo "wrong key: $f" >&2; exit 1; }
done
tar -tzf nocturne.db | grep -q "^nocturne-$ver-" || { echo "db lacks nocturne-$ver" >&2; exit 1; }
du -h nocturne-*.pkg.tar.zst
echo "OK. Next: deploy the website (website/README.md) to publish https://nocturne.ifsvivek.in/repo/arch/x86_64/"
