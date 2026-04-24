#!/bin/sh
# Croniq installer
# Usage: curl -fsSL https://raw.githubusercontent.com/nuetzliches/croniq/main/install.sh | sh
#
# Options (env vars):
#   CRONIQ_VERSION   — specific version to install (default: latest)
#   INSTALL_DIR      — where to place binaries (default: /usr/local/bin)
#   CRONIQ_BINARIES  — space-separated list (default: "croniq-server croniq croniq-mcp")

set -e

REPO="nuetzliches/croniq"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARIES="${CRONIQ_BINARIES:-croniq-server croniq croniq-mcp}"

# ── Detect OS + architecture ─────────────────────────────────────────────────

OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)  os_part="unknown-linux-gnu" ;;
  Darwin) os_part="apple-darwin" ;;
  *)
    echo "Unsupported operating system: $OS" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64)          arch_part="x86_64" ;;
  arm64|aarch64)   arch_part="aarch64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

TARGET="${arch_part}-${os_part}"

# ── Resolve version ──────────────────────────────────────────────────────────

if [ -z "$CRONIQ_VERSION" ]; then
  echo "Fetching latest release info..."
  CRONIQ_VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | head -1 \
    | sed 's/.*"v\([^"]*\)".*/\1/')
fi

if [ -z "$CRONIQ_VERSION" ]; then
  echo "Failed to determine the latest Croniq version." >&2
  echo "Set CRONIQ_VERSION explicitly and retry." >&2
  exit 1
fi

# ── Download + verify + extract ──────────────────────────────────────────────

ARCHIVE="croniq-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/v${CRONIQ_VERSION}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "Downloading Croniq v${CRONIQ_VERSION} for ${TARGET}..."
curl -fsSL --progress-bar "${BASE_URL}/${ARCHIVE}" -o "$TMP/$ARCHIVE"

# Verify SHA256 checksum when a suitable tool is available
if command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
else
  SHA256_CMD=""
fi

if [ -n "$SHA256_CMD" ]; then
  echo "Verifying checksum..."
  curl -fsSL "${BASE_URL}/SHA256SUMS" -o "$TMP/SHA256SUMS"
  # Run the check in a subshell so the working directory change is scoped
  (cd "$TMP" && grep "$ARCHIVE" SHA256SUMS | $SHA256_CMD --check -)
  echo "Checksum verified."
else
  echo "Warning: sha256sum/shasum not found — skipping checksum verification." >&2
fi

tar -xzf "$TMP/$ARCHIVE" -C "$TMP"

# ── Install binaries ─────────────────────────────────────────────────────────

need_sudo=""
if [ ! -w "$INSTALL_DIR" ]; then
  if command -v sudo >/dev/null 2>&1; then
    need_sudo="sudo"
  else
    echo "Cannot write to $INSTALL_DIR and sudo is not available." >&2
    echo "Run as root or set INSTALL_DIR to a writable directory." >&2
    exit 1
  fi
fi

$need_sudo mkdir -p "$INSTALL_DIR"

installed=""
for bin in $BINARIES; do
  if [ -f "$TMP/$bin" ]; then
    $need_sudo install -m 755 "$TMP/$bin" "$INSTALL_DIR/$bin"
    installed="$installed $bin"
  fi
done

# ── Done ─────────────────────────────────────────────────────────────────────

echo ""
echo "Croniq v${CRONIQ_VERSION} installed to ${INSTALL_DIR}:"
for bin in $installed; do
  echo "  ✓ $bin"
done
echo ""
echo "Quick start:"
echo "  croniq-server --help"
echo "  croniq --help"
echo ""
echo "Full documentation: https://github.com/${REPO}"
