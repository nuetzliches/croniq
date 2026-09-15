#!/usr/bin/env node
/**
 * Fail the build if the dashboard bundle names an origin other than its own
 * (ADR-0005).
 *
 * This exists because the way that constraint broke was invisible: a
 * dependency resolved its assets over the network at runtime, and the only
 * symptom anyone saw was icons missing behind a CSP violation. Nothing in the
 * source said `api.iconify.design`; it arrived through a default. A grep over
 * the built output is the one check that sees what actually ships rather than
 * what the source asks for.
 *
 * It is deliberately blunt: every absolute URL in `dist/` has to be on the
 * list below, with a reason. That over-reports — a URL in an error message is
 * not a fetch — and the over-reporting is the point. A new entry costs one
 * line and forces someone to look at it; the alternative, guessing which
 * string literals a browser would dereference, cannot be done statically.
 *
 * The check has a second half, for the other way the same constraint breaks:
 * an icon the build fails to bundle no longer falls back to anything, it is
 * simply not drawn. So every icon name the source writes is also looked for in
 * the output.
 *
 * Run: `npm run check:sources` (after `npm run build`), and in CI.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const DIST = fileURLToPath(new URL('../dist', import.meta.url))
const APP = fileURLToPath(new URL('../app', import.meta.url))
const UI_ROOT = fileURLToPath(new URL('..', import.meta.url))

/**
 * Absolute URLs allowed to appear in the built output, each with the reason it
 * is not a load. Matched as a prefix against the URL's origin + path.
 *
 * Nothing here is fetched by the browser. If an entry ever would be, it does
 * not belong on this list — it belongs in an ADR superseding 0005.
 */
const ALLOWED = [
  {
    prefix: 'http://www.w3.org/',
    why: 'XML namespace identifiers (`svg`, `xlink`, `MathML`). Namespaces are names, not addresses; no browser resolves them.',
  },
  {
    prefix: 'https://vuejs.org/error-reference/',
    why: "Vue's runtime builds this link into the text of an uncaught-error warning. A string printed to the console, not a resource.",
  },
  {
    prefix: 'https://tailwindcss.com',
    why: "Tailwind's license banner, emitted as a CSS comment at the top of the stylesheet.",
  },
  {
    prefix: 'http://localhost:4000',
    why: "Sample output in the login screen's scripted console (`LoginConsole.vue`) — text of a fake `croniq status`, and the default port at that.",
  },
]

const SCANNED_EXTENSIONS = ['.js', '.mjs', '.css', '.html', '.json', '.webmanifest', '.svg']

/** Absolute URLs, deliberately greedy about what counts as one. */
const URL_PATTERN = /https?:\/\/[^\s"'`)\\<>]+/g

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) {
      yield* walk(path)
    } else if (SCANNED_EXTENSIONS.some((ext) => entry.endsWith(ext))) {
      yield path
    }
  }
}

function isAllowed(url) {
  return ALLOWED.some(({ prefix }) => url.startsWith(prefix))
}

let distExists = true
try {
  statSync(DIST)
} catch {
  distExists = false
}

if (!distExists) {
  console.error(`No build to check at ${relative(UI_ROOT, DIST)}. Run \`npm run build\` first.`)
  process.exit(2)
}

/** @type {Map<string, Set<string>>} url -> files it appears in */
const offenders = new Map()

for (const file of walk(DIST)) {
  const contents = readFileSync(file, 'utf8')
  for (const match of contents.matchAll(URL_PATTERN)) {
    const url = match[0]
    if (isAllowed(url)) continue
    const seen = offenders.get(url) ?? new Set()
    seen.add(relative(UI_ROOT, file))
    offenders.set(url, seen)
  }
}

/*
 * Second half: every icon the source names has to be in the bundle.
 *
 * `icon.clientBundle.scan` in `vite.config.ts` finds these by reading the
 * source, and its file globs are a setting someone can outgrow — the default
 * set skips `.ts`, which is why the sidebar's icons were the ones missing. A
 * name the scanner never read produces no warning and no request; it produces
 * an icon that is not there.
 *
 * Bundled icons land in the output as JSON keyed by name inside their
 * collection, so the name followed by its opening brace is what to look for.
 * Matched against the whole of `dist/` rather than a particular chunk: which
 * chunk they land in is Rollup's business.
 */
const ICON_PATTERN = /\bi-[a-z0-9]+(?:-[a-z0-9]+)+\b/g
const SOURCE_EXTENSIONS = ['.vue', '.ts']

function* walkSource(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) {
      yield* walkSource(path)
    } else if (SOURCE_EXTENSIONS.some((ext) => entry.endsWith(ext))) {
      yield path
    }
  }
}

/** @type {Map<string, Set<string>>} `i-…` name -> the files that write it */
const usedIcons = new Map()

for (const file of walkSource(APP)) {
  // The shim's own tests name icons on purpose that are meant not to resolve.
  if (file.endsWith('.test.ts')) continue
  for (const match of readFileSync(file, 'utf8').matchAll(ICON_PATTERN)) {
    const seen = usedIcons.get(match[0]) ?? new Set()
    seen.add(relative(UI_ROOT, file))
    usedIcons.set(match[0], seen)
  }
}

const bundled = [...walk(DIST)]
  .filter((file) => file.endsWith('.js'))
  .map((file) => readFileSync(file, 'utf8'))
  .join('\n')

/** @type {Map<string, Set<string>>} */
const missingIcons = new Map()
for (const [icon, files] of usedIcons) {
  // `i-lucide-calendar-days` is collection `lucide`, icon `calendar-days`, but
  // where that split falls is not knowable from the name alone. Accept any
  // split the bundle can answer.
  const name = icon.slice('i-'.length)
  const candidates = []
  for (let at = name.indexOf('-'); at !== -1; at = name.indexOf('-', at + 1)) {
    candidates.push(name.slice(at + 1))
  }
  if (!candidates.some((candidate) => bundled.includes(`"${candidate}":{`))) {
    missingIcons.set(icon, files)
  }
}

if (offenders.size === 0 && missingIcons.size === 0) {
  console.log(
    `Local sources only: no unexpected origin in dist/, and all ${usedIcons.size} icons are bundled (ADR-0005).`,
  )
  process.exit(0)
}

if (offenders.size > 0) {
  console.error('The dashboard bundle names origins it is not allowed to (ADR-0005):\n')
  for (const [url, files] of [...offenders].sort()) {
    console.error(`  ${url}`)
    for (const file of files) {
      console.error(`      in ${file}`)
    }
  }
  console.error(
    '\nThe dashboard must resolve every resource from its own origin. Either bundle\n' +
      'the asset at build time, or — if this URL is a string the browser never\n' +
      'dereferences — add it to ALLOWED in scripts/check-local-sources.mjs with the\n' +
      'reason it is not a load.\n',
  )
}

if (missingIcons.size > 0) {
  console.error('Icons named in the source are missing from the bundle (ADR-0005):\n')
  for (const [icon, files] of [...missingIcons].sort()) {
    console.error(`  ${icon}`)
    for (const file of files) {
      console.error(`      in ${file}`)
    }
  }
  console.error(
    '\nThese render as nothing — there is no network fallback, by design. Check that\n' +
      "the icon's `@iconify-json/*` collection is installed, that the name is spelled\n" +
      'the way that collection spells it, and that `icon.clientBundle.scan` in\n' +
      'vite.config.ts reads the file it is written in.',
  )
}

process.exit(1)
