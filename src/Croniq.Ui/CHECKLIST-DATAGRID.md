# Croniq UI DataGrid Concept

> Goal: Extract shared, signal-first DataGrid component for Croniq UI features. Initial capabilities: Virtual Scroll and Column Templates. Align with Angular v21 patterns and repository guardrails.

## Scope & Goals

- Provide a standalone DataGrid primitive (Angular v21, signals-first, `ChangeDetectionStrategy.OnPush`) for reusable tables across feature modules.
- Optimize large datasets with virtual scrolling while keeping row rendering predictable and smooth.
- Support column templates (header/cell) to allow feature modules to supply bespoke rendering with strong typing.
- Keep dependencies minimal: prefer `@angular/cdk/scrolling` for virtual scroll; avoid heavy grid libs.

## Non-Goals (for now)

- Server-side sorting/filtering/paging orchestration (can be layered later).
- Drag-and-drop column reordering/resizing.
- Tree/grouped rows.
- Inline editing.

## Component Placement & Packaging

- Implement in `projects/ui-kit` as `DataGridComponent` and export via the UI Kit module barrel for reuse.
- Keep headless logic inside the component; expose lightweight presentational hooks so feature modules can style via Tailwind utilities.

## API Sketch (signal-based)

- `rows = input<readonly T[]>([])`
- `rowKey = input<(row: T, index: number) => string | number>` (required; drives trackBy and selection bookkeeping)
- `columns = input<readonly ColumnDef<T>[]>([])`
- `estimatedRowHeightPx = input<number>(48)` (used by virtual scroll)
- `bufferPx = input<number>(256)` (CDK viewport buffer)
- `emptyStateTemplate = input<TemplateRef<EmptyStateContext> | null>(null)`
- `loading = input<boolean>(false)` (to render skeleton/overlay)
- Outputs kept minimal initially (selection/sort to be added later if needed by features).

### Column Definition (typed template handles)

```ts
export type ColumnAlign = 'start' | 'center' | 'end';

export interface ColumnDef<T> {
  id: string; // stable identifier
  header?: TemplateRef<ColumnHeaderContext<T>>;
  cell: TemplateRef<ColumnCellContext<T>>; // required
  width?: string; // e.g., '200px', '20%'; optional
  align?: ColumnAlign;
  ariaLabel?: string; // optional override for accessibility
}

export interface ColumnCellContext<T> {
  $implicit: T;
  rowIndex: number;
  column: ColumnDef<T>;
}

export interface ColumnHeaderContext<T> {
  column: ColumnDef<T>;
  columnIndex: number;
}
```

### Template Usage (consumer side)

```html
<ng-template #nameHeader let-column="column">
  <span class="text-xs font-semibold text-cq-ink-700">{{ column.id }}</span>
</ng-template>

<ng-template #nameCell let-row let-column="column">
  <div class="truncate" [title]="row.name">{{ row.name }}</div>
</ng-template>

<cq-data-grid
  [rows]="rows()"
  [rowKey]="rowKeyFn"
  [columns]="[
    { id: 'name', header: nameHeader, cell: nameCell, width: '30%' },
    { id: 'status', header: statusHeader, cell: statusCell, align: 'center', width: '12rem' }
  ]"
  [virtualScroll]="true"
  [estimatedRowHeightPx]="48"
></cq-data-grid>
```

### Template Typing (TS + HTML)

- Keep `strictTemplates/strictInputAccessModifiers` enabled in `tsconfig.app.json` (already true repo-wide) so the compiler enforces template types.
- Consumers should type their rows and column defs once and let inference flow into templates:

```ts
type Row = { id: string; name: string; status: 'active' | 'paused' };

readonly rowKeyFn = (row: Row) => row.id;
readonly columns: readonly ColumnDef<Row>[] = [
  { id: 'name', header: nameHeader, cell: nameCell },
  { id: 'status', header: statusHeader, cell: statusCell },
];
```

```html
<ng-template #nameCell let-row let-column="column">
  <!-- `row` is a Row, `column` is ColumnDef<Row> -->
  <div class="truncate" [title]="row.name">{{ row.name }}</div>
</ng-template>

<cq-data-grid [rows]="rows()" [rowKey]="rowKeyFn" [columns]="columns"></cq-data-grid>
```

- The `TemplateRef<ColumnCellContext<T>>` signature on `ColumnDef` ensures `let-row`/`let-column` are strongly typed. Avoid `any` casts; if inference fails, use `satisfies readonly ColumnDef<Row>[]` to enforce correctness without widening:

```ts
const columns = [
  { id: 'name', header: nameHeader, cell: nameCell },
] satisfies readonly ColumnDef<Row>[];
```

## Rendering & Reactive Design

- Use `CdkVirtualScrollViewport` when `virtualScroll()` is true; fall back to simple `@for` loop when false.
- Always render via `CdkVirtualScrollViewport`; small datasets still run through virtual scroll (acceptable trade-off for unified path).
- Derive rendered rows via `computed(() => rows())` (future hook for paging) and feed `trackBy` from `rowKey`.
- Avoid `effect()` except for imperative scroll adjustments (e.g., resetting scroll when columns change); document any effect usage.
- Do not import `CommonModule`; import only required directives/pipes (e.g., `NgTemplateOutlet`, `NgClass` alternatives) directly per repo rules.
- Use built-in control flow (`@if`, `@for`) in templates.

## Virtual Scroll Notes

- Default item size equals `estimatedRowHeightPx()`; allow consumers to pass the actual average height to reduce viewport jitter.
- Set `minBufferPx`/`maxBufferPx` via `bufferPx()` to smooth fast scroll.
- Provide optional `aria-busy` overlay when `loading()` to prevent focus jumps.
- Expose a method `scrollToIndex(index: number)` for feature modules (e.g., jump to selection).

## Accessibility & UX

- Table semantics: render rows as list items within a `<div role="table">`, `<div role="row">`, `<div role="cell">`; ensure headers use `<div role="columnheader">`.
- Keyboard: arrow keys should move focus between rows; `Home/End/PageUp/PageDown` should map to virtual scroll viewport helpers.
- Focus management: maintain roving tabindex within the viewport; when rows recycle, ensure the focused row is restored via `rowKey` mapping.
- Announce loading/empty states via `aria-live="polite"`.

## Styling & Layout

- Keep layout CSS minimal and utility-friendly: grid/flex wrappers with Tailwind classes; expose CSS vars for row height and gutter if needed.
- Provide density knobs via CSS classes (`is-dense`, `is-comfortable`) that map to row padding and font size.
- Avoid hardcoded colors; use Croniq tokens (`--cq-*`) via Tailwind theme.

## Data & Performance

- Require stable `rowKey` to avoid DOM churn; throw in dev mode if duplicates are detected.
- Avoid mutating row objects; assume immutable input arrays. Document that consumers must provide new array references when data changes.
- Optionally allow `rowClasses = input<(row: T) => string | string[] | undefined>` for cheap conditional styling.

## Testing Strategy

- Unit tests (Karma/Jasmine) covering:
  - Virtual scroll renders expected window and honors `rowKey` trackBy.
  - Column templates receive correct context.
  - Empty/loading states and `aria` roles are applied.
  - `scrollToIndex` jumps correctly.
- Consider lightweight visual regression snapshots (Storybook/Chromatic TBD) once component stabilizes.

## Integration Steps

- Implement component in `projects/ui-kit/src/lib/data-grid/` with a small README and usage snippet.
- Add an example story/demo page (or harness) for local testing with >5k rows to validate scroll performance.
- Replace existing feature tables incrementally (e.g., schedules list) to validate API shape; adjust props based on real usage.
- Document public API in `docs/deep-dive/ui.md` once stable.

## Open Questions

- Should selection (single/multi) be first-class in v1 or layered as a directive?
- Do we need built-in column sorting indicators, or should features inject header templates that own sort state?
- Should sticky header/columns be included in v1 or deferred until a real use case arises?
