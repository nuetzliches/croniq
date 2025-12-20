# Detail-Surfaces (Dialog / Slide‑In) – Konzept

## Ziel

Detaildatensätze und Upsert-Flows (z.B. Schedules, Webhooks, API Keys) sollen nicht im Tabellen-Tab „ausbrechen“, sondern in einer klaren, fokussierten Oberfläche bearbeitet werden: entweder als Modal-Dialog oder als Slide‑In (Drawer). Das Konzept soll unsere Leitplanken (Zoneless + Signals + A11y + Tailwind Tokens) einhalten und sich an existierenden Patterns orientieren (vgl. Command Palette).

## Leitplanken (Repo-konform)

- Standalone Components, `ChangeDetectionStrategy.OnPush`.
- Signals für UI-State (`signal`, `computed`, ggf. `linkedSignal`); `effect()` nur für echte Side-Effects (z.B. Fokus-Management).
- Built-in Control Flow (`@if`, `@for`).
- A11y/WCAG 2.1 AA: Fokusführung, Escape, semantische Rollen, klare Labels.
- Keine neuen Hardcoded-Design-Tokens: Nutzung bestehender Tailwind Klassen/Variablen.
- Keine Lifecycle-Hooks (`ngOnInit`, …).

## Empfehlung (Default)

**Slide‑In Drawer als „Modal Dialog“ (a11y-technisch)**

- Für Detaildatensätze/Upsert ist ein Drawer meist besser als ein Center-Dialog: mehr Platz, weniger Kontextverlust, besonders bei Forms.
- Wichtig: Auch Drawer ist _modal_ zu behandeln: `role="dialog"`, `aria-modal="true"`, Fokus-Trap, Escape-to-close, Background nicht interaktiv.

**Wann Center-Dialog?**

- Sehr kurze, atomare Aktionen (z.B. „Rotate secret“, „Delete confirmation“), oder wenn nur 1–2 Inputs.

## UX-Entscheidungen (Minimal)

- Öffnen über „Open“/„Edit“ in der Liste → Drawer/Dialog öffnet.
- Laden: Skeleton/„Loading…“ im Surface (kein globales Blockieren).
- Aktionen in Footer/Toolbar:
  - Primary: „Save“ (Upsert)
  - Secondary: „Delete“ (falls vorhanden)
  - Optional: „Reload“ (Detail neu laden)
  - „Close“ immer verfügbar
- Keine zusätzlichen Screens/Routes.

## A11y-Anforderungen

1. **Semantik**

   - Overlay-Root: `role="dialog"` + `aria-modal="true"`.
   - `aria-labelledby` auf Titel.
   - Inhaltcontainer: `role="document"`.

2. **Fokus-Management**

   - Beim Öffnen: Fokus auf erstes sinnvolles Feld (oder Titel, wenn read-only).
   - Fokus bleibt im Dialog (Fokus-Trap).
   - Beim Schließen: Fokus zurück zum Auslöser (Button/Row Action).

3. **Tastatur**

   - `Escape` schließt Surface.
   - Tab/Shift+Tab rotiert innerhalb.

4. **Backdrop**
   - Klick auf Backdrop schließt (wie Command Palette). Optional im Surface (konfigurierbar), aber Default: schließen.

## Technische Umsetzung (2 Optionen)

### Option A (empfohlen): Reuse des existierenden „Command Palette“-Patterns

Pros:

- Passt bereits in Codebase: conditional render, overlay/backdrop, Escape, Fokus via Microtask.
- Kein zusätzliches CDK/Overlay-Framework notwendig.

Kernelemente:

- Conditional render `@if (isOpen()) { … }`
- Overlay `fixed inset-0 z-50 …`
- Backdrop `aria-hidden="true"` + click handler
- Keydown handler (Escape)
- Fokus-Sideeffect (kurzer `effect()`), analog zu `CommandPalette`

### Option B: Angular CDK A11y-Primitives (Fokus-Trap)

Pros:

- Saubere Fokus-Trap ohne hand-rolled Keyboard-Edgecases.

Bausteine:

- `@angular/cdk/a11y`: `CdkTrapFocus`, `cdkTrapFocusAutoCapture`.
- Weiterhin eigener DOM (ohne CDK Overlay), aber Fokus-Trapping robust.

> Hinweis: Wir vermeiden hier bewusst „große“ Overlay-Frameworks, solange unser DOM-basiertes Pattern reicht.

## Komponenten-Schnittstelle (konzeptionell)

Wir wollen **kleine, wiederverwendbare Primitives**, keine „page-sized“ Shared Components.

### `cq-detail-surface` (Primitive)

- Inputs:
  - `open: Signal<boolean>` (oder boolean + event)
  - `title: string`
  - `mode: 'drawer' | 'dialog'` (default `'drawer'`)
  - `closeOnBackdrop: boolean` (default true)
- Outputs:
  - `closed = output<void>()`
- Slots:
  - Header actions (optional)
  - Body
  - Footer actions

### Fokus-Restore (Primitive)

- Beim Öffnen den aktiven Trigger merken: `HTMLElement | null`.
- Beim Schließen: `queueMicrotask(() => trigger?.focus())`.

## Daten-/State-Flow (Upsert)

- List row action → `selectedId.set(id)` → `surfaceOpen.set(true)` → store lädt Detail.
- Detail in Store (Signal): sobald geladen, Form prefill (einmalig) aus Detail.
- Save:
  - Payload wird in TS gebaut (keine komplexe Logik im Template).
  - **TriggerId-Sicherheit**: bei Edit immer `triggerId` mitschicken (falls bekannt), um Upsert-Falle zu vermeiden (Cron-Change ohne triggerId → neuer Trigger).
- Delete:
  - Nach Erfolg: Surface schließen + List refresh.

## Fehlerbehandlung

- Fehler immer im Surface anzeigen (inline), nicht als globaler Toast.
- Auth/403: klare Nachricht (die Stores haben dafür bereits Patterns).

## Offene Punkte (für spätere Iteration)

- „Dirty form“ Guard (Bestätigung vor Close).
- Optional: Non-modal Drawer (für read-only Details) – aktuell nicht empfohlen wegen Fokus/Interaktionen.

## Referenz im Repo

- Vorbild für Overlay/Keyboard/Fokus: `src/app/shared/command-palette/*`.
