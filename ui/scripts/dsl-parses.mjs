// Does the DSL tab emit text that croniq can actually read?
//
// It did not. The renderer was ported from the React tree, where it had been
// hand-assembling something that looked like the DSL — `job "key" {`,
// `description = "…"`, `tags = [ … ]` — and the real lexer rejects that on the
// second line. The tab called it "the job as it is written"; pasting it into a
// Croniqfile produced a parse error.
//
// So this closes the loop the only way that settles it: read what the running
// dashboard puts on screen, hand it to the same binary an operator would, and
// report what the binary says.
//
// Usage (with `node scripts/dev-stack.mjs` running, from ui/):
//   node scripts/dsl-parses.mjs
//
// Needs a debug or release build of croniq. Not in CI — it needs both the dev
// stack and the binary; the unit tests cover the assembly around the
// formatter, and the formatter itself parses its own output in Rust.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const exe = process.platform === "win32" ? ".exe" : "";
const croniq = [
  path.join(ROOT, "target", "debug", `croniq${exe}`),
  path.join(ROOT, "target", "release", `croniq${exe}`),
].find((candidate) => fs.existsSync(candidate));

if (!croniq) {
  console.error("no croniq binary in target/debug or target/release — cargo build first");
  process.exit(1);
}

const base = `http://127.0.0.1:${process.env.CRONIQ_DEV_VUE_PORT ?? 4232}`;
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "croniq-dsl-"));

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

await page.goto(`${base}/login`, { waitUntil: "networkidle" });
await page.locator('input[autocomplete="username"]').fill("admin");
await page.locator('input[autocomplete="current-password"]').fill("demo-admin");
await Promise.all([
  page.waitForResponse((r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST"),
  page.locator('button[type="submit"]').click(),
]);

await page.goto(`${base}/jobs`, { waitUntil: "networkidle" });
await page.waitForTimeout(1500);

const jobs = await page.evaluate(() =>
  [...document.querySelectorAll("tbody tr td:nth-child(2)")].map((cell) =>
    cell.querySelector("span")?.textContent.trim(),
  ),
);

const keys = jobs.filter(Boolean);
// A run that silently checks nothing is the failure mode this script exists to
// catch, so an empty list is an error rather than a pass.
if (keys.length === 0) {
  console.error("no jobs found on /jobs — nothing was checked");
  await browser.close();
  process.exit(1);
}
console.log(`checking ${keys.length} job block(s)
`);

let failures = 0;
for (const key of keys) {
  await page.goto(`${base}/jobs/${encodeURIComponent(key)}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);
  await page.getByRole("tab", { name: "DSL" }).click();
  await page.waitForTimeout(900);

  const block = await page
    .locator("pre")
    .last()
    .innerText()
    .catch(() => "");
  const notes = await page
    .locator('[role="alert"], aside [data-slot="description"]')
    .allInnerTexts()
    .catch(() => []);

  if (!block.trim()) {
    console.log(`skip ${key}: nothing rendered${notes.length ? ` — ${notes[0].slice(0, 80)}` : ""}`);
    continue;
  }

  const file = path.join(tmp, `${key.replace(/[^\w.-]/g, "_")}.croniq`);
  fs.writeFileSync(file, block.endsWith("\n") ? block : `${block}\n`, "utf8");
  const run = spawnSync(croniq, ["validate", file], { encoding: "utf8" });

  if (run.status === 0) {
    console.log(`ok   ${key}`);
  } else {
    failures += 1;
    console.log(`FAIL ${key}`);
    console.log(
      (run.stderr || run.stdout || "")
        .split("\n")
        .slice(0, 8)
        .map((line) => `       ${line}`)
        .join("\n"),
    );
  }
}

console.log(failures ? `\n${failures} job block(s) do not parse` : "\nevery rendered block parses");
await browser.close();
process.exit(failures ? 1 : 0);
