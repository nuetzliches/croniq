# Visual design — stocktake and direction

As of 2026-09-10.

This document exists because it was missing.
[ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md) recorded that the
design phase would deliver "the design system, the component library and the
screen inventory". What was delivered was the library (Nuxt UI 4) and the inventory
([`ui-screen-inventory.md`](ui-screen-inventory.md)). **The design itself was
never decided by anyone** — and because it was on no list, the gap looked
like "still to come" rather than "nobody is responsible for this".

The Vue tree is **not a prototype**. ADR-0004 makes it the replacement for the
shipping dashboard; it has to reach that dashboard's level.

---

## Stocktake

Until now I had only read the React dashboard in source. That is no
basis for "no regression", so I captured it and looked at it —
all eleven screens, signed in, with the demo data of the dev stack.
`ui/scripts/capture-screens.mjs` makes that reproducible. (The script took
a tree parameter back then; since the cutover there is only one.)

### What it does well — the list nothing may fall short of

1. **Card chrome on a gradient ground.** Sidebar, topbar and content are
   rounded cards with an outer margin on a ground running from violet to
   blue. That is distinctive and does not look like a Bootstrap admin.
2. **A KPI language of its own.** An uppercase micro-label with letter spacing, a large
   number, an explanatory subline, an embedded sparkline. Applied consistently on the
   dashboard and the job detail.
3. **Tag chips with counters** (`env=demo 5`, `kind=ops 2`) as a filter bar.
4. **Job rows carry their history.** Every row in the job list shows a
   bar sparkline of the recent runs plus a success rate. A great deal of information
   in very little space.
5. **Definition lists as the detail rail** — label on the left, value on the right,
   monospace where they are values. Excellent to skim.
6. **The console.** A dark terminal panel inside the light chrome, timestamp /
   level / target / message in columns, structured fields dimmed and
   appended, level chips with a colour dot. The strongest screen.
7. **Login as a product page.** A headline ("Schedule. Observe. Recover."),
   positioning copy, **real live metrics from the public
   `/health`** and an animated terminal. You can see before signing in that
   the server is alive.
8. **A command palette** (Ctrl K) in the topbar.
9. Status pills, monospace IDs as links, relative times, right-aligned
   durations.

### Where it is weak

1. **Master/detail is applied uniformly, even where there is nothing to show.**
   On *Executions* the detail panel stands empty and claims ~60% of the
   area; on *Runners* with one runner it is ~85%. The gradient then
   dominates an area it does not frame but fills.
2. **The executions list is the least economical surface in the app.**
   Every execution is a three-line card (~88 px) — seven are visible.
   As a table it would be twenty-five. And because there are no columns, you
   cannot compare durations against each other. Of all screens, this one becomes
   the *one* execution view per the inventory.
3. **A native `<select>`** as the status filter — unstyled, breaks with everything
   else on the page.
4. **Columns wrap.** "29m ago" runs onto two lines in the job detail, in
   every row. The density is intended, but not worked out to the end.
5. **The detail rail is cut off at the bottom**, with nothing indicating
   that there is more content below.

In summary: **strong information design, weak layout robustness.**
That is a good starting position — the hard part is there, what is missing is
craft.

---

## Carried over, not decided

The product identity exists and is not reinvented as long as nobody decides
otherwise:

- **Mark:** the orbit sign from `public/icons/mark-mono.svg`, ported as
  `app/components/BrandMark.vue`.
- **Brand colour:** `#6A54DF` as a ramp with 500 on the brand value.
- **Icon set:** carried over in full, including the manifest.

---

## Direction

### Keep

The card chrome including its ground, the KPI language, the tag chips, the history in
list rows, the definition lists, the console as a dark terminal, and the
login as a product page with real metrics.

### Change

- **Lists are lists.** Master/detail only where the detail is actually needed.
  Runs becomes a real table with columns; the detail opens as a side panel
  or its own route, instead of permanently occupying two thirds of the area empty.
- **One density, fixed in one place.** The table in the job detail has
  the right density; it becomes the benchmark, including column widths that
  do not wrap.
- **Empty states earn their area.** A symbol plus two lines in a
  60% panel is not an empty state but a gap with a label.
- **Filters are components**, not native controls.

### New impulses

The point I missed most while looking at it:

**Croniq shows the past everywhere and the future nowhere.** It is a
scheduler — the most interesting thing is what happens *next*. "NEXT FIRE in 33s"
exists exactly once, in the detail of a single job. A compact
preview rail ("what fires in the next hour") would be real new value
and uses data the server already delivers via `/v1/jobs/states` and the forecast.

Further candidates, more weakly argued:

- **Make the failure heatmap the main answer.** It answers "is something
  broken" better than any number, but is small, unlabelled and sits at the bottom
  right.
- **Keyboard first.** Ctrl K exists; `j`/`k` in lists and Enter to open
  suit an operations tool and cost little.
- **A density switch** (compact/comfortable). Operators disagree here,
  and the decision does not have to be taken globally.
- **Dark mode as an equal.** Today the app is light with a dark
  console. For a tool in which people spend hours, that is an
  open question, not an answer.

---

## Approach

In passes, with stopping points — not in one go. It is precisely that
sequence that gives an opportunity to correct course before a decision is baked into
seven screens.

| # | Content | Result you can judge it by |
|---|---|---|
| 1 | Stocktake and direction | this document |
| 2 ✓ | Foundation: chrome, ground, density, typography, states (empty/loading/error) | shell and login look like the product |
| 3 ✓ | Runs as the first real screen | the direction is proven on the hardest list |
| 4 ✓ | Dashboard, runners, dead letters | the building blocks hold |
| 5 | Jobs, then the rest | — |

After every pass: captures of both trees side by side
(`node ui/scripts/capture-screens.mjs …`), so that "no regression"
stays a finding and does not become a claim.

---

## Postscript: what the test net does not see

The Playwright suite asserts routes, session, URL contracts and SSE — none of it
appearance. That is deliberate ([ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md),
scope guard 2), but it had a consequence: for a while the theme was **not
active at all**. `@theme` instead of `@theme static` let Tailwind optimise the entire
colour ramp away, so that every `bg-primary` resolved to transparent. All
behavioural checks stayed green.

`ui/app/lib/theme.test.ts` closes the cheap half of that gap. The
expensive half — whether it *looks good* — is closed by no test, but by looking.
Hence the capture script.

---

## Pass 2 — what was decided

**Ground.** Tinted, not white, with two weak radial gradients in
opposite corners. The first attempt was 9% brand on white and therefore
invisible; but the cause lay deeper than the opacity — with a white ground
*and* white cards there is nothing for the cards to float above. Now
the ground is tinted and the cards are opaque.

**Dark is an equal.** Not inverted lightness, but a
ground of its own from the same two anchors with different weights. The React tree
is light with an embedded dark console; for a tool that is open all
day, that was worth the question.

**Density in one place.** `--cq-row-h` (2.375 rem), `--cq-cell-x`, `--cq-cell-y`
and the utilities `cq-label` / `cq-num`. The measurements come from the table in the
job detail of the React tree, which is the only one with the right density. `cq-num`
sets `tabular-nums` *and* `white-space: nowrap` — the latter against exactly the
defect the audit found: "29m ago" on two lines in every row.

**States.** `AppEmpty`, `AppLoading`, `AppError`. Two decisions in them
that go beyond cosmetics: `AppEmpty` has an `action` slot, because an
empty state that only reports emptiness wastes the one moment in which someone
is certainly looking for a next step. And `AppError` does **not** offer "Try again"
on 403 and 404 — a button that promises to repeat a final
answer trains people to press it pointlessly.
Server-side error messages are preserved; croniq's API says useful things,
and replacing that with a friendly generic makes your own backend
harder to operate.

**Login as a product page.** Headline, positioning and three tiles with
real numbers from the public `/health`, plus a build line. Carried over,
because the first question with self-hosted software is "is this even running"
and this page can answer it before you have credentials.

**Not carried over:** the simulated terminal that types a command
nobody runs. It is charming and inauthentic, and the honest version
of the same idea — the actual server state — is already standing next to it.

### Left open

- ~~Density switch~~ and ~~keyboard~~ — both landed in pass 3.
- **Command palette.** The React tree has one, this one does not yet.
- **The preview rail** ("what fires next") from the new impulses —
  belongs to the dashboard, so pass 4.

---

## Pass 3 — Runs

The screen that replaces three, and the weakest one in the React tree. Measured
instead of claimed:

| | React | Vue |
|---|---|---|
| Row height | ~88 px (three-line cards) | **38 px**, all 200 identical |
| Visible runs | 7 | ~20 **with the detail open** |
| Columns | none | State, Job, Run, Runner, Fired, Duration |
| Empty detail panel | occupies ~60% | is not rendered at all |
| Status filter | native `<select>` | `USelectMenu` |

**The detail does not replace the list, it stands beside it.** `/executions/:id`
renders the same component — the link stays shareable, the list keeps its scroll
and filters. Verified: after clicking a row the URL reads
`/executions/<id>?state=completed`, the panel is there and so is the table.

**The reactivity trap.** `useExecutions` takes a *getter*, not a value.
That is the risk named most often in the migration plan: with a plain object
the query key and the request freeze on the first render, the page re-renders
and silently shows the old rows. `toValue` in `queryKey` *and*
`queryFn` is what makes the query run again.

**One new impulse redeemed:** keyboard navigation (`j`/`k`, Enter, Escape —
and it steals no keys from input fields). It needed a list to be
useful.

> **Postscript (pass 5).** A second impulse stood here: a
> density switch under `croniq_density`. It is out again — the
> reasoning is below.


**One finding from looking:** the Run column wrapped onto two lines with the
attempt marker `#2` — exactly the class of defect `cq-num` was built against,
except that I had forgotten the utility on that cell. Now all 200
rows are exactly 38 px.

### Still open on this screen

- ~~**Time-window filter.**~~ ✓ Pass 14 — and the server *could not* quite
  do it after all: `ExecutionFilter` carried `since`/`until` from the start, the
  HTTP handler just never read the parameters.
- ~~**Loading more.**~~ ✓ Pass 14.
- **Links** from job and runner into their respective screens, once they
  exist.

---

## Pass 4 — dashboard, runners, dead letters

**The preview rail is there, and it cost no server work.**
`/v1/dashboard/forecast` has always existed — the React dashboard just never
calls it, only the jobs page does. The sharpest audit finding ("shows the past
everywhere, the future nowhere") was therefore a missing
query, not missing data.

The rail uses two sources, deliberately: `jobs/states` delivers the exact
next fire time per job — what an operator reads — and the forecast
gives the shape of the next hour in buckets, that is, "and then it gets busy". A
list alone hides the load, a histogram alone hides the names. Overdue
jobs stand above it and not in the middle of it: they are not "coming up", they are
late.

**The dashboard** as decided in the inventory — a status board plus *only*
failures, no general run list. That would have been the fourth rendering
of the same table. The failure heatmap has moved up from the bottom right corner
and got weekday and hour labels; it
answers "is something broken" better than any number next to it.

**Runners** loses its master/detail. With one runner, ~85% of the area in the
React tree was an empty panel. Everything the detail showed is either
a field that fits in the row or a run list — and runs now live
in one place, so the row links there. The SSE core from #585 gets
its first consumer here, with `shallowRef` for the rows: every frame
replaces the whole array.

**Dead Letters** keeps its own screen, as decided. New compared with the
React version: a rejected replay shows the server's reasoning.
The stale-replay guard rejects when the logical fire time is older than
the policy allows — that is a decision, not an error, and "Replay
failed" conceals it.

### Found by looking

The throughput chart was empty even though "186 runs" stood next to it: percentage heights
in an intermediate `div` without a defined height resolve to zero. Visible only
by looking — no test would have reported it.

## Pass 5 — density and scroll behaviour

Two corrections to things already built, both out of looking at them.

### The density switch is out again

It moved in during pass 3 as a "new impulse", and that was the wrong
classification. A density control hands the reader back a problem that the
design should have solved — and it doubles the work permanently: every
future table has to look right at two densities, otherwise one of them is
the worse one. `comfortable` wins because these rows carry a status pill and
monospaced ids; both need the leading.

The row height now lives as `@utility cq-row` in `main.css` instead of as a
constant in three views. It was previously the same comment three times over — a
reliable sign that the decision sat one level too low.

The key `croniq_density` is no longer read and no longer
written; a leftover in the `localStorage` of a developer's browser has no consequence.

### The list scrolls, not the page

Filters, column headers and the maintenance banner travelled up along with the scroll.
The cause is the same class as the empty throughput chart from
pass 4, just the other way round: the shell was `min-h-screen`. A
*minimum* height is not a definite height, so `h-full` in every list page
resolved to `auto`, its `overflow-auto` container grew with the content instead of
scrolling — and what then scrolled was the document.

The chain that makes it hold:

| Level | before | now |
| --- | --- | --- |
| Shell root | `min-h-screen` | `h-screen overflow-hidden` |
| `<main>` | `overflow-y-auto`, block | `flex flex-col min-h-0 overflow-hidden` |
| Maintenance banner | scrolls along | `shrink-0`, stays put |
| Scroll area | — | a `min-h-0 flex-1 overflow-y-auto` around `<RouterView>` |

That one area serves both page shapes, and that is the reason to have it in
the shell instead of in every page: a document-shaped page (the
dashboard) is taller than the box and scrolls inside it; a list-shaped page
sets `h-full`, is thereby exactly the box, nothing overflows, and the only thing
that moves is the table body in its own frame. Sidebar,
header and the filter bar above the list stay put in both cases.

## Pass 6 — Jobs

The thickest screen, and the one on which the stocktake was clearest:
six tabs, two of them the same executions table with a different
row limit.

### Six tabs down to two

| Tab | where to |
| --- | --- |
| Overview | stays — and takes in *Schedule* |
| Executions | dropped; the header links to `/executions?job_key=…` |
| Schedule | into the overview; it is four fields |
| DSL | stays |
| Alerts | `/alerts` |
| Audit | audit log in the settings |

Pulling *Schedule* into the overview was the decision with the largest
effect: a job's rule is the first thing someone wants to know who opens a
job. Keeping it one click away turned the overview into a
field list that was missing exactly the important thing.

### The list now answers the question you opened it for

The React list was one column of names. "When does this run next" and "is
something late" were answered by opening jobs one at a time. Three
endpoints had the answer and were never joined here:
`/v1/jobs` (definition), `/v1/jobs/states` (next/last run, `overdue`,
lifecycle) and `/v1/schedules` (the rules). The new list joins them and
sorts overdue ones to the top.

Also new compared with the React version is that *source* is a column:
Croniqfile-managed or API-managed. Previously that was a property you
only noticed when a button was greyed out.

### Three findings from looking

**The tag chips do not hold up.** Carried over from the runner list, where a fleet
has a handful of tags. Jobs are tagged by team, environment *and* kind — even
the demo has seven, and the row pushed the counter and the primary action out of the
toolbar. Now a menu whose width does not depend on the number of tags.

**"just now" is the wrong tense in a future column.**
`formatRelative` says that within its five-second threshold, and in the
*Next fire* column it reads as the past. It now says "due now" there.

**Columns that do not fit next to the open detail are omitted rather than
cut off.** With the detail open the table was hard-clipped on the right
— the container does scroll horizontally, but with no visible cue. *Last
fire*, *Fires* and *Source* now disappear while a job is open.

And a correction to my own density decision from pass 5: the
job row is the only two-line row in the product (key above description) and
gets, at 49 px, the height two lines need. `cq-row` is a minimum for
table rows anyway, not a fixed value — for exactly this.

### Checked

`ui/scripts/write-paths.mjs` drives twelve write paths against the dev stack:
create, attach a schedule, disable/enable, trigger, pause/
resume, edit, render DSL, adopt, delete. All twelve green, and
the rejection of the adoption arrives in the server's own wording:

> DSL adoption is disabled — set `policy { dsl_adopt_on_mutate true }` in the
> Croniqfile to enable

Plus ten unit tests for `renderDsl`.

### Still open on this screen

- **The Vue tree has no e2e suite.** The acceptance criterion from ADR-0004 is
  the Playwright suite, but that runs against the *built* React dashboard on 4233.
  Until the Vue tree has a project of its own there,
  `vue-write-paths.mjs` is a stopgap and says so in its header.
- **Two `401`s on a cold start.** Not from this work, but measured here:
  `runRefresh` retries once, because another tab might have rotated the refresh
  token. On the very first visit there is no cookie at all, and
  the retry is guaranteed to be futile. Neither server nor client
  distinguishes "no cookie presented" from "cookie rejected" —
  the comment in the handler even names that explicitly as intent. A separate
  matter.
- **The calendar picker in the schedule editor** lists names, but there is still
  no calendar screen on which one could be created (pass 7).

## Pass 7 — Calendars

Step 6 of the build order, started with the calendar screen, because the
schedule editor from pass 6 already offers calendar names and until now
there was no place to create one.

### What the screen now answers

The React version showed the name, the zone and the rule text. With that, the one
question you open a calendar for — *does the gate do what I mean?* —
is answered nowhere in the product: you wrote rules and found out later, from
a job that ran or did not run.

There is no endpoint that evaluates a calendar over a time range, and
nothing in the client may guess the semantics — the DSL belongs to the Rust side, a
second implementation would be wrong eventually. But the server does *apply* the
gate and says so too: `/v1/schedules` names the calendar per schedule,
`/v1/jobs/states` delivers `next_fire_at` computed **through** the gate, and
`suppressed_by` names the gate when it is currently holding a job back.

The detail joins both: which jobs this calendar governs, when each one fires
next and which ones it is holding back at this moment. Empirical instead of
claimed — and without a line of server code.

In the list the new column is *Used by*. A calendar nobody
references is not broken, it just does nothing — and that state was
previously indistinguishable from a working one. Now it says
`unused`.

### The builder can now edit too

The React version fell back to the raw text field when editing, on the
grounds that parsing saved DSL back into the typed form was
"best-effort". But `parseCalendarRules` reports whether it worked. So: try
first, show the builder on a clean parse, otherwise the saved text.
Editing is the common case; sending it to the escape hatch by default turned
the builder into a create-only feature.

Verified as a round trip: `include weekly weekday / exclude annual 12-25`
saved, reopened, formatted back identically.

### Three bugs found while checking

**`structuredClone` blew up the builder.** The initial rules come back out of the
wasm via `serde_wasm_bindgen`, and those objects are not structured-cloneable
— the clone threw, and the render went down with it. Now a
field-by-field copy of the three fields the type has.

**`USelectMenu` is called "Show popup" by a screen reader.** Nuxt UI renders
a select as a button and sets that `aria-label` itself; it beats
the label of the `UFormField` above it. Every select on the page would have been
announced identically — the mechanism instead of the choice. Affected was the calendar picker in the
schedule editor.

**And a measurement error of my own, which matters more than the other two.** The first
version of the check script went over the DOM and fell back to `textContent`
when it could not compute a name. Result: "all clean" — while the
real tree announced "Show popup". A lenient checker is worse than
none, it certifies the defect. `ui/scripts/accessible-names.mjs` now reads
Chromium's own tree over CDP (`Accessibility.getFullAXTree`).

State afterwards: **0 unnamed controls** across all built Vue screens,
dialogs opened. For comparison, #595 counted 51 of 54 in the React dashboard.

### Still open

- The schedule editor is only reached by the name checker if an
  API-managed job exists; otherwise it says so instead of reporting "clean".
- **The DSL tab on the job emits text that does not parse** — done, see
  pass 8.

## Pass 8 — the DSL tab that did not parse

Faithfully ported over from the React tree, documented as "the only place in the product
that shows a job in the form it is written in" — and
wrong. What the tab emitted:

```
job "demo:report" {
  description = "Generates a summary report"
  tags        = ["env=demo","kind=report"]
}
```

The real grammar has no `=`, does not quote the job key and writes tags
as a bare list. Against croniq's own lexer:

```
× unexpected character '='
   ╭─[rendered.croniq:2:15]
 2 │   description = "Generates a summary report"
   ·               ┬
```

Line 2. Anyone who copied the text into a Croniqfile got a parse error.

### Nothing is assembled by hand any more

`formatJobBlock` and `formatCalendarBlock` come from `croniq-config`, compiled
to wasm — the same crate the server loads a Croniqfile with. And
the Rust side **parses its own output** before returning it, and
reformats it canonically. What comes back parses by construction.

That is exactly the argument the calendar builder was built with in pass 7.
This tab is what happens when you do not apply it.

### What the API does not know is stated, not guessed

The saved job carries `max_retries` — a number, not a strategy. The DSL
needs one (`retry exponential { … }`). So the block writes *no*
retry block and says next to it why:

> The job retries 3 times, but the API does not record which backoff strategy,
> so no retry block is written — it would have to invent one.

The same for a rule the DSL does not know at all. The API accepts
`*/7 * * * *`; the Croniqfile grammar knows intervals, daily, weekdays
and monthly — no raw cron. Such a job is not representable, and the tab
says so instead of showing something similar.

### Checked, not claimed

`ui/scripts/dsl-parses.mjs` reads what the running dashboard writes to the
screen and hands it to the same binary an operator would have:

```
checking 6 job block(s)
ok   demo:heartbeat … ok   demo:reload-probe
every rendered block parses
```

Plus ten unit tests around the formatter (which fields arrive, whether the
notes tell the truth) and two new steps in `vue-write-paths.mjs`.

### Two measurement errors along the way

**A script that checked nothing reported "passed".** `dsl-parses.mjs` found
no job rows and printed "every rendered block parses" anyway. An empty
check set is now an error, not a success.

**And `npx vue-tsc --noEmit` checks nothing at all in `ui/`.** `tsconfig.json`
has `"files": []` and only `references`; without `--build` TypeScript does not follow
them. A `const x: number = "str"` passes. That is why a missing
export in `croniq-dsl.ts` only showed up in the browser, after "typecheck green" had been
reported. The project script `npm run typecheck` calls `vue-tsc --build`
and finds it — CI was fine the whole time, the hand-run invocation was the
hole.

## Pass 9 — Alerts

The last open question from the inventory, answered while building. The
decision and its reasoning are in `ui-screen-inventory.md`; in short: **one
screen, three views** (rules, channels, deliveries), instead of splitting configuration
and log across two screens. The stocktake's objection
was *mixing*, and the opposite of that is structure, not distance — you
snooze a rule because of what the log shows.

### What becomes visible that was mute before

**A channel no rule uses.** It now shows as `unused` in the
channel list — the same class of finding as the unused calendar.

**A rule that names a channel that does not exist.** The compiler keeps
the reference verbatim and warns only at fire time. So the rule looks
configured and delivers nowhere — in the product until now a silent
failure, here flagged in the list and the detail.

**A rule that does not do what the file says.** Overrides (snooze, throttle,
disable) stand in the row, and in the header bar a warning counts how
many rules are currently overridden. The difference between "nothing is
broken" and "nothing is being reported" is the most important statement this screen makes.

**Every delivery links in both directions** — to the run that
triggered it and to the rule that sent it. The React version
named both and linked neither, even though "which run was that?" is the first
question after an alert.

### The note is mandatory, and that is a good thing

The server requires a reason on every override. The form respects
that instead of filling it with a placeholder: a rule that is silent for an
unexplained reason is worse than a loud one.

### The demo did not show the feature

`Croniqfile.demo` had no `alerts { }` block. The demo deliberately lets
runs fail (`RUNNER_FAIL_RATE`) and reported that nowhere — the
alerts screen would have stayed empty, and therefore uncheckable. Now there is a channel
(`shell "echo …"`, without consequence) and a rule on `demo:*`. With that the
demo shows alerting the way the jobs show the scheduler.

`alerts { }` is boot-only — a reload reports it as `pending_restart`, instead of
applying it (see `operations.md`). The dev stack had to restart for it.

Verified all the way into the log: `alerts.delivered rule=demo-failures
channel=demo-log job_key=demo:heartbeat`, and the row is in the screen.

### In passing: the version looked like a button

Reported. It was a `UBadge` at `h-8` — a filled box, the same height as the
icon buttons next to it, so in a row of controls the fourth
button, one that does nothing on a click. The version is a fact about the
server, not an action, and should be the least clickable element up here.
Now plain text; sha, build time and environment are in the `title`.

## Pass 10 — Settings

Four views on one screen, addressed via path segments instead of `?tab=` —
the React tree used a query parameter, the rest of this tree uses
path segments for exactly this shape (see alerts). One idiom.

### Two tables become one

*Users* and *Invitations* stood in the React tree as two tables
one below the other. They are two answers to **one** question — an invitation is
someone who has been granted access and has not arrived yet — and separated,
you had to assemble "who can sign in to this server" from two lists with
different columns in your head. Now one list with a status.

What had to be decided along the way: which invitations belong on a list that is
called "who has access"?

- **Accepted** → is now a user, would be counted twice.
- **Revoked** → a decision already taken and carried out. The
  row is noise that accumulates forever; it belongs in the audit log,
  not here.
- **Expired** → stays. And that is exactly where the difference lies: an
  expired invitation is not a settled matter but someone who is still
  waiting. "Why did she never sign in" is answered here, and
  the answer is to invite her again.

### One component for everything you only see once

API keys, personal access tokens, invitation links and TOTP recovery codes
share a property the surrounding interface keeps forgetting:
**this render is the only copy.** In the React tree each of them was its own
markup — and the recovery codes were the one that fell through: the
checkbox asked the operator to confirm that they had saved codes that were never
rendered.

Now `SecretOnce`, one component, and it deliberately cannot be clicked
away by accident: "Done" stays disabled until it has been copied or explicitly
confirmed. The write-path test asserts exactly that.

### Scopes get presets

The React form was twenty checkboxes and nothing else — that is where
credentials go wrong: nobody thinks through twenty booleans, so you tick
`admin`, or roughly the right ones, and notice later. Three presets
name what people actually want to express; the full list
stays below them. `Runner` is the one that pays off: setting the pull-protocol scopes
plus registration and heartbeat wrong by hand yields a runner
that connects and then silently takes no work.

### The audit log now says who, not which UUID

The "Who" column showed actor IDs. A column of UUIDs answers "who did what"
with "somebody did something". The user list is loaded on this screen
anyway, so the ID resolves to a name — with a short ID as the
fallback for an actor who is no longer a user, which is exactly the case in
which the log counts most. Rows also link to what they
changed.

### Two findings from checking

**A branch nothing could ever reach.** The token table had a
rendering for revoked tokens. `GET /v1/users/me/tokens` simply returns `[]` after
a revocation — no tombstone. The branch was dead code; the
filter stays (if a server ever changes that, "render as valid" would be the
worse bug), the second section is gone. The test now asserts the
disappearance.

**And a test bug that nearly passed as a product bug.** Two rows for
the same invitation looked as though revoked invitations were sticking
around. A look at the server's response showed three invitations: one
revoked (correctly filtered out) and **two open** ones, from two test runs that
had aborted before their cleanup. The product was fine, the test
left rubbish behind. It now cleans up beforehand.

### The admin derivation in a composable

`isAdmin` sat in the shell and would have come into being a second time in the
settings — with a subtle default: **`true` when there is no
user record.** That is not waving things through, but the case of an
API-key session: `GET /v1/users/me` only resolves for password, OIDC and
PAT sessions, and a key session's authority comes from its scopes,
which the server enforces on every request. Short enough to retype and subtle
enough to retype wrongly — so `useIsAdmin()`, once, with the
reasoning beside it.

## Pass 11 — the console, and the end of the build order

The last screen from `ui-screen-inventory.md`, and the second consumer of the
SSE core from #585. That is exactly why the core was extracted: the runner stream and the
console differ in almost everything you see, and in nothing that
easily goes wrong — frame assembly across chunk boundaries, the 401 refresh, the
reconnect backoff.

### What a tail conceals, it now says

The React version buffered silently while it was paused, and discarded the
oldest events silently when its buffer filled up. A console you look
away from said nothing afterwards about the gap. Both are now counted and
shown: the *Resume* button carries the number of waiting events, and above
the list it says how many fell out of the back.

### Two things that make reading easier

**A coloured border on the left** for `warn` and `error`, in addition to the
level column: you find an error by skimming the left edge,
not by reading every row.

**Following stops when you scroll up**, and a *Follow* button
appears. Reading something while the view yanks itself downwards every few hundred
milliseconds is the most annoying thing a live tail can do.

### A false alarm, verified rather than believed

The log carried `WARN … no job config for completion — job not in DSL or store`
for `smoke:calendar-user`, a job I had deleted minutes earlier.
That looked like a real bug: job deleted, trigger keeps firing.

Looked into instead of reported: `/v1/schedules` joined against `/v1/jobs` yields
**zero orphaned triggers**. The warnings came from executions that were queued before
the deletion and finished afterwards — correct
behaviour, and the warning is exactly the right one.

### The placeholder is retired

Up to this point, routes that were not built rendered a `NotBuiltYet` naming the
build step they belong to — so that the shell stayed walkable and
nobody mistook a gap for finished work. With the last gap it is gone. A
route that does not exist is from now on an error, not a note.

**With that the build order from the inventory has been worked through.** What remains
is under "Still open" — first of all that the Vue tree still has no
Playwright project of its own (#620) and that the acceptance criterion from ADR-0004
therefore still measures the React dashboard.

## Pass 12 — the login page, and what was missing behind it

Feedback: the new login page seems heavily stripped down compared with the old one —
is more work still outstanding there?

On looking rather than guessing, the difference fell into two very unequal
halves.

### What I took for decoration

The terminal mock, the grid, the rotating verb line, the green
health colours. I classified them as landing-page aesthetics and left them
out — **and that was wrong**, in two respects: the judgement
itself, and that I made it alone. Brought back in pass 15.

### What was a real gap

**There was no way back into the account.** "Forgot your password?" was missing —
`POST /v1/auth/password-reset/request` has always existed on the server, the
React version offered it, the new one did not. Anyone who forgets their password needed
an administrator and a shell.

And then, while checking that one point, **the bigger thing**:

### Two links that led nowhere

The server builds them itself:

```rust
// api/password_reset.rs
let confirm_url = format!("{base}/password-reset/confirm?token={raw_token}");
// api/invitations.rs
let accept_url = format!("{base}/invitations/accept?token={raw_token}");
```

**Neither of the two trees had a route for them.** A password request
worked, the mail went out, the link opened the not-found page. Anyone who
was invited could not get in at all — the settings screen dutifully shows the
administrator a link to pass on, and that link was a dead
end.

The flow was half built and looked complete from the side an operator tests:
you invite somebody, get a link, the confirmation appears.
That it leads nowhere is noticed only by the invitee.

Both pages now exist, public (whoever opens them has no
session yet). Demonstrated as a complete chain: invite → follow the link → create
the account → **sign in** → clean up.

### Password rules before the round trip

`croniq_auth::password` is the authority and rejects with 400. The limits
now additionally live in `lib/password.ts` — not as a second
implementation, but so a form can say "at least eight characters"
*before* it asks. The interesting limit is the upper one: bcrypt ignores
everything beyond 72 **bytes**, so a longer passphrase would be silently
truncated. Three tests, one of them with an emoji — four bytes per character is
exactly the case a character count gets wrong.

### And a catch-all sentence on the most common path

The first version handled `410` and `404` specially and let the rest run into "The
server refused that." But an **unknown** token answers `401` —
so the single most likely error fell into the catch-all. Now
every branch of both handlers has been read off the Rust code and named, and the
test requires the message to name a *remedy*, not merely a
rejection.

### In passing

"Sign in to `127.0.0.1:4232`" is back. Anyone with staging and production in
two tabs open otherwise cannot tell them apart from the card — and
typing production credentials into staging is a mistake the page can simply
prevent. The React version had that right.

## Pass 13 — the command palette, and a ghost job

The palette was on the capability list and was missing — so not a nice-to-have, but
a gap against the scope guard.

### Shortcuts that did not exist

The React palette printed a shortcut next to every entry: `G D`, `G J`, `G E`.
**Nothing implemented them.** `g` then `d` did nothing on any screen.
A hint that lies is worse than none — so the new version does not port it
but makes it true: `useGoToShortcuts` implements the chords,
and the palette prints them *because* they work.

Three things a two-key chord needs, each asserted separately by the test:
it steals no key from an input field, it expires after 1.2 s (a
forgotten `g` must not turn the next key into a navigation minutes later),
and the fact that it is armed is visible — otherwise it is invisible
state.

Plus a **visible trigger** in the header with the shortcut printed on it.
A palette reachable only through a key combination nobody has been told
about is, for most people, simply not there.

The search also covers calendars and alert rules, which now exist as
screens.

### And along the way: a job that no longer exists stood in "Next hour"

Noticed on the screenshot of the palette — in the dashboard behind it stood
`smoke:calendar-user` and `smoke:vue-jobs`, both deleted.

`GET /v1/jobs/states` **outlives the job**, and that is deliberate. The server
says so at startup:

> job_states rows exist for jobs this configuration does not define. They are
> kept (a job may be temporarily absent) and no longer produce metrics.

A job that briefly drops out of the Croniqfile and comes back should not lose its
history (#470). A state row is therefore **no evidence that
the job exists** — and that is exactly how the rail read it. The job list
never had the bug, because it builds from `/v1/jobs` and joins the state onto it; the
rail built from the state and joins nothing.

Now it filters to jobs that exist — and before the job list arrives it shows
nothing rather than briefly showing deleted ones.

### A bigger find that fell out of it — and a correction to myself

When recomputing, the number did not fit: "99 fires" for five jobs, calculated as
79. `/v1/dashboard/forecast` reads `state.triggers` — the **in-memory**
registry, not the store.

And that drifts: `DELETE /v1/jobs/{key}` clears the definition, the trigger rows and
`job_states` in the store, but does **not remove the job from the running
registry**. The scheduler keeps firing it, the watchdog cancels every
execution as "stranded", and that runs until the next reload or restart.
Evidenced in the log: `smoke:calendar-user` was enqueued at 15:19:30 — minutes
after the deletion.

**That refutes my acquittal from pass 11.** There I had seen exactly
this symptom, joined `/v1/schedules` against `/v1/jobs`, measured "zero
orphaned triggers" and filed it as a false alarm. I had checked the
wrong table: store triggers and jobs are consistent — the registry
that the scheduler and the forecast actually use is not. A separate matter.

## Pass 14 — time windows and loading more

The last two points from the Runs screen, and the first was not what
it said there.

### "The server can do it, the interface cannot" was only half true

`ExecutionFilter` has always carried `since` and `until`, and the SQL applies
both. The **HTTP handler never read the query parameters** — so the only
access to the run history could not express a time window at all. The same
class as the unused forecast: the capability was there and nobody called it.

An unreadable value is ignored rather than rejected, as this endpoint has always
done with `state` and `limit` — a 400 would be a new failure mode for
callers who today get a sensible list.

### `until` is also the cursor

The list is sorted by `created_at` descending, and `until` bounds
the same field inclusively. That makes keyset paging possible without a store change:
request again with `until` = the `created_at` of the oldest row.

**Precision is the crux here**, and the test pins it down.
`created_at` sits in SQLite as an RFC3339 *string* and is compared
lexicographically. A cursor truncated to milliseconds — exactly what
`Date.toISOString()` delivers — sorts **below** a row within
the same millisecond (`+` is 0x2B, `4` is 0x34) and lets it drop out.

That is the losing direction: truncated, rows would be **silently skipped**,
not repeated. My first assumption was the opposite; the test refuted it.
The rule now stands in the handler, in the type and in the OpenAPI spec:
**return the `created_at` value unchanged, the one the server delivered.
Never reconstruct it.**

The inclusive bound returns the cursor row once more; the client discards
it by `id`. Evidenced on the running stack: 200 rows, after *Load older*
**399** — twice 200 minus the one overlap.

### The window is a length, not two points in time

"The last hour" is the question people have; two date fields make them
do arithmetic for it. The URL carries `window=1h`, not two instants — a
shared link then means "the last hour" *whenever it is opened*
instead of freezing a window around the moment of copying. That is almost always
what the sender meant.

### An observation that looked like a bug and was not one

The *Fired* column is not strictly monotonic: the list sorts by
`created_at`, what is displayed is `fire_at`. Measured instead of rebuilt: over 200
rows the two differ **199 times** — but only by about a
second, and **2 of 200** visibly fall out of order. An artefact in the
seconds range that the relative time rounds away. No reason to rebuild.

## Pass 15 — the stage restored

Feedback on pass 12: "a clear regression", and four
things by name — the text animation, the green health colours, the console preview,
the grid background.

I had checked exactly those in pass 12 and filed them as decoration. That
was a design decision about somebody else's product, taken without
asking. All four are back.

### Four parts

**The grid background** is a raster over two gradients, radially
masked so that it fades out towards the edge instead of ending at an edge.

**The third word rotates** through five verbs (`Recover`, `Replay`,
`Diagnose`, `Audit`, `Scale`), 3.5 s per word. Croniq is not one verb, and
naming five in succession says more than fixing one.

**The console** types a `croniq` command, streams the output, holds, clears.
Every command shown is a real subcommand of the CLI; the
output lines are illustrative but shaped by the responsibility of the respective
command — the rule that keeps a demo from becoming a lie.
It pauses on hover, and only during the hold phase (the typing and
output beats are too short for a hover to be useful there).

**The tiles carry tone.** The subline is green when the thing is
healthy, amber when it is not, and **grey as long as `/health` has not answered
yet** — colouring an unknown state green would be a claim nobody has
made yet.

### Three things that only showed up on looking

**The dark stage alone was not enough.** I had only coloured the
background — but every token above it (`text-highlighted`, `bg-default`, the card)
resolves from the class on `<html>`. Result: dark text on a dark
ground and three white tiles. The page now sets `dark` for its lifetime,
as the shipping version always did.

**`mode="out-in"` was the obvious transition and the wrong one.** The old
word leaves before the new one arrives — the line stands visibly
empty every few seconds. Now both sit in the same grid cell and cross-fade. Measured
over four rotations, every 100 ms: **0 of 160 frames without a word.**

**And the most serious one: the lightness choice was lost.** I had saved the
DOM state on mount and written it back on leaving.
But on a cold start of `/login`, `onMounted` runs **before** the store's theme
watcher — so what was saved was an attribute nobody had set yet,
and anyone who had chosen "light" got *nothing* back after signing in. The
store owns the theme, so the store restores it: `reapplyTheme()`.

A screenshot would have shown none of the three.

### Reduced motion

Everything that moves is off under `prefers-reduced-motion` — not faster,
but off: no timer runs, the console shows a **finished** demo instead of
an empty frame, the verb stands still. Checked.

## Pass 16 — three postscripts

Three pieces of feedback on pass 15, each with a cause of its own.

### The table header painted over the dialog

In the "New job" dialog, `STATUS` and `JOB` stood across the form, and the
field *Job key* had disappeared behind them.

Cause: my `sticky top-0 z-10` on the `<thead>`. A z-index only means something
relative to a stacking context — and without one around the table the
header competed with the **whole page**. Nuxt UI positions the dialog with
`z-index: auto`, so any positive number beats it.

The fix is not to give the dialog a higher number, but to give the header
its frame of reference: `isolation: isolate` on the scroll container. With that,
its z-index only applies within its own table, which is all it ever needed.

That now lives as `@utility cq-list` in `main.css` and is used by all eight
lists — as a utility and not as a class chain repeated eight times,
so that the property that matters has one place and one reason.

Checked with a raster over the dialog area: **whatever paints there must belong to the
dialog.** That catches the whole class of defect, not just this case.

### The background was painted and still invisible

Measured: the grid, the gradients and the mask were present in every viewport. You
just could not see them — and that is a fair description, not a
misperception.

What was missing were the **drifting spotlights**: two large, softly blurred
colour fields in the `screen` blend mode, with 90 s and 130 s periods so they never fall
into step. The raster only reads where a light passes behind it.
Without them the page lies flat.

Under `prefers-reduced-motion` they **stand still instead of disappearing** — they
are what makes the background legible at all, and taking them away would mean taking the
background away.

### The console had no counter

The old version showed a thin bar running down in the idle state. Without
it the hold phase is dead air: the output stands, nothing happens for five seconds,
and nothing indicates that more is coming.

It pauses with the demo — anyone who hovers to read visibly stops the
clock instead of silently shifting it. Measured: 503 px → 349 px while
running, then held at 343 px.

## Pass 17 — bulk deletion, the last gap in the scope guard

Going through the capability list, exactly one line was left without a place:
*"Dead Letters: … single and **bulk deletion**"*. The server can do it
(`POST /v1/dead-letters/bulk-delete`: either an `ids` list or
`all: true`, optionally narrowed to a `job_key`), the React version
could do it, mine could not.

Two paths, because they are different intentions: tick rows and
"Discard selected", or "Discard all". The destructive button only appears
once something is selected — a delete button that permanently stands next to a
work list is one you eventually stop reading.

The selection hangs on `id`, not on the index, and rows that disappear
(replayed elsewhere, cleared by retention) drop out of it —
otherwise a later bulk action names IDs that no longer exist.

What is reported is the **number** the server returns. A bulk deletion that
says "done" looks exactly like one that hit nothing.

The operator hint from the same line was already there, incidentally: the server
bakes it into `dead_reason` server-side (`"{reason} — {hint}"`), so the detail
shows it along with the rest.

### The same lesson three times, in one afternoon

The check step fell over three times, every time on **my assumption about live
data**, never on the product:

1. It first deleted the whole queue — and thereby took the *next* run's
   basis away. Exactly the order dependency I had criticised two passes
   earlier with the invitations.
2. The console check required an empty list after clearing. With a
   server that is talking, new rows arrive at the same moment — the
   assertion was about the load, not about the behaviour.
3. And the bulk deletion required the table to shrink by exactly two.
   At a 100% failure rate, two new ones came in while the deletion was running.

All three now assert what the feature promises (the server response, that the
selection is empty, that the deleted items are gone) instead of a number that depends on the
failure rate of the demo. And where the basis is missing, **the step says so
out loud** instead of ticking green: a skipped test over an empty
table would be the worse outcome.
