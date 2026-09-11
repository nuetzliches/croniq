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

/* ─── Console ───────────────────────────────────────────────────────────── */
//
// Nothing is written here, but the tail has state a screenshot cannot show:
// what pausing does with the events that keep arriving, and whether the level
// filter actually narrows the view.

await step("the console tails, filters and pauses without losing the tail", async () => {
  await page.goto(`${base}/console`, { waitUntil: "domcontentloaded" });
  // The demo server speaks roughly once a minute, so give it a moment. The
  // first connect asks for a snapshot, which is what makes this bounded.
  await page.waitForTimeout(6000);

  const rows = () => page.locator('[role="log"] > div').count();
  const initial = await rows();
  if (initial === 0) throw new Error("no events arrived on the stream");

  // Levels filter client-side: the count changes without the stream moving.
  await page.getByRole("button", { name: "info", exact: true }).click();
  await page.waitForTimeout(400);
  const withoutInfo = await rows();
  if (withoutInfo >= initial) throw new Error("turning off a level did not narrow the view");
  await page.getByRole("button", { name: "info", exact: true }).click();
  await page.waitForTimeout(400);

  // Pausing must hold what arrives, not drop it.
  await page.getByRole("button", { name: "Pause the console" }).click();
  await page.waitForTimeout(500);
  const frozen = await rows();
  await page.waitForTimeout(3000);
  if ((await rows()) !== frozen) throw new Error("the view moved while paused");
  await page.getByRole("button", { name: "Resume the console" }).click();
  await page.waitForTimeout(600);

  await page.getByRole("button", { name: "Clear the console" }).click();
  await page.waitForTimeout(400);
  if ((await rows()) > 1) throw new Error("clear left events behind");
});

/* ─── Account recovery ──────────────────────────────────────────────────── */
//
// The links the server emails have to land somewhere. Both of them —
// `/invitations/accept` and `/password-reset/confirm` — were dead ends in both
// dashboards until this tree served them, so this walks a whole invitation
// from "invite someone" to "that person is signed in".

const WHO = `smoke-invitee-${Date.now().toString(36)}`;
let acceptUrl = "";

await step("an invitation hands back a link", async () => {
  await page.goto(`${base}/settings/people`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "Invite someone" }).click();
  await page.getByPlaceholder("someone@example.com").fill(`${WHO}@example.com`);
  await Promise.all([
    page.waitForResponse((r) => r.url().endsWith("/v1/invitations") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Invite", exact: true }).click(),
  ]);
  await page.waitForTimeout(900);
  acceptUrl = (await page.locator("pre").first().innerText()).trim();
  if (!/\/invitations\/accept\?token=/.test(acceptUrl)) {
    throw new Error(`not an accept link: ${acceptUrl.slice(0, 80)}`);
  }
  console.log(`     link: ${acceptUrl.replace(/token=.*/, "token=…")}`);
});

await step("the link lands on a real page, not the not-found", async () => {
  // Point it at the dev server rather than the server's own origin.
  const url = new URL(acceptUrl);
  const local = `${base}${url.pathname}${url.search}`;
  await page.goto(local, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  const body = await page.locator("body").innerText();
  if (/not found/i.test(body)) throw new Error("the accept link still opens the not-found page");
  if (!/Accept your invitation/i.test(body)) throw new Error(`unexpected page: ${body.slice(0, 80)}`);
});

await step("the password rules are stated before the round trip", async () => {
  await page.getByRole("textbox", { name: "Username" }).fill(WHO);
  await page.getByLabel("Password", { exact: true }).fill("short");
  await page.getByLabel("Repeat it").fill("short");
  await page.getByRole("button", { name: "Create my account" }).click();
  await page.waitForTimeout(500);
  const alert = await page.getByRole("alert").first().innerText();
  if (!/at least 8/i.test(alert)) throw new Error(`no local rule check: ${alert}`);
});

await step("accepting creates the account", async () => {
  await page.getByLabel("Password", { exact: true }).fill("smoke-password-1");
  await page.getByLabel("Repeat it").fill("smoke-password-1");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/invitations/accept") && r.request().method() === "POST"),
    page.getByRole("button", { name: "Create my account" }).click(),
  ]);
  await page.waitForTimeout(1000);
  const body = await page.locator("body").innerText();
  if (!/Account created/i.test(body)) throw new Error(`accept did not succeed: ${body.slice(0, 120)}`);
});

await step("the new account can sign in", async () => {
  const fresh = await browser.newContext();
  const p2 = await fresh.newPage();
  await p2.goto(`${base}/login`, { waitUntil: "networkidle" });
  await p2.locator('input[autocomplete="username"]').fill(WHO);
  await p2.locator('input[autocomplete="current-password"]').fill("smoke-password-1");
  const [res] = await Promise.all([
    p2.waitForResponse((r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST"),
    p2.locator('button[type="submit"]').click(),
  ]);
  if (res.status() !== 200) throw new Error(`sign-in failed: ${res.status()}`);
  await p2.waitForSelector("nav", { timeout: 8000 });
  await fresh.close();
});

await step("a reset request is offered and says the same either way", async () => {
  const anon = await browser.newContext();
  const p3 = await anon.newPage();
  await p3.goto(`${base}/login`, { waitUntil: "networkidle" });
  await p3.waitForTimeout(900);
  await p3.locator('input[autocomplete="username"]').fill("no-such-user-at-all");
  await Promise.all([
    p3.waitForResponse((r) => r.url().includes("/v1/auth/password-reset/request")),
    p3.getByRole("button", { name: /Forgot your password/i }).click(),
  ]);
  await p3.waitForTimeout(700);
  const notice = await p3.getByRole("status").last().innerText();
  if (!/if that account exists/i.test(notice)) throw new Error(`leaky wording: ${notice}`);
  await anon.close();
});

await step("a spent or bogus reset token is reported honestly", async () => {
  await page.goto(`${base}/password-reset/confirm?token=definitely-not-a-token`, {
    waitUntil: "networkidle",
  });
  await page.waitForTimeout(800);
  await page.getByLabel("New password").fill("another-password-1");
  await page.getByLabel("Repeat it").fill("another-password-1");
  await page.getByRole("button", { name: "Set password" }).click();
  await page.waitForTimeout(1000);
  const alert = await page.getByRole("alert").first().innerText();
  // It must name the remedy, not merely report a refusal.
  if (!/no longer usable/i.test(alert)) throw new Error(`unhelpful: ${alert}`);
  if (!/request a new one/i.test(alert)) throw new Error(`no remedy offered: ${alert}`);
  if (/refused that/i.test(alert)) throw new Error(`fell through to the generic message: ${alert}`);
  console.log(`     said: ${alert.replace(/\s+/g, " ").slice(0, 90)}`);
});

// Clean up: remove the user this created.
await step("clean up the smoke account", async () => {
  await page.goto(`${base}/settings/people`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1400);
  const remove = page.getByRole("button", { name: `Remove ${WHO}` });
  if ((await remove.count()) === 0) throw new Error("the new user is not in the people list");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/v1/users/") && r.request().method() === "DELETE"),
    remove.click(),
  ]);
  await page.waitForTimeout(800);
});

/* ─── Command palette ───────────────────────────────────────────────────── */

await step("the palette opens, finds a job by name, and goes there", async () => {
  await page.goto(`${base}/`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);
  await page.keyboard.press("Control+k");
  await page.waitForTimeout(600);
  await page.getByPlaceholder(/Search jobs, runners/).fill("heartbeat");
  await page.waitForTimeout(600);
  const options = await page.getByRole("option").allInnerTexts();
  if (!options.some((t) => t.includes("demo:heartbeat"))) {
    throw new Error(`no job match: ${options.join(" | ")}`);
  }
  await page.keyboard.press("Enter");
  await page.waitForTimeout(900);
  if (!/\/jobs\/demo%3Aheartbeat/.test(page.url())) throw new Error(`went to ${page.url()}`);
});

await step("the g chord works, unlike the hints it replaces", async () => {
  // The React palette printed `G D`, `G J` and the rest beside its entries
  // while nothing implemented them. These are printed because they work.
  await page.goto(`${base}/`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  await page.keyboard.press("g");
  await page.waitForTimeout(300);
  if ((await page.getByRole("status").filter({ hasText: "then d" }).count()) === 0) {
    throw new Error("nothing showed that the chord was armed");
  }
  await page.keyboard.press("r");
  await page.waitForTimeout(800);
  if (new URL(page.url()).pathname !== "/executions") throw new Error(`g r went to ${page.url()}`);
});

await step("the chord never steals a key from a field", async () => {
  await page.goto(`${base}/jobs`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1000);
  const box = page.getByPlaceholder("Job key or description…");
  await box.fill("");
  await box.type("gr");
  await page.waitForTimeout(600);
  if (new URL(page.url()).pathname !== "/jobs") throw new Error(`typing navigated to ${page.url()}`);
  if ((await box.inputValue()) !== "gr") throw new Error("the field lost the keystrokes");
});

await step("an abandoned chord expires rather than arming forever", async () => {
  await page.goto(`${base}/`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);
  await page.keyboard.press("g");
  await page.waitForTimeout(1600);
  await page.keyboard.press("r");
  await page.waitForTimeout(600);
  if (new URL(page.url()).pathname !== "/") throw new Error(`a stale chord fired: ${page.url()}`);
});

await step("what fires next lists only jobs that still exist", async () => {
  // `/v1/jobs/states` outlives the job on purpose (#470). The rail used to
  // read it alone, so deleted jobs sat in "what fires next" forever.
  await page.goto(`${base}/`, { waitUntil: "networkidle" });
  await page.waitForTimeout(2000);
  const railText = await page.locator("section", { hasText: "NEXT HOUR" }).first().innerText();
  for (const ghost of ["smoke:", "probe"]) {
    if (railText.includes(ghost)) throw new Error(`a deleted job is listed: ${railText}`);
  }
});

/* ─── Runs: time window and paging ──────────────────────────────────────── */

const count = () => page.locator("tbody tr").count();

await step("the list starts capped at one page", async () => {
  await page.goto(`${base}/executions`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1800);
  const n = await count();
  if (n !== 200) throw new Error(`expected a full first page of 200, got ${n}`);
});

await step("load older reaches past the cap", async () => {
  const before = await count();
  await page.getByRole("button", { name: "Load older" }).click();
  await page.waitForTimeout(1800);
  const after = await count();
  if (after <= before) throw new Error(`still ${after} rows — paging added nothing`);
  console.log(`     ${before} -> ${after} rows`);
});

await step("no row is shown twice despite the inclusive cursor", async () => {
  const ids = await page.locator("tbody tr td:nth-child(3)").allInnerTexts();
  const trimmed = ids.map((t) => t.trim());
  const dupes = trimmed.filter((v, i) => trimmed.indexOf(v) !== i);
  if (dupes.length) throw new Error(`duplicated rows: ${[...new Set(dupes)].slice(0, 3).join(", ")}`);
});

await step("the window filter narrows, and lives in the URL", async () => {
  await page.goto(`${base}/executions?window=1h`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1800);
  const windowed = await count();
  await page.goto(`${base}/executions`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1800);
  const unbounded = await count();
  if (windowed >= unbounded) throw new Error(`1h window (${windowed}) did not narrow ${unbounded}`);
  console.log(`     last hour: ${windowed} rows, unbounded: ${unbounded}`);
});

await step("changing a filter starts the paging over", async () => {
  await page.goto(`${base}/executions`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1600);
  await page.getByRole("button", { name: "Load older" }).click();
  await page.waitForTimeout(1600);
  const paged = await count();
  await page.getByRole("button", { name: "Filter by time window" }).click();
  await page.waitForTimeout(400);
  await page.getByRole("option", { name: "Last hour" }).click();
  await page.waitForTimeout(1800);
  const after = await count();
  if (after >= paged) throw new Error(`filter kept ${after} rows from the previous paging`);
});

console.log(problems.length ? `\nconsole noise:\n  ${problems.join("\n  ")}` : "\nno console errors");
await browser.close();
