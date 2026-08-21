#!/bin/sh
# Croniq installer
# Usage: curl -fsSL https://raw.githubusercontent.com/nuetzliches/croniq/main/install.sh | sh
#
# Options (env vars):
#   CRONIQ_VERSION   — specific version to install (default: latest)
#   INSTALL_DIR      — where to place binaries (default: /usr/local/bin)
#   CRONIQ_BINARIES  — space-separated list (default: "croniq-server croniq croniq-mcp")
#
# Flags (pass after `sh -s --` when piping from curl):
#   --insecure-skip-verify — proceed without SHA256 verification. Only for
#     environments where the SHA256SUMS file is unreachable or no sha256
#     tool exists; the downloaded binaries are NOT integrity-checked.

set -e

REPO="nuetzliches/croniq"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARIES="${CRONIQ_BINARIES:-croniq-server croniq croniq-mcp}"

SKIP_VERIFY=0
for arg in "$@"; do
  case "$arg" in
    --insecure-skip-verify) SKIP_VERIFY=1 ;;
    *)
      echo "Unknown option: $arg" >&2
      echo "Supported flags: --insecure-skip-verify" >&2
      exit 1
      ;;
  esac
done

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
  echo "Fetching latest release..."
  # Follow the redirect from /releases/latest — avoids GitHub API rate limits
  _url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest")
  CRONIQ_VERSION="${_url##*/v}"
  unset _url
fi

if [ -z "$CRONIQ_VERSION" ]; then
  echo "Failed to determine the latest Croniq version." >&2
  echo "Set CRONIQ_VERSION explicitly and retry." >&2
  exit 1
fi

# The redirect only yields a version if it landed on a `v<semver>` server tag.
# It can land elsewhere: the repository also publishes SDK releases (e.g.
# `python-sdk-v0.4.0`), and one of those standing as "Latest" leaves the cut
# above with nothing to strip — `$CRONIQ_VERSION` would then be the entire URL
# and the download below would 404 on a nonsensical path. Fail here, where the
# message can say what happened.
case "$CRONIQ_VERSION" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "Resolved an unexpected latest release: '${CRONIQ_VERSION}'." >&2
    echo "Set CRONIQ_VERSION to a server version (e.g. CRONIQ_VERSION=0.34.0) and retry." >&2
    exit 1
    ;;
esac

# ── Download + verify + extract ──────────────────────────────────────────────

ARCHIVE="croniq-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/v${CRONIQ_VERSION}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "Downloading Croniq v${CRONIQ_VERSION} for ${TARGET}..."
curl -fsSL --progress-bar "${BASE_URL}/${ARCHIVE}" -o "$TMP/$ARCHIVE"

# Verify the SHA256 checksum. Verification is fail-closed: a missing
# SHA256SUMS file or a missing sha256 tool aborts the install instead of
# continuing with an unverified binary. `--insecure-skip-verify` is the
# explicit escape hatch for the rare environment where that is acceptable
# (e.g. installing an old release from before SHA256SUMS was published).
if [ "$SKIP_VERIFY" = "1" ]; then
  echo "WARNING: --insecure-skip-verify given — installing WITHOUT checksum verification." >&2
else
  if command -v sha256sum >/dev/null 2>&1; then
    SHA256_CMD="sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    SHA256_CMD="shasum -a 256"
  else
    echo "Error: neither sha256sum nor shasum is available, so the download cannot be verified." >&2
    echo "Install one of them, or re-run with --insecure-skip-verify to proceed without verification:" >&2
    echo "  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh -s -- --insecure-skip-verify" >&2
    exit 1
  fi

  if ! curl -fsSL "${BASE_URL}/SHA256SUMS" -o "$TMP/SHA256SUMS" 2>/dev/null; then
    echo "Error: failed to fetch ${BASE_URL}/SHA256SUMS — refusing to install an unverified binary." >&2
    echo "Releases before the checksum file was published, or a network problem, can cause this." >&2
    echo "Re-run with --insecure-skip-verify to proceed without verification:" >&2
    echo "  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh -s -- --insecure-skip-verify" >&2
    exit 1
  fi

  echo "Verifying checksum..."
  # Run the check in a subshell so the working directory change is scoped
  (cd "$TMP" && grep "$ARCHIVE" SHA256SUMS | $SHA256_CMD --check -)
  echo "Checksum verified."
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
