// Does the list scroll, or does the page?
//
// The shell owns exactly one scroll region and every list page is sized to fit
// it, so that filters, column heads and the maintenance banner stay put while
// the table body moves (docs/ui-visual-design.md, Durchgang 5). That is a
// claim about geometry, so it gets measured rather than eyeballed: scroll the
// deepest overflow container as far as it goes, then check that the document
// did not move and the chrome above the list did not shift by a pixel.
//
// Usage (with `node scripts/dev-stack.mjs` running, from ui/):
//   node scripts/measure-scroll.mjs
//   CRONIQ_VIEWPORT_H=520 node scripts/measure-scroll.mjs
//
// The short viewport is the one that proves anything: at 900px the demo data
// fits on most screens and nothing overflows at all.
//
// Like capture-screens.mjs this is a tool for checking, not a CI gate — it
// needs the dev stack and real data.

import { chromium } from "@playwright/test";

const base = `http://127.0.0.1:${process.env.CRONIQ_DEV_VUE_PORT ?? 4232}`;

const browser = await chromium.launch();
// The viewport is a parameter because the interesting case is the short one:
// at 900px the demo data fits on most screens and nothing overflows, which
// proves nothing. `CRONIQ_VIEWPORT_H=520` forces every page to overflow.
const height = Number(process.env.CRONIQ_VIEWPORT_H ?? 900);
const page = await browser.newPage({ viewport: { width: 1440, height } });

await page.goto(`${base}/login`, { waitUntil: "networkidle" });
await page.locator('input[autocomplete="username"]').fill("admin");
await page.locator('input[autocomplete="current-password"]').fill("demo-admin");
await Promise.all([
  page.waitForResponse((r) => r.url().includes("/v1/auth/login") && r.request().method() === "POST"),
  page.locator('button[type="submit"]').click(),
]);
await page.waitForSelector("nav");

for (const [name, path, chrome] of [
  ["Runs", "/executions", '[aria-label="Filter by state"]'],
  ["Runners", "/runners", "nav"],
  ["Dead letters", "/dead-letters", "nav"],
  ["Dashboard", "/", "nav"],
]) {
  // Not `networkidle`: /runners holds an SSE stream open, so the network is
  // never idle there and waiting for it is waiting forever.
  await page.goto(`${base}${path}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1800);

  const before = await page.evaluate(() => ({
    doc: {
      scroll: document.documentElement.scrollHeight,
      client: document.documentElement.clientHeight,
      top: window.scrollY,
    },
    // Every element that can actually scroll vertically, deepest last.
    scrollers: [...document.querySelectorAll("*")]
      .filter((el) => {
        const overflow = getComputedStyle(el).overflowY;
        return (
          (overflow === "auto" || overflow === "scroll") &&
          el.scrollHeight > el.clientHeight + 1
        );
      })
      .map((el) => ({
        tag: el.tagName.toLowerCase(),
        cls: el.className.toString().slice(0, 60),
        over: el.scrollHeight - el.clientHeight,
      })),
  }));

  const chromeBefore = await page.locator(chrome).first().boundingBox();

  // Scroll the deepest scroller as far as it goes.
  await page.evaluate(() => {
    const els = [...document.querySelectorAll("*")].filter((el) => {
      const overflow = getComputedStyle(el).overflowY;
      return (overflow === "auto" || overflow === "scroll") && el.scrollHeight > el.clientHeight + 1;
    });
    const target = els[els.length - 1];
    if (target) target.scrollTop = target.scrollHeight;
    window.scrollTo(0, document.documentElement.scrollHeight);
  });
  await page.waitForTimeout(300);

  const chromeAfter = await page.locator(chrome).first().boundingBox();
  const docTop = await page.evaluate(() => window.scrollY);

  const pageScrolls = before.doc.scroll > before.doc.client + 1;
  const drift =
    chromeBefore && chromeAfter ? Math.round((chromeAfter.y - chromeBefore.y) * 100) / 100 : "n/a";

  console.log(`\n${name}  (${path})`);
  console.log(`  document scrollHeight/clientHeight : ${before.doc.scroll}/${before.doc.client}` +
    `  -> page ${pageScrolls ? "SCROLLS" : "does not scroll"}`);
  console.log(`  window.scrollY after scrolling     : ${docTop}`);
  console.log(`  chrome (${chrome}) drift           : ${drift}px`);
  for (const s of before.scrollers) {
    console.log(`  scroller: <${s.tag}> ${s.over}px over  .${s.cls}`);
  }
}

await browser.close();
