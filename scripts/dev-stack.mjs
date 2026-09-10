#!/usr/bin/env node
//
// Start everything needed to compare the two dashboards side by side:
//
//   croniq-server   http://127.0.0.1:4000   API, seeded demo data, one runner
//   ui   (React)    http://127.0.0.1:5173   the shipping dashboard
//   ui-vue (Vue)    http://127.0.0.1:5174   the rebuild (ADR-0004)
//
// Both dev servers proxy /v1, /health, /version and /metrics to :4000, so each
// is same-origin with the API and gets the refresh cookie exactly as
// production does (ADR-0001). Two tabs, one server, one database — whatever
// you do in one is visible in the other.
//
// Usage:
//   node scripts/dev-stack.mjs           everything
//   node scripts/dev-stack.mjs --no-vue  server + React only
//   node scripts/dev-stack.mjs --no-ui   server + Vue only
//   node scripts/dev-stack.mjs --api     server only (use your own dev server)
//
// Ports default to 4000 / 5173 / 5174 and are overridable when something else
// on the machine already holds one:
//   CRONIQ_DEV_PORT, CRONIQ_DEV_UI_PORT, CRONIQ_DEV_VUE_PORT
//
// Prerequisites, once:
//   cargo build -p croniq-cli -p croniq-server -p croniq-demo-runner \
//     --bin croniq --bin croniq-server --bin croniq-demo-runner
//   (cd ui && npm ci) && (cd ui-vue && npm ci)
//
// Deliberately not docker compose: HMR through a bind mount on Windows is
// slow and the point of this script is the edit-reload loop.
//
// Node rather than a shell script for the reason build-wasm.mjs gives: npm and
// the terminals here are not guaranteed to be POSIX on Windows.

import { createServer } from "node:net";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? ".exe" : "";

const args = new Set(process.argv.slice(2));
const apiOnly = args.has("--api");
const withReact = !apiOnly && !args.has("--no-ui");
const withVue = !apiOnly && !args.has("--no-vue");

const PORT = Number(process.env.CRONIQ_DEV_PORT ?? 4000);
const REACT_PORT = Number(process.env.CRONIQ_DEV_UI_PORT ?? 5173);
const VUE_PORT = Number(process.env.CRONIQ_DEV_VUE_PORT ?? 5174);
const USER = "admin";
const PASSWORD = "demo-admin";
const API_KEY = "croniq_dev_stack_key_not_for_production_use";

/**
 * Refuse to start on an occupied port, with an answer rather than a stack.
 *
 * Both dev servers run with `strictPort`, so a clash is already fatal — but
 * vite reports it as an unhandled listen error from inside a child process,
 * several frames deep and after everything else has started. Checking here
 * turns that into one line naming the variable to set. (Vite's default
 * behaviour, silently taking the next free port, is worse than either: the
 * script would print URLs that are not where the servers are.)
 */
function assertPortFree(port, label, envVar) {
  return new Promise((resolve) => {
    const probe = createServer();
    probe.once("error", () => {
      console.error(
        `\n[dev] Port ${port} (${label}) is already in use.\n` +
          `      Something else on this machine holds it — on Windows:\n` +
          `        Get-NetTCPConnection -LocalPort ${port} -State Listen |\n` +
          `          ForEach-Object { Get-Process -Id $_.OwningProcess }\n` +
          `      Then either stop it, or pick another port:\n` +
          `        ${envVar}=<port> node scripts/dev-stack.mjs\n`,
      );
      process.exit(1);
    });
    probe.once("listening", () => probe.close(() => resolve()));
    probe.listen(port, "127.0.0.1");
  });
}

function bin(name) {
  const dir = process.env.CRONIQ_BIN_DIR ?? path.join(ROOT, "target", "debug");
  const file = path.join(dir, `${name}${exe}`);
  if (!fs.existsSync(file)) {
    console.error(
      `\nMissing ${file}\n\n` +
        `Build the binaries first:\n` +
        `  cargo build -p croniq-cli -p croniq-server -p croniq-demo-runner \\\n` +
        `    --bin croniq --bin croniq-server --bin croniq-demo-runner\n`,
    );
    process.exit(1);
  }
  return file;
}

/**
 * A persistent data directory, unlike the e2e stack's throwaway one.
 *
 * Comparing two dashboards is worth nothing if the jobs you created to look at
 * are gone after a restart. Delete it to start clean; `croniq init` only seeds
 * an empty database, so a stale directory keeps its old admin password.
 */
function dataDir() {
  const dir = process.env.CRONIQ_DEV_DATA_DIR ?? path.join(os.tmpdir(), "croniq-dev-stack");
  const fresh = !fs.existsSync(path.join(dir, "croniq.db"));
  fs.mkdirSync(dir, { recursive: true });
  if (fresh) {
    const res = spawnSync(
      bin("croniq"),
      ["init", "--data-dir", dir, "--username", USER, "--password", PASSWORD, "--api-key", API_KEY],
      { encoding: "utf8" },
    );
    if (res.status !== 0) {
      console.error(`croniq init failed (${res.status}):\n${res.stdout}\n${res.stderr}`);
      process.exit(1);
    }
    console.log(`[dev] seeded a fresh database in ${dir}`);
  }
  return dir;
}

const children = [];
let shuttingDown = false;

/**
 * Kill a child *and its descendants*.
 *
 * `child.kill()` is not enough for the dev servers: npm on Windows is a `.cmd`
 * shim, so the direct child is the shim and vite is its grandchild. Killing
 * the shim leaves vite holding its port, and the next run of this script finds
 * 5173 occupied and silently moves to another port — which is how you end up
 * comparing two dashboards on the wrong URLs.
 */
function killTree(child) {
  if (!child.pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    child.kill();
  }
}

function shutdown(code = 0) {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const child of children) killTree(child);
  process.exit(code);
}
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => shutdown(0));

function run(label, file, argv, options = {}) {
  const child = spawn(file, argv, {
    cwd: options.cwd ?? ROOT,
    env: { ...process.env, ...options.env },
    stdio: ["ignore", "pipe", "pipe"],
    shell: options.shell ?? false,
  });
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
}

await assertPortFree(PORT, "API", "CRONIQ_DEV_PORT");
if (withReact) await assertPortFree(REACT_PORT, "React dev server", "CRONIQ_DEV_UI_PORT");
if (withVue) await assertPortFree(VUE_PORT, "Vue dev server", "CRONIQ_DEV_VUE_PORT");

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
const npmEnv = { CRONIQ_API_ORIGIN: `http://127.0.0.1:${PORT}` };
if (withReact) {
  run("react", "npm", ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(REACT_PORT)], {
    cwd: path.join(ROOT, "ui"),
    env: npmEnv,
    shell: true,
  });
}
if (withVue) {
  run("vue", "npm", ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(VUE_PORT)], {
    cwd: path.join(ROOT, "ui-vue"),
    env: npmEnv,
    shell: true,
  });
}

console.log(
  [
    "",
    `[dev] API      http://127.0.0.1:${PORT}   (${USER} / ${PASSWORD})`,
    withReact ? `[dev] React    http://127.0.0.1:${REACT_PORT}` : null,
    withVue ? `[dev] Vue      http://127.0.0.1:${VUE_PORT}` : null,
    "[dev] Ctrl-C stops everything.",
    "",
  ]
    .filter(Boolean)
    .join("\n"),
);
