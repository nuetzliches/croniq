#!/usr/bin/env node
//
// Start everything needed to work on the dashboard:
//
//   croniq-server   http://127.0.0.1:4230   API, seeded demo data, one runner
//   dashboard       http://127.0.0.1:4231   the Vue SPA in ui/
//
// 4230-4233 is croniq's development block. The reasoning, and the numbers
// themselves, live in scripts/lib/stack.mjs — they used to be spelled out in
// nine files, and moving one meant finding all nine (issue #676).
//
// The dev server proxies /v1, /health, /version and /metrics to the API, so it
// is same-origin with it and gets the refresh cookie exactly as production
// does (ADR-0001).
//
// Usage:
//   node scripts/dev-stack.mjs           API, runner and dashboard
//   node scripts/dev-stack.mjs --api     API and runner only — bring your own
//                                        dev server, or point a build at it
//
// Ports default to 4230 / 4231 and are overridable when something else on the
// machine already holds one:
//   CRONIQ_DEV_PORT, CRONIQ_DEV_UI_PORT
//
// Prerequisites, once:
//   cargo build -p croniq-cli -p croniq-server -p croniq-demo-runner \
//     --bin croniq --bin croniq-server --bin croniq-demo-runner
//   (cd ui && npm ci)
//
// Deliberately not docker compose: HMR through a bind mount on Windows is
// slow and the point of this script is the edit-reload loop.
//
// Node rather than a shell script for the reason build-wasm.mjs gives: npm and
// the terminals here are not guaranteed to be POSIX on Windows.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  PORTS,
  ROOT,
  assertPortFree,
  bin,
  createSupervisor,
  seed,
} from "./lib/stack.mjs";

const args = new Set(process.argv.slice(2));
const withUi = !args.has("--api");

const PORT = PORTS.api;
const UI_PORT = PORTS.ui;
const USER = "admin";
const PASSWORD = "demo-admin";
const API_KEY = "croniq_dev_stack_key_not_for_production_use";

/**
 * A persistent data directory, unlike the e2e stack's throwaway one.
 *
 * Comparing two dashboards is worth nothing if the jobs you created to look at
 * are gone after a restart. Delete it to start clean; `croniq init` only seeds
 * an empty database, so a stale directory keeps its old admin password.
 */
function dataDir() {
  const dir = process.env.CRONIQ_DEV_DATA_DIR ?? path.join(os.tmpdir(), "croniq-dev-stack");
  if (seed(dir, { user: USER, password: PASSWORD, apiKey: API_KEY })) {
    console.log(`[dev] seeded a fresh database in ${dir}`);
  }
  return dir;
}

const { run } = createSupervisor("dev");

await assertPortFree(PORT, "API", "CRONIQ_DEV_PORT");
if (withUi) await assertPortFree(UI_PORT, "dev server", "CRONIQ_DEV_UI_PORT");

const dir = dataDir();

run(
  "api",
  bin("croniq-server"),
  [
    "--config",
    path.join(ROOT, "Croniqfile.demo"),
    "--listen",
    `127.0.0.1:${PORT}`,
    "--data-dir",
    dir,
  ],
  {
    env: {
      CRONIQ_DATA_DIR: dir,
      CRONIQ_DEMO_MODE: "1",
      RUST_LOG: process.env.RUST_LOG ?? "info",
    },
  },
);

run("runner", bin("croniq-demo-runner"), [], {
  env: {
    CRONIQ_SERVER_URL: `http://127.0.0.1:${PORT}`,
    CRONIQ_API_KEY: API_KEY,
    RUNNER_FAIL_RATE: process.env.RUNNER_FAIL_RATE ?? "0.05",
    RUNNER_TAGS: "env=dev,role=worker",
    RUST_LOG: process.env.RUST_LOG ?? "warn",
  },
});

// `shell: true` because npm on Windows is a .cmd shim that spawn cannot exec
// directly. The arguments are literals from this file, not input.
//
// `--host 127.0.0.1` is not decoration. Vite's default host is `localhost`,
// which on this platform resolves to `::1` — so the server answers on IPv6
// while the banner below advertises an IPv4 address that refuses the
// connection, and `assertPortFree`, which probes 127.0.0.1, cannot see a vite
// already holding the port on `::1`. Pinning the family makes the printed URL
// true and the pre-flight check meaningful.
if (withUi) {
  run("ui", "npm", ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(UI_PORT)], {
    cwd: path.join(ROOT, "ui"),
    env: { CRONIQ_API_ORIGIN: `http://127.0.0.1:${PORT}` },
    shell: true,
  });
}

console.log(
  [
    "",
    `[dev] API        http://127.0.0.1:${PORT}   (${USER} / ${PASSWORD})`,
    withUi ? `[dev] Dashboard  http://127.0.0.1:${UI_PORT}` : null,
    "[dev] Ctrl-C stops everything.",
    "",
  ]
    .filter(Boolean)
    .join("\n"),
);
