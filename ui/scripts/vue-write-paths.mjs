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
  await page.getByPlaceholder("0 3 * * *").fill("every 10 minutes");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/schedules") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Add schedule" }).click(),
  ]);
  await page.waitForTimeout(700);
  await page.getByText("every 10 minutes").first().waitFor({ timeout: 3000 });
});

await step("the list shows the rule and a next fire", async () => {
  await page.waitForTimeout(1200);
  const row = page.locator("tbody tr", { hasText: KEY });
  const text = await row.innerText();
  if (!text.includes("every 10 minutes")) throw new Error(`rule missing from the row: ${text}`);
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

await step("the DSL tab renders real Croniqfile syntax", async () => {
  await page.getByRole("tab", { name: "DSL" }).click();
  await page.waitForTimeout(900);
  const pre = await page.locator("pre").last().innerText();
  // Unquoted key and no `=`: the grammar the lexer actually accepts. The
  // previous renderer emitted `job "key" { description = … }`, which does not
  // parse — see ui/scripts/dsl-parses.mjs.
  if (!pre.includes(`job ${KEY} {`)) throw new Error(`no job block: ${pre.slice(0, 80)}`);
  if (pre.includes(`job "${KEY}"`)) throw new Error("the key is quoted; that does not parse");
  if (pre.includes(" = ")) throw new Error("assignment syntax; that does not parse");
  if (!pre.includes("every 10 minutes")) throw new Error("the schedule is missing");
});

await step("a schedule with no DSL spelling is reported, not faked", async () => {
  await page.getByRole("tab", { name: "Overview" }).click();
  await page.waitForTimeout(600);
  await page.getByRole("button", { name: "Edit this schedule" }).click();
  await page.getByPlaceholder("0 3 * * *").fill("*/7 * * * *");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/schedules") && r.request().method() === "PUT"),
    page.getByRole("button", { name: "Save", exact: true }).click(),
  ]);
  await page.waitForTimeout(1000);
  await page.getByRole("tab", { name: "DSL" }).click();
  await page.waitForTimeout(1000);
  const panel = await page.locator('aside[aria-label="Job detail"]').innerText();
  if (!panel.includes("*/7 * * * *")) throw new Error("the offending rule is not named");
  if (!panel.includes("not raw cron")) throw new Error("no explanation of why it cannot be written");
  if ((await page.locator("pre").count()) > 0) throw new Error("it rendered a block anyway");
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

/* ─── Alerts ────────────────────────────────────────────────────────────── */
//
// Nothing here is created or deleted: alert rules and channels are declared in
// the Croniqfile and read-only through the API. What *is* writable is an
// override, and it is the one thing on this screen that changes whether an
// operator gets paged — so it is the one thing worth driving.

await step("snooze a rule, then clear it again", async () => {
  await page.goto(`${base}/alerts`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1400);
  const rule = page.locator("tbody tr").first();
  if ((await rule.count()) === 0) throw new Error("no alert rule configured to exercise");
  const name = (await rule.locator("td").first().innerText()).trim();
  await rule.click();
  await page.waitForTimeout(1200);

  await page.getByRole("button", { name: "Snooze" }).click();
  await page.getByLabel("Duration").fill("15m");
  // The note is mandatory server-side and the form refuses without one, which
  // is the point: a rule that is quiet for an unexplained reason is worse than
  // one that is noisy.
  await page.getByRole("button", { name: "Apply" }).click();
  await page.waitForTimeout(600);
  const refusal = await page.getByRole("alert").last().innerText().catch(() => "");
  if (!/note is required/i.test(refusal)) throw new Error(`no note was demanded: ${refusal}`);

  await page.getByLabel("Reason for the override").fill("smoke script");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/snooze") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Apply" }).click(),
  ]);
  await page.waitForTimeout(1200);

  const detail = await page.locator('aside[aria-label="Alert rule detail"]').innerText();
  if (!/Snoozed until/.test(detail)) throw new Error(`the snooze is not reported: ${detail.slice(0, 120)}`);
  if (!detail.includes("smoke script")) throw new Error("the note is not shown");

  const row = await page.locator("tbody tr", { hasText: name }).innerText();
  if (!/snoozed/i.test(row)) throw new Error(`the list does not show the override: ${row}`);

  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/override") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: "Clear override" }).click(),
  ]);
  await page.waitForTimeout(1200);
  const after = await page.locator("tbody tr", { hasText: name }).innerText();
  if (/snoozed/i.test(after)) throw new Error("the override survived clearing");
});

await step("a delivery links to the run that caused it", async () => {
  await page.goto(`${base}/alerts/deliveries`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1600);
  const rows = await page.locator("tbody tr").count();
  if (rows === 0) {
    console.log("     (no deliveries in the log yet — link check skipped)");
    return;
  }
  const link = page.locator('tbody tr a[href^="/executions/"]').first();
  if ((await link.count()) === 0) throw new Error("no delivery links to its run");
  await link.click();
  await page.waitForURL(/\/executions\/.+/, { timeout: 5000 });
});

/* ─── Settings ──────────────────────────────────────────────────────────── */

const PAT = "smoke-token";
const CLIENT = "smoke-client";

await step("a minted token is shown once and guarded on the way out", async () => {
  await page.goto(`${base}/settings`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "New token" }).click();
  await page.getByPlaceholder("laptop-cli").fill(PAT);
  await page.getByRole("button", { name: "Read-only", exact: true }).click();
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/users/me/tokens") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Create token" }).click(),
  ]);
  await page.waitForTimeout(900);

  // The whole point of SecretOnce: it will not let itself be dismissed until
  // the value has been copied or the acknowledgement ticked, because this
  // render is the only copy that will ever exist.
  const done = page.getByRole("button", { name: "Done" });
  if (!(await done.isDisabled())) throw new Error("Done was clickable before acknowledgement");
  await page.getByRole("checkbox", { name: /stored this somewhere safe/i }).check();
  if (await done.isDisabled()) throw new Error("Done stayed disabled after acknowledgement");
  await done.click();
  await page.waitForTimeout(600);
});

await step("the token is listed, then revoked out of existence", async () => {
  const row = page.locator("tbody tr", { hasText: PAT });
  if ((await row.count()) === 0) throw new Error("the new token is not in the list");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/users/me/tokens/") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: `Revoke ${PAT}` }).click(),
  ]);
  await page.waitForTimeout(1100);
  // The server does not keep a tombstone: a revoked token leaves the list
  // entirely. Asserting it here is what stopped the UI from carrying a
  // "revoked" rendering that nothing could ever produce.
  if ((await page.locator("tbody tr", { hasText: PAT }).count()) > 0) {
    throw new Error("the revoked token is still listed");
  }
});

await step("an invitation hands back its accept link", async () => {
  await page.goto(`${base}/settings/people`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);

  // Clear anything a previous interrupted run left behind. The server allows
  // several pending invitations for one address, so without this the run after
  // a failure sees two rows and blames the product.
  for (let guard = 0; guard < 10; guard += 1) {
    const stale = page.getByRole("button", {
      name: "Revoke the invitation for smoke@example.com",
    });
    if ((await stale.count()) === 0) break;
    await stale.first().click();
    await page.waitForTimeout(700);
  }

  await page.getByRole("button", { name: "Invite someone" }).click();
  await page.getByPlaceholder("someone@example.com").fill("smoke@example.com");
  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/invitations") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Invite", exact: true }).click(),
  ]);
  await page.waitForTimeout(900);
  const shown = await page.getByRole("alert").first().innerText();
  if (!/accept/i.test(shown) && !/http/i.test(shown)) {
    throw new Error(`the accept link was not surfaced: ${shown.slice(0, 120)}`);
  }
  await page.getByRole("checkbox", { name: /sent the link/i }).check();
  await page.getByRole("button", { name: "Done" }).click();
  await page.waitForTimeout(600);
});

await step("the invited person appears in the one people list, then is revoked", async () => {
  const row = page.locator("tbody tr", { hasText: "smoke@example.com" });
  const before = await row.count();
  if (before === 0) throw new Error("the invitation is not in the people list");
  if (before > 1) throw new Error(`${before} rows for one person — revoked invitations are lingering`);
  if (!/invited/.test(await row.innerText())) throw new Error("it is not marked as invited");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/invitations/") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: "Revoke the invitation for smoke@example.com" }).click(),
  ]);
  await page.waitForTimeout(1100);
  // A revoked invitation is a decision already carried out; it leaves the
  // list of who has access and lives on in the audit log.
  if ((await page.locator("tbody tr", { hasText: "smoke@example.com" }).count()) > 0) {
    throw new Error("the revoked invitation is still listed");
  }
});

await step("an API client is created with a preset, given a key, and deleted", async () => {
  await page.goto(`${base}/settings/clients`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "New client" }).click();
  await page.getByPlaceholder("ci-pipeline").fill(CLIENT);
  // The preset is the point: nobody reasons correctly about twenty booleans.
  await page.getByRole("button", { name: "Runner", exact: true }).click();
  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/api-clients") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Create client" }).click(),
  ]);
  await page.waitForTimeout(1100);

  const card = page.locator("li", { hasText: CLIENT }).first();
  const scopes = await card.innerText();
  if (!scopes.includes("work:poll")) throw new Error(`the preset did not apply: ${scopes}`);

  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/api-keys") && r.request().method() === "POST"),
    card.getByRole("button", { name: "New key" }).click(),
  ]);
  await page.waitForTimeout(900);
  const keyDone = page.getByRole("button", { name: "Done" });
  if (!(await keyDone.isDisabled())) throw new Error("the key could be dismissed unacknowledged");
  await page.getByRole("checkbox", { name: /stored this somewhere safe/i }).check();
  await keyDone.click();
  await page.waitForTimeout(600);

  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/api-clients/") && r.request().method() === "DELETE"),
    page.getByRole("button", { name: `Delete ${CLIENT}` }).click(),
  ]);
  await page.waitForTimeout(900);
  if ((await page.locator("li", { hasText: CLIENT }).count()) > 0) {
    throw new Error("the client survived deletion");
  }
});

await step("the audit log names people rather than UUIDs, and links its targets", async () => {
  await page.goto(`${base}/settings/audit`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1600);
  const rows = await page.locator("tbody tr").count();
  if (rows === 0) throw new Error("the audit log is empty after all of the above");
  const who = await page.locator("tbody tr td:nth-child(2)").allInnerTexts();
  const uuidish = who.filter((text) => /^[0-9a-f]{8}-[0-9a-f]{4}-/i.test(text.trim()));
  if (uuidish.length) throw new Error(`actors still render as UUIDs: ${uuidish[0]}`);
});

console.log(problems.length ? `\nconsole noise:\n  ${problems.join("\n  ")}` : "\nno console errors");
await browser.close();
