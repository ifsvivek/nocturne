#!/usr/bin/env bash
# Build the signed apt and dnf repositories next to the pacman one: $OUT/deb/ (dists/stable/main/binary-amd64 + pool/) and $OUT/rpm/ (repodata/ + the rpm).
# Called by scripts/publish-repo.sh; usable alone.   usage: [OUT=website/static/repo] [PROFILE=release|fast-release] [SKIP_BUILD=1] scripts/publish-repo-deb-rpm.sh
# Needs docker and gpg with the signing key in the keyring. Repository metadata is generated in containers (apt-ftparchive, createrepo_c, rpmsign); signing
# happens with the host gpg: the apt Release and repomd.xml are signed on the host, the .rpm inside a container that reaches the host gpg-agent through its
# restricted "extra" socket, so the secret key never leaves the host keyring. SKIP_BUILD=1 packages nothing and uses what is already in dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
FPR=5204B42AF768DE210D4979E397FA02EFCF1A4E3B
OUT=$(realpath -m "${OUT:-website/static/repo}"); PROFILE="${PROFILE:-release}"; export PROFILE
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
gpg --list-secret-keys "$FPR" >/dev/null 2>&1 || { echo "signing key $FPR not in this keyring" >&2; exit 1; }
if [ -z "${SKIP_BUILD:-}" ]; then scripts/package.sh deb; scripts/package.sh rpm; fi
deb=$(ls dist/nocturne_"$ver"*_amd64.deb | sort -V | tail -1); rpm=$(ls dist/nocturne-"$ver"-*.x86_64.rpm | sort -V | tail -1)

# Small local images with the repo tools (built once).
docker image inspect nocturne-build:debian-repo >/dev/null 2>&1 || docker build -q -t nocturne-build:debian-repo - >/dev/null <<'D'
FROM debian:stable
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends apt-utils && rm -rf /var/lib/apt/lists/*
D
docker image inspect nocturne-build:fedora-repo >/dev/null 2>&1 || docker build -q -t nocturne-build:fedora-repo - >/dev/null <<'D'
FROM fedora:latest
RUN dnf install -y -q createrepo_c rpm-sign gnupg2 && dnf clean all
D

mkdir -p "$OUT"
cp packaging/repo/ARCH-GPG-KEY "$OUT/NOCTURNE-GPG-KEY"          # armored: dnf gpgkey=
gpg --batch --yes --export "$FPR" > "$OUT/NOCTURNE-GPG-KEY.gpg"   # binary: apt signed-by=

# ---- apt ----
d="$OUT/deb"; pool="$d/pool/main/n/nocturne"
mkdir -p "$pool" "$d/dists/stable/main/binary-amd64"
cp "$deb" "$pool/"
ls -1 "$pool"/nocturne_*_amd64.deb | sort -V | head -n -2 | xargs -r rm -f   # keep the newest two versions
docker run --rm --user "$(id -u):$(id -g)" -v "$d":/r -w /r nocturne-build:debian-repo bash -euc '
  apt-ftparchive packages pool > dists/stable/main/binary-amd64/Packages
  gzip -9nf -k dists/stable/main/binary-amd64/Packages
  apt-ftparchive -o APT::FTPArchive::Release::Origin=NOCTURNE -o APT::FTPArchive::Release::Label=NOCTURNE \
    -o APT::FTPArchive::Release::Suite=stable -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 -o APT::FTPArchive::Release::Components=main \
    release dists/stable > dists/stable/Release'
rel="$d/dists/stable"; rm -f "$rel/InRelease" "$rel/Release.gpg"
gpg --batch --yes --use-agent -u "$FPR" --clearsign -o "$rel/InRelease" "$rel/Release"
gpg --batch --yes --use-agent -u "$FPR" --detach-sign --armor -o "$rel/Release.gpg" "$rel/Release"

# ---- dnf ----
r="$OUT/rpm"; mkdir -p "$r"
cp "$rpm" "$r/"
ls -1 "$r"/nocturne-*.rpm | sort -V | head -n -2 | xargs -r rm -f
sock=$(gpgconf --list-dirs agent-extra-socket)
docker run --rm -v "$r":/r -v "$sock":/root/.gnupg/S.gpg-agent -v "$PWD/packaging/repo/ARCH-GPG-KEY":/key:ro -e FPR="$FPR" -e UID_="$(id -u):$(id -g)" nocturne-build:fedora-repo bash -euc '
  gpg --batch --import /key 2>/dev/null
  rpmsign --define "_gpg_name $FPR" --addsign /r/nocturne-*.rpm
  rm -rf /r/repodata
  createrepo_c --quiet --general-compress-type gz /r
  chown -R "$UID_" /r'
gpg --batch --yes --use-agent -u "$FPR" --detach-sign --armor -o "$r/repodata/repomd.xml.asc" "$r/repodata/repomd.xml"
cat > "$r/nocturne.repo" <<'EOT'
[nocturne]
name=NOCTURNE
baseurl=https://nocturne.ifsvivek.in/repo/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://nocturne.ifsvivek.in/repo/NOCTURNE-GPG-KEY
EOT

# ---- verify ----
good() { gpg --verify --status-fd 1 "$@" 2>/dev/null | grep -q "VALIDSIG $FPR" || { echo "BAD signature: $*" >&2; exit 1; }; }
good "$rel/InRelease"; good "$rel/Release.gpg" "$rel/Release"; good "$r/repodata/repomd.xml.asc" "$r/repodata/repomd.xml"
grep -q "^Filename: pool/main/n/nocturne/$(basename "$deb")" "$d/dists/stable/main/binary-amd64/Packages" || { echo "Packages lacks $(basename "$deb")" >&2; exit 1; }
docker run --rm -v "$r":/r:ro -v "$PWD/packaging/repo/ARCH-GPG-KEY":/key:ro nocturne-build:fedora-repo bash -euc '
  rpm --import /key; rpm -K /r/nocturne-*.rpm | tee /dev/stderr | grep -q "signatures OK"'
big=$(find "$d" "$r" -type f -size +25M); [ -z "$big" ] || { echo "over the 25 MiB per-file limit of the Worker: $big" >&2; exit 1; }
du -h "$pool"/*.deb "$r"/*.rpm
echo "OK: apt $d, dnf $r. Deploy the website to publish."
