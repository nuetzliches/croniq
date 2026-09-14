//
// What the dev stack and the e2e stack both have to do.
//
// The two scripts bring up the same three processes for different reasons —
// one for an edit-reload loop, one for Playwright — and they had drifted into
// two copies of the same ninety lines: binary resolution, `croniq init`
// seeding, prefixed spawning, and the shutdown that has to kill descendants
// rather than children (issue #673).
//
// Copies are not merely wasteful here, they are asymmetric. `killTree` and
// `assertPortFree` were written for the dev stack after two specific failures,
// and the e2e stack never got either — so the same clash that produces one
// clear line locally produces a Playwright timeout in CI.
//
// What deliberately stays in the callers: which ports, which data directory
// policy (persistent versus throwaway), and what to run. Those are the
// differences that justify two scripts at all.
//
// Node rather than a shell script for the reason build-wasm.mjs gives: npm
// runs scripts through cmd.exe on Windows, where `bash` resolves to the WSL
// shim and fails without a configured distro.

import { createServer } from "node:net";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The repository root, from this file's own location. */
export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const exe = process.platform === "win32" ? ".exe" : "";

/**
 * Croniq's development port block: 4230-4233, contiguous and deliberately not
 * 4000.
 *
 * The server's *product* default stays 4000 — it is baked into
 * `Croniqfile.demo`, `docker-compose.yml` and the README quickstart — but the
 * dev stack running there too meant `docker compose up` and these scripts
 * could not coexist.
 *
 * 4232 is free. The dashboard sat there while it was being rebuilt beside the
 * React one, which held 4231; with one dashboard left, the UI slot in the
 * block is where it belongs.
 *
 * Spelled once, because they were spelled in nine files and moving one meant
 * finding all nine (issue #676). The environment overrides are applied here
 * too, so a caller cannot read the constant and forget the override.
 */
export const PORTS = {
  /** Dev API. */
  api: Number(process.env.CRONIQ_DEV_PORT ?? 4230),
  /** Dev dashboard, served by vite. */
  ui: Number(process.env.CRONIQ_DEV_UI_PORT ?? 4231),
  /** The e2e stack's server — not the dev stack's, so running the suite does
   *  not require stopping what you were looking at. */
  e2e: Number(process.env.CRONIQ_E2E_PORT ?? 4233),
};

/**
 * Resolve a built binary.
 *
 * `CRONIQ_BIN_DIR` wins so CI can point at a release build or a downloaded
 * artefact without this script knowing how it got there.
 */
export function bin(name) {
  const dir = process.env.CRONIQ_BIN_DIR ?? path.join(ROOT, "target", "debug");
  const file = path.join(dir, `${name}${exe}`);
  if (!fs.existsSync(file)) {
    throw new Error(
      `missing ${file}\n\n` +
        `Build the binaries first:\n` +
        `  cargo build -p croniq-cli -p croniq-server -p croniq-demo-runner \\\n` +
        `    --bin croniq --bin croniq-server --bin croniq-demo-runner\n` +
        `Or point CRONIQ_BIN_DIR at a directory that has it.`,
    );
  }
  return file;
}

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
export function assertPortFree(port, label, envVar) {
  return new Promise((resolve) => {
    const probe = createServer();
    probe.once("error", () => {
      console.error(
        `\n[stack] Port ${port} (${label}) is already in use.\n` +
          `        Something else on this machine holds it — on Windows:\n` +
          `          Get-NetTCPConnection -LocalPort ${port} -State Listen |\n` +
          `            ForEach-Object { Get-Process -Id $_.OwningProcess }\n` +
          `        Then either stop it, or pick another port:\n` +
          `          ${envVar}=<port> …\n`,
      );
      process.exit(1);
    });
    probe.once("listening", () => probe.close(() => resolve()));
    probe.listen(port, "127.0.0.1");
  });
}

/**
 * Seed a database, once.
 *
 * `croniq init` only seeds an empty database, so a stale directory keeps its
 * old admin password — which is why the dev stack's persistent directory and
 * the e2e stack's throwaway one behave differently and both are correct.
 * Returns whether it actually seeded.
 */
export function seed(dir, { user, password, apiKey }) {
  const fresh = !fs.existsSync(path.join(dir, "croniq.db"));
  fs.mkdirSync(dir, { recursive: true });
  if (!fresh) return false;

  const result = spawnSync(
    bin("croniq"),
    ["init", "--data-dir", dir, "--username", user, "--password", password, "--api-key", apiKey],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    console.error(`croniq init failed (${result.status}):\n${result.stdout}\n${result.stderr}`);
    process.exit(1);
  }
  return true;
}

/**
 * Supervise a set of child processes: prefixed output, and one way down.
 *
 * `killTree` rather than `child.kill()` is the part that matters. npm on
 * Windows is a `.cmd` shim, so the direct child is the shim and vite is its
 * grandchild; killing the shim leaves vite holding its port, and the next run
 * then refuses to start — or, before `assertPortFree` existed, silently moved
 * to another port and printed a URL nothing was listening on.
 */
export function createSupervisor(prefix = "stack") {
  const children = [];
  let shuttingDown = false;

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

  /**
   * Start a child, tagging every line it writes.
   *
   * An early exit takes the whole stack down: a runner that died on startup
   * leaves a suite failing on missing data twenty seconds later, and the
   * reason is far away from the symptom.
   */
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
    return child;
  }

  return { run, shutdown, killTree, prefix };
}
