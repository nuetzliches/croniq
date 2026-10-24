#!/bin/sh
# Install a pinned, checksum-verified wasm-pack binary.
#
# The one place the wasm-pack version and its SHA-256 pins live. The
# Dockerfile's `wasm-builder` stage, ci.yml's `ui` and `e2e` jobs and
# pages.yml all call this script, so none of them pipes the upstream
# `installer/init.sh` into a shell any more. That was unpinned code fetched
# from the network, and in pages.yml it ran next to the OIDC token that
# deploys the site (review 2026-10-24, C-5).
#
# Usage: scripts/install-wasm-pack.sh [DEST_DIR]
#   DEST_DIR defaults to /usr/local/bin. Linux x86_64 / aarch64 only -- the
#   musl release tarballs are static, so they run on any glibc or musl host.
#
# When bumping WASM_PACK_VERSION, recompute both pins:
#   curl -fsSL <release tarball url> | sha256sum
set -eu

WASM_PACK_VERSION=0.13.1
WASM_PACK_SHA256_X86_64=c539d91ccab2591a7e975bcf82c82e1911b03335c80aa83d67ad25ed2ad06539
WASM_PACK_SHA256_AARCH64=2e65038769f8bbaa5fc237ad4bb523e692df99458cbd3e3d92525b89d8762379

dest="${1:-/usr/local/bin}"

if [ "$(uname -s)" != "Linux" ]; then
  echo "install-wasm-pack.sh: only Linux is supported (got $(uname -s))" >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64)        arch=x86_64;  sha256="$WASM_PACK_SHA256_X86_64" ;;
  aarch64|arm64) arch=aarch64; sha256="$WASM_PACK_SHA256_AARCH64" ;;
  *) echo "install-wasm-pack.sh: unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

url="https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/wasm-pack-v${WASM_PACK_VERSION}-${arch}-unknown-linux-musl.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "$url" -o "$tmp/wasm-pack.tar.gz"
# Verified before extraction, so a tampered artefact fails here instead of
# running.
echo "${sha256}  $tmp/wasm-pack.tar.gz" | sha256sum -c -
mkdir -p "$dest"
tar xzf "$tmp/wasm-pack.tar.gz" -C "$dest" --strip-components=1 --wildcards '*/wasm-pack'
"$dest/wasm-pack" --version
