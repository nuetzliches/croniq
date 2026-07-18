#!/usr/bin/env node
//
// Build the croniq-config-wasm bridge and copy the artefacts into
// ui/src/lib/wasm/. Idempotent: skips the build if the wasm output is
// already newer than every Rust source file.
//
// Node instead of bash so the npm hooks work from any shell: on Windows
// npm runs scripts through cmd.exe, where `bash` resolves to the WSL
// shim in System32 and fails without a configured distro.
//
// Why no `wasm-pack build --out-dir ../../ui/src/lib/wasm`?
//   wasm-pack rewrites the entire out dir on every run, including the
//   `.gitignore` and `package.json` it drops in. Copying lets us keep
//   the rest of `ui/src/lib/` clean and gives us a single place to
//   delete if the bridge is ever ripped out.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const CRATE_DIR = path.join(ROOT, "crates", "croniq-config-wasm");
const OUT_PKG = path.join(CRATE_DIR, "pkg");
const UI_DEST = path.join(ROOT, "ui", "src", "lib", "wasm");

function mtimeOrNull(file) {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return null;
  }
}

// Newest mtime among all `*.rs` / `Cargo.toml` files under the given
// roots (files or directories; missing paths are ignored).
function newestInputMtime(roots) {
  let newest = null;
  for (const root of roots) {
    let stat;
    try {
      stat = fs.statSync(root);
    } catch {
      continue;
    }
    const files = stat.isDirectory()
      ? fs
          .readdirSync(root, { recursive: true })
          .filter((f) => f.endsWith(".rs") || path.basename(f) === "Cargo.toml")
          .map((f) => path.join(root, f))
      : [root];
    for (const file of files) {
      const mtime = mtimeOrNull(file);
      if (mtime !== null && (newest === null || mtime > newest)) newest = mtime;
    }
  }
  return newest;
}

// Up-to-date check: if every output file is newer than every input
// `.rs` (and Cargo.toml), do nothing. Skips a ~5 s wasm-pack call on
// `vite dev` reloads when only TypeScript changed.
function needsRebuild() {
  const outMtime = mtimeOrNull(path.join(UI_DEST, "croniq_config_wasm_bg.wasm"));
  if (outMtime === null) return true;
  const newestInput = newestInputMtime([
    path.join(CRATE_DIR, "src"),
    path.join(CRATE_DIR, "Cargo.toml"),
    path.join(ROOT, "crates", "croniq-config"),
  ]);
  // No source tree to compare against (e.g. Docker stage with only
  // `ui/` mounted, where the WASM artefacts were copied in from an
  // earlier stage) — trust the existing artefact and skip the
  // rebuild. Without this guard the script forces a rebuild and then
  // fails on the missing wasm-pack a few lines down.
  if (newestInput === null) return false;
  return newestInput > outMtime;
}

if (!needsRebuild()) {
  console.log("wasm bridge: up-to-date — skipping rebuild");
  process.exit(0);
}

// No `shell: true` needed: wasm-pack ships as a native binary
// (wasm-pack.exe on Windows), which spawnSync resolves via PATH itself.
const probe = spawnSync("wasm-pack", ["--version"], { stdio: "ignore" });
if (probe.error || probe.status !== 0) {
  console.error("ERROR: wasm-pack not on PATH. Install with:");
  console.error("    cargo install wasm-pack");
  console.error("or see https://rustwasm.github.io/wasm-pack/installer/");
  process.exit(1);
}

console.log("wasm bridge: building (wasm-pack build --target web --release)…");
const build = spawnSync("wasm-pack", ["build", "--target", "web", "--release", "--out-dir", "pkg"], {
  cwd: CRATE_DIR,
  stdio: "inherit",
});
if (build.status !== 0) process.exit(build.status ?? 1);

fs.mkdirSync(UI_DEST, { recursive: true });
// Only the artefacts the UI actually loads — skip wasm-pack's own
// package.json + .gitignore + README.md. The .d.ts is needed for
// TypeScript editing; the .js is the loader; the .wasm is the binary.
for (const file of [
  "croniq_config_wasm.js",
  "croniq_config_wasm.d.ts",
  "croniq_config_wasm_bg.wasm",
  "croniq_config_wasm_bg.wasm.d.ts",
]) {
  fs.copyFileSync(path.join(OUT_PKG, file), path.join(UI_DEST, file));
}

console.log("wasm bridge: copied to ui/src/lib/wasm/");
