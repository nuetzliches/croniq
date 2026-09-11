// Drive the Vue dashboard's write paths against a live dev stack.
//
// A stopgap, and labelled as one. ADR-0004 makes the Playwright suite the
// acceptance gate for the rebuild, but that suite runs against the *built*
// React dashboard the server serves on 4233 — the Vue tree has no e2e project
// yet (see docs/ui-visual-design.md, "Noch offen"). Until it does, the screens
// that create, edit and delete things have no automated check at all, and a
// broken mutation is invisible in a screenshot.
//
// So this walks the paths a screenshot cannot: create a job, attach a
// schedule, disable and re-enable it, trigger it, pause and resume, edit,
// render its DSL, and delete it again — reporting any 4xx the page made along
// the way. It cleans up after itself; the job it creates is `smoke:vue-jobs`.
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

console.log(problems.length ? `\nconsole noise:\n  ${problems.join("\n  ")}` : "\nno console errors");
await browser.close();
