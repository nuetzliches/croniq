#!/usr/bin/env node
//
// The `seed` job of devstack.toml: seed the dev database once.
//
// The same `croniq init` the Docker entrypoint runs on a first start, with the
// credentials devstack hands over from the `server` service of
// docker-compose.yml — so the demo runner's API key and the admin login match
// the quickstart. `seed()` skips an existing database, which is what makes
// this safe to run on every `devstack up`: `croniq init` itself would fail on
// the second run, creating an admin user that already exists.
//
// `CRONIQ_BIN_DIR` points at the job's artifact, where `croniq` was built.

import { seed } from "./lib/stack.mjs";

const dir = process.env.CRONIQ_DATA_DIR;
if (!dir) {
  console.error("CRONIQ_DATA_DIR is not set");
  process.exit(1);
}

const seeded = seed(dir, {
  user: process.env.CRONIQ_ADMIN_USER ?? "admin",
  password: process.env.CRONIQ_ADMIN_PASSWORD ?? "demo-admin",
  apiKey: process.env.CRONIQ_INIT_API_KEY ?? "croniq_dev_stack_key_not_for_production_use",
});
console.log(seeded ? `seeded a fresh database in ${dir}` : `database in ${dir} exists, left as is`);
