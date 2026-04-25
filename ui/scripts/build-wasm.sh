#!/usr/bin/env bash
#
# Build the croniq-config-wasm bridge and copy the artefacts into
# ui/src/lib/wasm/. Idempotent: skips the build if the wasm output is
# already newer than every Rust source file.
#
# Why no `wasm-pack build --out-dir ../../ui/src/lib/wasm`?
#   wasm-pack rewrites the entire out dir on every run, including the
#   `.gitignore` and `package.json` it drops in. Copying lets us keep
#   the rest of `ui/src/lib/` clean and gives us a single place to
#   delete if the bridge is ever ripped out.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CRATE_DIR="$ROOT/crates/croniq-config-wasm"
OUT_PKG="$CRATE_DIR/pkg"
UI_DEST="$ROOT/ui/src/lib/wasm"

# Up-to-date check: if every output file is newer than every input
# `.rs` (and Cargo.toml), do nothing. Skips a ~5 s wasm-pack call on
# `vite dev` reloads when only TypeScript changed.
needs_rebuild() {
    [ ! -f "$UI_DEST/croniq_config_wasm_bg.wasm" ] && return 0
    local newest_input
    newest_input=$(find "$CRATE_DIR/src" "$CRATE_DIR/Cargo.toml" "$ROOT/crates/croniq-config" \
        -name '*.rs' -o -name 'Cargo.toml' 2>/dev/null \
        | xargs -r ls -t 2>/dev/null | head -n 1 || true)
    [ -z "$newest_input" ] && return 0
    [ "$newest_input" -nt "$UI_DEST/croniq_config_wasm_bg.wasm" ] && return 0
    return 1
}

if ! needs_rebuild; then
    echo "wasm bridge: up-to-date — skipping rebuild"
    exit 0
fi

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "ERROR: wasm-pack not on PATH. Install with:"
    echo "    cargo install wasm-pack"
    echo "or see https://rustwasm.github.io/wasm-pack/installer/"
    exit 1
fi

echo "wasm bridge: building (wasm-pack build --target web --release)…"
( cd "$CRATE_DIR" && wasm-pack build --target web --release --out-dir pkg )

mkdir -p "$UI_DEST"
# Only the artefacts the UI actually loads — skip wasm-pack's own
# package.json + .gitignore + README.md. The .d.ts is needed for
# TypeScript editing; the .js is the loader; the .wasm is the binary.
cp "$OUT_PKG/croniq_config_wasm.js" \
   "$OUT_PKG/croniq_config_wasm.d.ts" \
   "$OUT_PKG/croniq_config_wasm_bg.wasm" \
   "$OUT_PKG/croniq_config_wasm_bg.wasm.d.ts" \
   "$UI_DEST/"

echo "wasm bridge: copied to ui/src/lib/wasm/"
