#!/usr/bin/env node
//
// Bring up a Croniq stack for the Playwright suite: seed a fresh database,
// start croniq-server over the shipped demo profile, and attach one demo
// runner so the Runners page and the console have something live to show.
//
// Playwright's `webServer` starts this and kills it when the run ends, so
// `npm run test:e2e` behaves the same on a laptop as in CI. That symmetry is
// the point — an e2e job that only works on the runner is a job nobody can
// debug.
//
// Deliberately not `docker compose up`: that would rebuild the image (a full
// Rust release build) for every run. The binaries are the same ones CI has
// already built by this point, and the seeding this does is exactly what
// docker-entrypoint.sh does — `croniq init` with a fixed admin and API key.
//
// Node rather than a shell script for the reason build-wasm.mjs gives: npm
// runs scripts through cmd.exe on Windows, where `bash` resolves to the WSL
// shim and fails without a configured distro.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Where the suite expects the server. Not :4000 — a dev stack may hold it. */
export const E2E_PORT = Number(process.env.CRONIQ_E2E_PORT ?? 4010);

/** Fixed demo credentials. Public by design; see docker-compose.yml. */
export const E2E_USER = "admin";
export const E2E_PASSWORD = "demo-admin";
const E2E_API_KEY = "croniq_e2e_local_suite_key_not_for_production_use";

const exe = process.platform === "win32" ? ".exe" : "";

/**
 * Resolve a built binary.
 *
 * `CRONIQ_BIN_DIR` wins so CI can point at a release build or a downloaded
 * artefact without this script knowing how it got there.
 */
function bin(name) {
  const dir = process.env.CRONIQ_BIN_DIR ?? path.join(ROOT, "target", "debug");
  const file = path.join(dir, `${name}${exe}`);
  if (!fs.existsSync(file)) {
    throw new Error(
      `missing ${file}\n` +
        `Build it first:\n` +
        `  cargo build -p croniq-cli -p croniq-server -p croniq-demo-runner \\\n` +
        `    --bin croniq --bin croniq-server --bin croniq-demo-runner\n` +
        `Or point CRONIQ_BIN_DIR at a directory that has it.`,
    );
  }
  return file;
}

/**
 * A fresh data directory per run.
 *
 * Never reused: `croniq init` only seeds on an empty database, so a leftover
 * directory would silently keep an older schema and older credentials, and the
 * failure would surface as a login error three tests later.
 */
function freshDataDir() {
  const dir = path.join(os.tmpdir(), `croniq-e2e-${process.pid}-${Date.now()}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function seed(dataDir) {
  const res = spawnSync(
    bin("croniq"),
    [
      "init",
      "--data-dir",
      dataDir,
      "--username",
      E2E_USER,
      "--password",
      E2E_PASSWORD,
      "--api-key",
      E2E_API_KEY,
    ],
    { encoding: "utf8" },
  );
  if (res.status !== 0) {
    throw new Error(`croniq init failed (${res.status}):\n${res.stdout}\n${res.stderr}`);
  }
}

const children = [];

function spawnChild(label, file, args, env) {
  const child = spawn(file, args, {
    env: { ...process.env, ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  // Prefix the output so a failing run shows which process complained.
  for (const stream of [child.stdout, child.stderr]) {
    stream.setEncoding("utf8");
    stream.on("data", (chunk) => {
      for (const line of chunk.split("\n")) {
        if (line.trim()) process.stderr.write(`[${label}] ${line}\n`);
      }
    });
  }
  child.on("exit", (code, signal) => {
    if (!shuttingDown) {
      process.stderr.write(`[${label}] exited early: code=${code} signal=${signal}\n`);
      shutdown(1);
    }
  });
  children.push(child);
  return child;
}

let shuttingDown = false;
function shutdown(code = 0) {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const child of children) child.kill();
  process.exit(code);
}

for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => shutdown(0));

const dataDir = freshDataDir();
seed(dataDir);

spawnChild(
  "server",
  bin("croniq-server"),
  [
    "--config",
    path.join(ROOT, "Croniqfile.demo"),
    // The demo profile hardcodes `listen :4000` and a container data_dir;
    // both are overridden here so the suite never collides with a running
    // dev stack and never writes outside the temp directory.
    "--listen",
    `127.0.0.1:${E2E_PORT}`,
    "--data-dir",
    dataDir,
    "--ui-dir",
    path.join(ROOT, "ui", "dist"),
  ],
  {
    CRONIQ_DATA_DIR: dataDir,
    // Demo mode seeds nothing here (init already ran) but does relax the
    // dashboard's first-run affordances the demo profile assumes. It also
    // refuses to bind a non-loopback address, which is why the listen above
    // is explicitly 127.0.0.1 rather than :PORT.
    CRONIQ_DEMO_MODE: "1",
    RUST_LOG: process.env.RUST_LOG ?? "warn",
  },
);

spawnChild("runner", bin("croniq-demo-runner"), [], {
  CRONIQ_SERVER_URL: `http://127.0.0.1:${E2E_PORT}`,
  CRONIQ_API_KEY: E2E_API_KEY,
  // No failures: dead-letter and retry behaviour deserve their own
  // deterministic fixtures rather than a coin flip that makes unrelated
  // assertions flaky.
  RUNNER_FAIL_RATE: "0",
  RUNNER_MAX_INFLIGHT: "4",
  RUNNER_TAGS: "env=e2e,role=worker",
  RUST_LOG: process.env.RUST_LOG ?? "warn",
});
