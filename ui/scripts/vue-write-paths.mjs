// Drive the Vue dashboard's write paths against a live dev stack.
//
// A stopgap, and labelled as one. ADR-0004 makes the Playwright suite the
// acceptance gate for the rebuild, but that suite runs against the *built*
// React dashboard the server serves on 4233 — the Vue tree has no e2e project
// yet (see docs/ui-visual-design.md, "Noch offen"). Until it does, the screens
// that create, edit and delete things have no automated check at all, and a
// broken mutation is invisible in a screenshot.
//
// So this walks the paths a screenshot cannot, reporting any 4xx or page error
// made along the way:
//
//   Jobs      create, attach a schedule, disable and re-enable it, trigger,
//             pause and resume, edit, render the DSL, adopt, delete.
//   Calendars build rules, save, confirm an unreferenced calendar says so,
//             bind a schedule to it, confirm the detail names the gated job,
//             reopen it in the builder, delete.
//
// It cleans up after itself: `smoke:vue-jobs`, `smoke:calendar-user` and
// `smoke-business-days` are all removed before it exits.
//
// Usage (with `node scripts/dev-stack.mjs` running, from ui/):
//   node scripts/vue-write-paths.mjs
//
// Not in CI: it needs the dev stack and writes to the demo database.

import { chromium } from "@playwright/test";

const base = `http://127.0.0.1:4232`;
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const problems = [];
page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
page.on("response", (r) => {
  if (r.status() >= 400) problems.push(`${r.status()} ${r.request().method()} ${new URL(r.url()).pathname}`);
});

const step = async (name, fn) => {
  try {
    await fn();
    console.log(`ok   ${name}`);
  } catch (e) {
    console.log(`FAIL ${name}: ${String(e).split("\n")[0]}`);
  }
};

await page.goto(`${base}/login`, { waitUntil: "networkidle" });
await page.locator('input[autocomplete="username"]').fill("admin");
await page.locator('input[autocomplete="current-password"]').fill("demo-admin");
await Promise.all([
  page.waitForResponse((r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST"),
  page.locator('button[type="submit"]').click(),
]);

const KEY = "smoke:vue-jobs";

await page.goto(`${base}/jobs`, { waitUntil: "networkidle" });
await page.waitForTimeout(800);

await step("create a job", async () => {
  await page.getByRole("button", { name: "New job" }).first().click();
  await page.getByPlaceholder("demo:report").fill(KEY);
  await page.getByPlaceholder("What this job does").fill("Created by the smoke script");
  await page.getByPlaceholder("nightly report").fill("smoke test");
  await page.getByPlaceholder("10m").fill("2m");
  await page.getByPlaceholder("3").first().fill("1");
  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/jobs") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Create job" }).click(),
  ]);
  await page.waitForURL(/\/jobs\/smoke/);
});

await page.waitForTimeout(900);

await step("the new job is API-managed, so editing is offered", async () => {
  const edit = page.getByRole("button", { name: "Edit", exact: true });
  if (await edit.isDisabled()) throw new Error("Edit is disabled on an API job");
});

await step("add a schedule", async () => {
  await page.getByRole("button", { name: "Add" }).click();
  await page.getByPlaceholder("0 3 * * *").fill("*/10 * * * *");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/schedules") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Add schedule" }).click(),
  ]);
  await page.waitForTimeout(700);
  await page.getByText("*/10 * * * *").first().waitFor({ timeout: 3000 });
});

await step("the list shows the rule and a next fire", async () => {
  await page.waitForTimeout(1200);
  const row = page.locator("tbody tr", { hasText: KEY });
  const text = await row.innerText();
  if (!text.includes("*/10")) throw new Error(`rule missing from the row: ${text}`);
});

await step("disable and re-enable the schedule", async () => {
  await page.getByRole("button", { name: "Disable this schedule" }).click();
  await page.waitForTimeout(700);
  await page.getByRole("button", { name: "Enable this schedule" }).waitFor({ timeout: 3000 });
  await page.getByRole("button", { name: "Enable this schedule" }).click();
  await page.waitForTimeout(700);
});

await step("run now reports what the server did", async () => {
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/trigger") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Run now" }).click(),
  ]);
  await page.waitForTimeout(600);
  const note = await page.getByRole("status").last().innerText().catch(() => "");
  if (!/Queued|Coalesced/.test(note)) throw new Error(`no outcome reported: ${note}`);
});

await step("pause and resume", async () => {
  await page.getByRole("button", { name: "Pause" }).click();
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: "Resume" }).waitFor({ timeout: 3000 });
  await page.getByRole("button", { name: "Resume" }).click();
  await page.waitForTimeout(900);
});

await step("edit the job", async () => {
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByPlaceholder("What this job does").fill("Edited by the smoke script");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/jobs/") && r.request().method() === "PUT"),
    page.getByRole("button", { name: "Save" }).click(),
  ]);
  await page.waitForTimeout(800);
  await page.getByText("Edited by the smoke script").first().waitFor({ timeout: 3000 });
});

await step("the DSL tab renders the job", async () => {
  await page.getByRole("tab", { name: "DSL" }).click();
  await page.waitForTimeout(500);
  const pre = await page.locator("pre").last().innerText();
  if (!pre.includes(`job "${KEY}"`)) throw new Error("the job block is missing");
  if (!pre.includes("*/10 * * * *")) throw new Error("the schedule is missing");
});

await step("adopt on a Croniqfile job surfaces the server's answer", async () => {
  await page.goto(`${base}/jobs/demo%3Areport`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: "Adopt" }).click();
  await page.waitForTimeout(1200);
  const alert = await page.getByRole("alert").last().innerText().catch(() => "");
  console.log(`     adopt said: ${alert.replace(/\s+/g, " ").slice(0, 120) || "(nothing — it worked)"}`);
});

await step("delete the smoke job", async () => {
  await page.goto(`${base}/jobs/${encodeURIComponent(KEY)}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: `Delete ${KEY}` }).click();
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/jobs/") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: "Delete", exact: true }).click(),
  ]);
  await page.waitForURL(/\/jobs$/, { timeout: 5000 });
});

/* ─── Calendars ─────────────────────────────────────────────────────────── */

const CAL = "smoke-business-days";
const CAL_JOB = "smoke:calendar-user";

await step("create a calendar through the rule builder", async () => {
  await page.goto(`${base}/calendars`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: "New calendar" }).first().click();
  await page.getByPlaceholder("business-days").fill(CAL);
  await page.getByPlaceholder("Europe/Berlin").first().fill("Europe/Berlin");
  await page.waitForTimeout(900);
  const dsl = await page.locator("output").innerText();
  if (!dsl.trim() || dsl.trim() === "—") throw new Error("the builder emitted no DSL");
  console.log(`     builder emitted: ${dsl.replace(/\s+/g, " | ").trim()}`);
  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/calendars") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Create calendar" }).click(),
  ]);
  await page.waitForURL(/\/calendars\/.+/);
});

await step("a calendar nothing references reports itself unused", async () => {
  await page.goto(`${base}/calendars`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1000);
  const text = await page.locator("tbody tr", { hasText: CAL }).innerText();
  if (!text.includes("unused")) throw new Error(`expected "unused", got: ${text.replace(/\s+/g, " ")}`);
});

await step("bind a job's schedule to it", async () => {
  await page.goto(`${base}/jobs`, { waitUntil: "networkidle" });
  await page.getByRole("button", { name: "New job" }).first().click();
  await page.getByPlaceholder("demo:report").fill(CAL_JOB);
  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/jobs") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Create job" }).click(),
  ]);
  await page.waitForTimeout(1100);
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await page.getByPlaceholder("0 3 * * *").fill("*/5 * * * *");
  // Addressed by its accessible name, which is also the assertion that it has
  // one — Nuxt UI names a select trigger "Show popup" unless told otherwise.
  await page.getByRole("button", { name: "Calendar this schedule is gated by" }).click();
  await page.waitForTimeout(500);
  await page.getByRole("option", { name: CAL }).click();
  await page.waitForTimeout(300);
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/schedules") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Add schedule" }).click(),
  ]);
  await page.waitForTimeout(900);
});

await step("the calendar detail names the job it gates", async () => {
  await page.goto(`${base}/calendars`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1400);
  const row = page.locator("tbody tr", { hasText: CAL });
  if ((await row.innerText()).includes("unused")) throw new Error("still reported as unused");
  await row.click();
  await page.waitForTimeout(1400);
  const detail = await page.locator('aside[aria-label="Calendar detail"]').innerText();
  if (!detail.includes(CAL_JOB)) throw new Error("the gated job is missing from the detail");
});

await step("editing a calendar opens in the builder, not the raw box", async () => {
  await page.getByRole("button", { name: "Edit" }).click();
  await page.waitForTimeout(1400);
  const toggle = await page
    .getByRole("button", { name: /Edit as text|Back to the builder/ })
    .innerText();
  if (!toggle.includes("Edit as text")) throw new Error(`opened in raw mode ("${toggle}")`);
  const dsl = await page.locator("output").innerText();
  if (!dsl.trim() || dsl.trim() === "—") throw new Error("the builder did not seed from stored DSL");
  console.log(`     round-tripped: ${dsl.replace(/\s+/g, " | ").trim()}`);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
});

await step("delete the smoke calendar and its job", async () => {
  await page.goto(`${base}/jobs/${encodeURIComponent(CAL_JOB)}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: `Delete ${CAL_JOB}` }).click();
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/jobs/") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: "Delete", exact: true }).click(),
  ]);
  await page.waitForTimeout(900);
  await page.goto(`${base}/calendars`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1000);
  await page.locator("tbody tr", { hasText: CAL }).click();
  await page.waitForTimeout(1000);
  await page.getByRole("button", { name: `Delete ${CAL}` }).click();
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/calendars/") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: "Delete", exact: true }).click(),
  ]);
});

console.log(problems.length ? `\nconsole noise:\n  ${problems.join("\n  ")}` : "\nno console errors");
await browser.close();
