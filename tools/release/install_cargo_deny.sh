#!/usr/bin/env bash
# Install the pinned cargo-deny release binary after verifying its SHA-256 (F-23).
# Usage: tools/release/install_cargo_deny.sh DEST_DIR   (Linux x86_64)
set -euo pipefail
VERSION=0.20.2
SHA256=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
dest="${1:?usage: install_cargo_deny.sh DEST_DIR}"
name="cargo-deny-$VERSION-x86_64-unknown-linux-musl"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 3 -o "$work/$name.tar.gz" \
  "https://github.com/EmbarkStudios/cargo-deny/releases/download/$VERSION/$name.tar.gz"
echo "$SHA256  $work/$name.tar.gz" | sha256sum -c -
tar -xzf "$work/$name.tar.gz" -C "$work"
mkdir -p "$dest"
install -m 0755 "$work/$name/cargo-deny" "$dest/cargo-deny"
"$dest/cargo-deny" --version
