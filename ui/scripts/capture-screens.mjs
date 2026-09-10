#!/usr/bin/env node
//
// Screenshot every dashboard screen, signed in, against real demo data.
//
// This exists because "the rebuild must not be a regression" is otherwise an
// unfalsifiable claim. ADR-0004 makes the Vue tree the replacement for the
// shipping dashboard, and the Playwright suite deliberately asserts nothing
// about appearance — so the only way to compare the two is to look at them,
// side by side, on the same data.
//
// Lives in ui/ because that is where playwright is installed; it captures both
// trees regardless.
//
// Usage (with `node scripts/dev-stack.mjs` running, from ui/):
//   node scripts/capture-screens.mjs react  ../screens/react
//   node scripts/capture-screens.mjs vue    ../screens/vue
//
// Ports follow the dev stack; override with CRONIQ_DEV_UI_PORT /
// CRONIQ_DEV_VUE_PORT the same way.
//
// Not a test and deliberately not in CI: a screenshot diff is flaky (fonts,
// animation, live timestamps) and for a one-developer project the cost
// outweighs it. This is a tool for looking, not a gate.

import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const tree = process.argv[2] ?? "react";
const outDir = path.resolve(process.argv[3] ?? path.join(ROOT, "screens", tree));

const PORTS = {
  react: Number(process.env.CRONIQ_DEV_UI_PORT ?? 4100),
  vue: Number(process.env.CRONIQ_DEV_VUE_PORT ?? 4101),
};
const port = PORTS[tree];
if (!port) {
  console.error(`unknown tree '${tree}' — expected one of: ${Object.keys(PORTS).join(", ")}`);
  process.exit(1);
}
const base = `http://127.0.0.1:${port}`;

/**
 * Screens in the order they appear in the navigation. Paths, not names, so the
 * same list works for both trees — the Vue tree's unbuilt routes render their
 * placeholder rather than failing, which is itself worth seeing in a capture.
 */
const SCREENS = [
  ["01-dashboard", "/"],
  ["02-jobs", "/jobs"],
  ["03-executions", "/executions"],
  ["04-runners", "/runners"],
  ["05-dead-letters", "/dead-letters"],
  ["06-alerts", "/alerts"],
  ["07-calendars", "/calendars"],
  ["08-console", "/console"],
  ["09-settings-profile", "/settings?tab=profile"],
  ["10-settings-clients", "/settings?tab=clients"],
];

mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await context.newPage();

// The login page first, signed out — it is a screen too, and in the React tree
// one of the more considered ones.
await page.goto(`${base}/login`, { waitUntil: "networkidle" });
await page.waitForTimeout(1200);
await page.screenshot({ path: path.join(outDir, "00-login.png") });
console.log("00-login");

await page.locator('input[autocomplete="username"]').fill("admin");
await page.locator('input[autocomplete="current-password"]').fill("demo-admin");
await Promise.all([
  page.waitForResponse((r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST"),
  page.getByRole("button", { name: /sign in/i }).click(),
]);
await page.getByRole("navigation", { name: "Main navigation" }).waitFor({ timeout: 15_000 });

for (const [name, route] of SCREENS) {
  await page.goto(`${base}${route}`);
  await page.getByRole("navigation", { name: "Main navigation" }).waitFor({ timeout: 15_000 });
  // Long enough for lazy chunks, the first data render and one SSE frame.
  await page.waitForTimeout(2500);
  await page.screenshot({ path: path.join(outDir, `${name}.png`) });
  console.log(name);
}

await browser.close();
console.log(`\nwrote ${SCREENS.length + 1} screens to ${outDir}`);
