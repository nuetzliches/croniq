#!/usr/bin/env node
//
// Capture the computed layout properties of every element on every dashboard
// page, keyed by DOM position.
//
// Written for #584. `src/styles/components.css` defined `.grid`, `.gap-4…20`
// and `.grow` as unlayered CSS, which beats Tailwind's `@layer utilities`
// regardless of source order — so those class names meant something other than
// what a reader of the markup would assume. Renaming them to `cq-*` is a pure
// rename: every declaration and every element keeps exactly the values it had.
// "Exactly" is a claim worth checking rather than asserting, hence this.
//
// Usage:
//   node scripts/style-snapshot.mjs before.json
//   …make the change, rebuild…
//   node scripts/style-snapshot.mjs after.json
//   node scripts/style-snapshot.mjs --diff before.json after.json
//
// Needs a stack on CRONIQ_E2E_PORT (see scripts/e2e-stack.mjs) and a built
// bundle in dist/.
//
// Kept in the tree because the same question — "did this change anything a
// user can see?" — comes up for every CSS refactor, and because the answer is
// only credible if the tool that produced it is inspectable.

import fs from "node:fs";
import { chromium } from "@playwright/test";

const PORT = Number(process.env.CRONIQ_E2E_PORT ?? 4010);
const BASE = `http://127.0.0.1:${PORT}`;

/** Layout-affecting properties the components.css/Tailwind overlap can touch. */
const PROPS = [
  "display",
  "gap",
  "row-gap",
  "column-gap",
  "grid-template-columns",
  "flex-grow",
  "flex-shrink",
  "flex-basis",
  "min-width",
];

const ROUTES = [
  "/",
  "/jobs",
  "/executions",
  "/runners",
  "/dead-letters",
  "/alerts",
  "/calendars",
  "/console",
  "/settings?tab=profile",
  "/settings?tab=users",
  "/settings?tab=clients",
  "/settings?tab=audit",
];

/**
 * A stable identity for an element that survives a class rename.
 *
 * Deliberately positional (`div[2]/span[1]/…`) rather than selector-based: the
 * whole point is that class names change, so anything keyed on them would
 * compare nothing.
 */
function collect(props) {
  const rows = {};
  const walk = (el, path) => {
    const cs = getComputedStyle(el);
    const values = {};
    for (const p of props) values[p] = cs.getPropertyValue(p);
    rows[path] = values;
    const counts = {};
    for (const child of el.children) {
      const tag = child.tagName.toLowerCase();
      counts[tag] = (counts[tag] ?? 0) + 1;
      walk(child, `${path}/${tag}[${counts[tag]}]`);
    }
  };
  walk(document.body, "body");
  return rows;
}

async function snapshot(outPath) {
  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  await page.goto(`${BASE}/login`);
  await page.locator('input[autocomplete="username"]').fill("admin");
  await page.locator('input[autocomplete="current-password"]').fill("demo-admin");
  await Promise.all([
    page.waitForResponse(
      (r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST",
    ),
    page.getByRole("button", { name: /sign in/i }).click(),
  ]);
  await page.getByRole("navigation", { name: "Main navigation" }).waitFor();

  const out = {};
  for (const route of ROUTES) {
    await page.goto(`${BASE}${route}`);
    await page.getByRole("navigation", { name: "Main navigation" }).waitFor();
    // Let lazy chunks and the first data render settle. Not networkidle: the
    // SSE streams never go idle, so it would time out on /runners and /console.
    await page.waitForTimeout(1500);
    out[route] = await page.evaluate(collect, PROPS);
    process.stderr.write(`${route}: ${Object.keys(out[route]).length} nodes\n`);
  }

  await browser.close();
  fs.writeFileSync(outPath, JSON.stringify(out, null, 1));
  process.stderr.write(`wrote ${outPath}\n`);
}

/**
 * Compare two snapshots.
 *
 * Nodes present in only one side are reported separately and do not count as
 * differences: the demo stack keeps executing jobs, so row counts drift
 * between runs no matter what the CSS does. A changed *value* on a node
 * present in both is the signal.
 */
function diff(beforePath, afterPath) {
  const a = JSON.parse(fs.readFileSync(beforePath, "utf8"));
  const b = JSON.parse(fs.readFileSync(afterPath, "utf8"));
  let changes = 0;
  let onlyOne = 0;
  for (const route of Object.keys(a)) {
    for (const [path, before] of Object.entries(a[route])) {
      const after = b[route]?.[path];
      if (!after) {
        onlyOne++;
        continue;
      }
      for (const prop of PROPS) {
        if (before[prop] !== after[prop]) {
          changes++;
          console.log(`${route} ${path}\n  ${prop}: ${before[prop]} -> ${after[prop]}`);
        }
      }
    }
    for (const path of Object.keys(b[route] ?? {})) {
      if (!a[route][path]) onlyOne++;
    }
  }
  console.log(`\n${changes} changed properties, ${onlyOne} nodes present on one side only`);
  process.exit(changes === 0 ? 0 : 1);
}

const [arg1, arg2, arg3] = process.argv.slice(2);
if (arg1 === "--diff") diff(arg2, arg3);
else await snapshot(arg1 ?? "style-snapshot.json");
