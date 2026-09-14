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
// The machinery this shares with the dev stack — binary resolution, seeding,
// prefixed spawning, killing descendants rather than children — lives in
// scripts/lib/stack.mjs (issue #673). What stays here is what makes this the
// *e2e* stack: a throwaway data directory, a built bundle rather than a dev
// server, and a runner configured never to fail.

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
} from "../../scripts/lib/stack.mjs";

/**
 * Where the suite expects the server: 4233, the last slot in croniq's
 * 4230-4233 development block. Deliberately not the dev stack's own port —
 * running the suite must not require stopping what you were looking at.
 */
export const E2E_PORT = PORTS.e2e;

/** The dashboard build the server serves. */
const UI_DIST = path.join(ROOT, "ui", "dist");

/** Fixed demo credentials. Public by design; see docker-compose.yml. */
export const E2E_USER = "admin";
export const E2E_PASSWORD = "demo-admin";
const E2E_API_KEY = "croniq_e2e_local_suite_key_not_for_production_use";

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

// A missing bundle would otherwise surface as every test failing on a blank
// page, which reads as a broken dashboard rather than a missing build step.
if (!fs.existsSync(path.join(UI_DIST, "index.html"))) {
  console.error(
    `no built dashboard at ${UI_DIST}\n` + `Build it first:\n` + `  npm --prefix ui run build`,
  );
  process.exit(1);
}
console.log(`[e2e] serving the dashboard from ${UI_DIST}`);

// The dev stack has had this check since a clash cost an afternoon; the e2e
// stack never got it, so the same clash surfaced as a Playwright timeout with
// no cause attached (issue #673).
await assertPortFree(E2E_PORT, "e2e server", "CRONIQ_E2E_PORT");

const dataDir = freshDataDir();
seed(dataDir, { user: E2E_USER, password: E2E_PASSWORD, apiKey: E2E_API_KEY });

const { run } = createSupervisor("e2e");

run(
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
    UI_DIST,
  ],
  {
    env: {
      CRONIQ_DATA_DIR: dataDir,
      // Demo mode seeds nothing here (init already ran) but does relax the
      // dashboard's first-run affordances the demo profile assumes. It also
      // refuses to bind a non-loopback address, which is why the listen above
      // is explicitly 127.0.0.1 rather than :PORT.
      CRONIQ_DEMO_MODE: "1",
      // `info`, not `warn`: the console page tails this feed, and a suite that
      // only ever sees an empty console cannot tell a working stream from a
      // broken one.
      RUST_LOG: process.env.RUST_LOG ?? "info",
    },
  },
);

run("runner", bin("croniq-demo-runner"), [], {
  env: {
    CRONIQ_SERVER_URL: `http://127.0.0.1:${E2E_PORT}`,
    CRONIQ_API_KEY: E2E_API_KEY,
    // No failures: dead-letter and retry behaviour deserve their own
    // deterministic fixtures rather than a coin flip that makes unrelated
    // assertions flaky.
    RUNNER_FAIL_RATE: "0",
    RUNNER_MAX_INFLIGHT: "4",
    RUNNER_TAGS: "env=e2e,role=worker",
    RUST_LOG: process.env.RUST_LOG ?? "warn",
  },
});
