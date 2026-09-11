// Which controls would a screen reader announce as nothing useful?
//
// Issue #595 counted 51 of 54 inputs in the React dashboard with no accessible
// name. The rebuild is the chance not to inherit that, but "we were careful"
// is not a measurement.
//
// This reads Chromium's **own** accessibility tree, over CDP
// (`Accessibility.getFullAXTree`), not the markup. That distinction is the whole point and it was
// learned the hard way: a first version of this script walked the DOM and fell
// back to `textContent` when it could not compute a name, which made it report
// a screen as clean while the real tree announced a select as "Show popup".
// A lenient checker is worse than none — it certifies the bug.
//
// It walks every screen, then opens the dialogs and panels that hide most of
// the product's form controls behind a click, because a screen that is clean
// until you open one is not clean.
//
// Usage (with `node scripts/dev-stack.mjs` running, from ui/):
//   node scripts/accessible-names.mjs            # the Vue tree
//   node scripts/accessible-names.mjs 4231       # the React tree, for comparison
//
// Not in CI: it needs the dev stack. A checking tool, like capture-screens.mjs
// and measure-scroll.mjs.

import { chromium } from "@playwright/test";

const port = process.argv[2] ?? process.env.CRONIQ_DEV_VUE_PORT ?? 4232;
const base = `http://127.0.0.1:${port}`;

/** Roles worth naming: things a person operates. */
const CONTROL_ROLES = new Set([
  "button",
  "checkbox",
  "combobox",
  "link",
  "listbox",
  "menuitem",
  "menuitemcheckbox",
  "radio",
  "searchbox",
  "slider",
  "spinbutton",
  "switch",
  "textbox",
]);

/**
 * Names that exist but say nothing about what the control does.
 *
 * "Show popup" is Nuxt UI's own default on a select trigger: it describes the
 * mechanism, not the choice being made, and it is identical on every select on
 * the page.
 */
const GENERIC = new Set([
  "",
  "show popup",
  "toggle",
  "button",
  "menu",
  "select",
  "choose",
  "options",
]);

// Deliberately *not* generic: "Close" on a dialog and "Open" on a disclosure
// say exactly what the control does. A checker that flags them trains the
// reader to ignore it.

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const cdp = await page.context().newCDPSession(page);

/**
 * Chromium's computed tree, over CDP. `page.accessibility` was removed from
 * Playwright; `Accessibility.getFullAXTree` is the same data from the source.
 */
async function axTree() {
  await cdp.send("Accessibility.enable");
  const { nodes } = await cdp.send("Accessibility.getFullAXTree");
  return nodes;
}

async function check(label) {
  const nodes = await axTree();
  const byId = new Map(nodes.map((node) => [node.nodeId, node]));
  const found = [];
  for (const node of nodes) {
    if (node.ignored) continue;
    const role = node.role?.value ?? "";
    const name = (node.name?.value ?? "").trim().replace(/\s+/g, " ");
    if (!CONTROL_ROLES.has(role) || !GENERIC.has(name.toLowerCase())) continue;
    // A short trail of named ancestors, so a bare "(none)" is locatable.
    const trail = [];
    let parent = byId.get(node.parentId);
    while (parent && trail.length < 3) {
      const parentName = (parent.name?.value ?? "").trim().replace(/\s+/g, " ");
      if (parentName) trail.unshift(parentName.slice(0, 30));
      parent = byId.get(parent.parentId);
    }
    found.push({ role, name: name || "(none)", trail: trail.join(" › ") });
  }
  if (found.length) {
    console.log(`\n${label} — ${found.length} unnamed`);
    for (const control of found) {
      console.log(`  ${control.role.padEnd(9)} ${control.name.padEnd(14)} in: ${control.trail}`);
    }
  } else {
    console.log(`\n${label} — clean`);
  }
  return found.length;
}

await page.goto(`${base}/login`, { waitUntil: "networkidle" });
await page.waitForTimeout(900);

let total = 0;
total += await check("/login");

await page.locator('input[autocomplete="username"]').fill("admin");
await page.locator('input[autocomplete="current-password"]').fill("demo-admin");
await Promise.all([
  page.waitForResponse((r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST"),
  page.locator('button[type="submit"]').click(),
]);

for (const path of [
  "/",
  "/executions",
  "/runners",
  "/dead-letters",
  "/jobs",
  "/calendars",
  "/alerts",
  "/alerts/channels",
  "/alerts/deliveries",
  "/settings",
  "/settings/people",
  "/settings/clients",
  "/settings/audit",
]) {
  await page.goto(base + path, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1600);
  total += await check(path);
}

await page.goto(`${base}/jobs`, { waitUntil: "networkidle" });
await page.waitForTimeout(1200);
await page.getByRole("button", { name: "New job" }).first().click();
await page.waitForTimeout(800);
total += await check("/jobs — new-job dialog");
await page.keyboard.press("Escape");
await page.waitForTimeout(400);

// The schedule editor lives inside a job's detail, and its calendar picker is
// the control that prompted this script. It only renders its form for an
// API-managed job — a Croniqfile-managed one is read-only — so pick that row
// or say plainly that the check did not reach it.
const apiJob = page.locator("tbody tr", { hasText: "API" }).first();
if (await apiJob.count()) {
  await apiJob.click();
  await page.waitForTimeout(1400);
  const add = page.getByRole("button", { name: "Add", exact: true });
  if (await add.count()) {
    await add.click();
    await page.waitForTimeout(700);
  }
  total += await check("/jobs — job detail, schedule editor open");
} else {
  console.log("\n/jobs — job detail: skipped, no API-managed job to edit");
}

await page.goto(`${base}/calendars`, { waitUntil: "networkidle" });
await page.waitForTimeout(1200);
await page.getByRole("button", { name: "New calendar" }).first().click();
await page.waitForTimeout(1600);
total += await check("/calendars — new-calendar dialog");

console.log(`\ntotal unnamed controls: ${total}`);
await browser.close();
