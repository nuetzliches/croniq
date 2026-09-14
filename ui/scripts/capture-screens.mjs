#!/usr/bin/env node
//
// Screenshot every dashboard screen, signed in, against real demo data.
//
// This exists because "the rebuild must not be a regression" was otherwise an
// unfalsifiable claim: the Playwright suite deliberately asserts nothing about
// appearance, so the only way to compare the old dashboard with the new one
// was to look at them, side by side, on the same data. It took a tree argument
// for exactly as long as there were two. It still earns its place — a capture
// is the cheapest way to see every screen at once after a change.
//
// Usage (with `node scripts/dev-stack.mjs` running, from ui/):
//   node scripts/capture-screens.mjs ../screens/today
//
// The port follows the dev stack; override with CRONIQ_DEV_UI_PORT.
//
// Not a test and deliberately not in CI: a screenshot diff is flaky (fonts,
// animation, live timestamps) and for a one-developer project the cost
// outweighs it. This is a tool for looking, not a gate.

import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const outDir = path.resolve(process.argv[2] ?? path.join(ROOT, "screens"));
const port = Number(process.env.CRONIQ_DEV_UI_PORT ?? 4232);
const base = `http://127.0.0.1:${port}`;

/** Screens in the order they appear in the navigation. */
const SCREENS = [
  ["01-dashboard", "/"],
  ["02-jobs", "/jobs"],
  ["03-executions", "/executions"],
  ["04-runners", "/runners"],
  ["05-dead-letters", "/dead-letters"],
  ["06-alerts", "/alerts"],
  ["07-calendars", "/calendars"],
  ["08-console", "/console"],
  ["09-settings-profile", "/settings/profile"],
  ["10-settings-clients", "/settings/clients"],
];

mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await context.newPage();

// The login page first, signed out — it is a screen too, and one of the more
// considered ones.
await page.goto(`${base}/login`, { waitUntil: "networkidle" });
await page.waitForTimeout(1200);
await page.screenshot({ path: path.join(outDir, "00-login.png") });
console.log("00-login");

await page.getByLabel("Username", { exact: true }).fill("admin");
await page.getByLabel("Password", { exact: true }).fill("demo-admin");
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
