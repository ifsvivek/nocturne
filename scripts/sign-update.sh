#!/bin/sh
# Maintainer-only release step: sign update artifacts with the ed25519 update key.
#   scripts/sign-update.sh <artifact>...      writes <artifact>.sig (base64 of the raw 64-byte signature)
#   env: UPDATE_KEY (default keys/nocturne-update-key.pem)
# The signature is over the artifact bytes, exactly what crates/core/src/update_install.rs verifies against the public key
# in packaging/update/nocturne-update-key.pub. The private key never leaves this machine: not CI, not the repository.
set -eu
key="${UPDATE_KEY:-keys/nocturne-update-key.pem}"
[ $# -gt 0 ] || { echo "usage: $0 <artifact>..." >&2; exit 2; }
[ -f "$key" ] || { echo "error: update key not found: $key" >&2; exit 1; }
for a in "$@"; do
  [ -f "$a" ] || { echo "error: no such file: $a" >&2; exit 1; }
  openssl pkeyutl -sign -rawin -inkey "$key" -in "$a" -out "$a.sig.bin"
  base64 -w0 < "$a.sig.bin" > "$a.sig"
  rm -f "$a.sig.bin"
  echo "signed $a"
done
