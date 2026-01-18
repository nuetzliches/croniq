import { CdkContextMenuTrigger } from '@angular/cdk/menu';
import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, ElementRef, Input, TemplateRef, ViewEncapsulation, computed, contentChild, contentChildren, inject, input, viewChild, viewChildren } from '@angular/core';

export type ColumnAlign = 'start' | 'center' | 'end';

// Helper type to resolve deep keys (e.g. "user.profile.name")
export type NestedKeyOf<T> = T extends object
  ? {
    [K in keyof T & (string | number)]: T[K] extends (Date | Array<unknown>)
    ? `${K}`
    : T[K] extends object
    ? `${K}` | `${K}.${NestedKeyOf<T[K]>}`
    : `${K}`;
  }[keyof T & (string | number)]
  : never;

export interface ColumnDef<T> {
  /** Stable identifier for trackBy; falls back to key/index when omitted. */
  id?: string;
  /** Header label (preferred) or custom header template. */
  header?: string | TemplateRef<ColumnHeaderContext<T>>;
  /** Optional cell template. If omitted, `key` drives rendering. */
  cell?: TemplateRef<ColumnCellContext<T>>;
  /** Optional key to pluck the value from the row when no `cell` is provided. */
  key?: NestedKeyOf<T>;
  width?: string;
  align?: ColumnAlign;
  ariaLabel?: string;
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

export interface RowContextMenuContext<T> {
  row: T;
  rowIndex: number;
}

export interface EmptyStateContext<T> {
  rows: readonly T[];
}

@Directive({
  selector: '[cqHeader]'
})
export class CqHeaderDefDirective {
  readonly template = inject(TemplateRef);
}

@Directive({
  selector: '[cqCell]'
})
export class CqCellDefDirective<T> {
  readonly template = inject(TemplateRef);

  @Input() cqCell: T[] | ReadonlyArray<T> | '' | undefined;

  static ngTemplateContextGuard<TContext>(
    dir: CqCellDefDirective<TContext>,
    ctx: unknown
  ): ctx is ColumnCellContext<TContext> {
    return true;
  }
}

@Component({
  selector: 'cq-column',
  template: '',
})
export class CqColumnComponent<T> {
  readonly id = input<string>();
  readonly header = input<string>();
  readonly key = input<NestedKeyOf<T>>();
  readonly width = input<string>();
  readonly align = input<ColumnAlign>();
  readonly ariaLabel = input<string>();

  readonly headerDef = contentChild(CqHeaderDefDirective);
  readonly cellDef = contentChild(CqCellDefDirective);
}

@Component({
  selector: 'cq-data-grid',
  imports: [
    CdkContextMenuTrigger,
    CdkVirtualScrollViewport,
    CdkFixedSizeVirtualScroll,
    CdkVirtualForOf,
    NgTemplateOutlet,
  ],
  template: `
    <div class="cq-data-grid__head" role="rowgroup">
      <div class="cq-data-grid__row cq-data-grid__row--header" role="row">
        @for (column of effectiveColumns(); track columnTrackId(column, $index); let columnIndex = $index) {
          <div
            class="cq-data-grid__cell cq-data-grid__cell--header"
            role="columnheader"
            [attr.aria-label]="column.ariaLabel ?? headerLabel(column, columnIndex)"
            [style.width]="column.width ?? null"
            [class.is-align-center]="column.align === 'center'"
            [class.is-align-end]="column.align === 'end'"
          >
            @if (isTemplateHeader(column.header)) {
              <ng-container
                [ngTemplateOutlet]="column.header"
                [ngTemplateOutletContext]="{ column, columnIndex }"
              />
            } @else {
              <span class="cq-data-grid__cell-text">{{ headerLabel(column, columnIndex) }}</span>
            }
          </div>
        }
      </div>
    </div>

    <ng-template #rowCells let-row let-rowIndex="rowIndex">
      @for (column of effectiveColumns(); track columnTrackId(column, $index); let columnIndex = $index) {
        <div
          class="cq-data-grid__cell"
          role="cell"
          [style.width]="column.width ?? null"
          [class.is-align-center]="column.align === 'center'"
          [class.is-align-end]="column.align === 'end'"
        >
          @if (column.cell) {
            <ng-container
              [ngTemplateOutlet]="column.cell"
              [ngTemplateOutletContext]="{ $implicit: row, rowIndex, column }"
            />
          } @else {
            <span class="cq-data-grid__cell-text">{{ formatCell(row, column, rowIndex) }}</span>
          }
        </div>
      }
    </ng-template>

    <cdk-virtual-scroll-viewport
      cdkFixedSizeVirtualScroll
      #viewport
      class="cq-data-grid__viewport"
      role="rowgroup"
      [itemSize]="estimatedRowHeightPx()"
      [minBufferPx]="bufferPx()"
      [maxBufferPx]="bufferPx()"
    >
      <ng-container
        *cdkVirtualFor="let row of validatedRows(); let rowIndex = index; trackBy: trackRow"
      >
        @if (rowContextMenu()) {
          <div
            #rowEl
            class="cq-data-grid__row"
            role="row"
            [attr.data-row-key]="rowKey()(row, rowIndex)"
            [attr.data-row-index]="rowIndex"
            [attr.tabindex]="rowIndex === 0 ? 0 : -1"
            [class]="rowClassList(row)"
            (keydown)="onRowKeydown($event, rowIndex)"
            [cdkContextMenuTriggerFor]="rowContextMenu()!"
            [cdkContextMenuTriggerData]="rowContextMenuData(row, rowIndex)"
          >
            <ng-container
              [ngTemplateOutlet]="rowCells"
              [ngTemplateOutletContext]="{ $implicit: row, rowIndex }"
            />
          </div>
        } @else {
          <div
            #rowEl
            class="cq-data-grid__row"
            role="row"
            [attr.data-row-key]="rowKey()(row, rowIndex)"
            [attr.data-row-index]="rowIndex"
            [attr.tabindex]="rowIndex === 0 ? 0 : -1"
            [class]="rowClassList(row)"
            (keydown)="onRowKeydown($event, rowIndex)"
          >
            <ng-container
              [ngTemplateOutlet]="rowCells"
              [ngTemplateOutletContext]="{ $implicit: row, rowIndex }"
            />
          </div>
        }
      </ng-container>
    </cdk-virtual-scroll-viewport>

    @if (!hasRows()) {
      <div class="cq-data-grid__empty" role="rowgroup">
        @if (emptyStateTemplate()) {
          <ng-container
            [ngTemplateOutlet]="emptyStateTemplate()!"
            [ngTemplateOutletContext]="{ rows: validatedRows() }"
          />
        } @else {
          <div class="cq-data-grid__cell-text">
            @if (loading()) {
              Loading rows...
            } @else {
              No rows found.
            }
          </div>
        }
      </div>
    }
  `,
  styles: `
    :host {
      --cq-data-grid-row-height: 48px;
      display: block;
      border: 1px solid var(--cq-border, #e5e7eb);
      border-radius: 0.75rem;
      background: var(--cq-surface, #ffffff);
      color: inherit;
      font-size: 0.875rem;
    }

    .cq-data-grid__head,
    .cq-data-grid__body,
    .cq-data-grid__viewport {
      display: block;
    }

    .cq-data-grid__viewport {
      height: 100%;
      max-height: 640px;
      overflow: auto;
    }

    .cq-data-grid__row {
      display: flex;
      align-items: center;
      min-height: var(--cq-data-grid-row-height);
      padding: 0 0.75rem;
      gap: 0.5rem;
      outline: none;
    }

    .cq-data-grid__row:focus-visible {
      box-shadow: inset 0 0 0 2px var(--cq-accent-400, #2563eb);
    }

    .cq-data-grid__row--header {
      background: var(--cq-surface-5, #f8fafc);
      border-bottom: 1px solid var(--cq-border, #e5e7eb);
    }

    .cq-data-grid__cell {
      flex: 1 1 0;
      display: flex;
      align-items: center;
      gap: 0.25rem;
      min-width: 0;
      padding: 0.5rem 0;
    }

    .cq-data-grid__cell--header {
      font-size: 0.75rem;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.02em;
      color: var(--cq-text-secondary);
    }

    .cq-data-grid__cell-text {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .cq-data-grid__empty {
      padding: 1rem;
      color: var(--cq-ink-500, #6b7280);
      text-align: center;
    }

    .is-align-center {
      justify-content: center;
      text-align: center;
    }

    .is-align-end {
      justify-content: flex-end;
      text-align: end;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  host: {
    role: 'table',
    class: 'cq-data-grid',
    '[attr.aria-busy]': 'loading() ? "true" : "false"',
  },
})
export class DataGrid<T> {
  readonly rows = input<readonly T[]>([]);
  readonly rowKey = input.required<(row: T, index: number) => string | number>();
  readonly columns = input<readonly ColumnDef<T>[]>([]);
  readonly estimatedRowHeightPx = input(48);
  readonly bufferPx = input(256);
  readonly emptyStateTemplate = input<TemplateRef<EmptyStateContext<T>> | null>(null);
  readonly loading = input(false);
  readonly rowClasses = input<(row: T) => string | readonly string[] | undefined>();
  readonly rowContextMenu = input<TemplateRef<RowContextMenuContext<T>> | null>(null);

  private readonly contentColumns = contentChildren(CqColumnComponent);

  // Compute effective columns from either explicit input OR content children
  readonly effectiveColumns = computed<readonly ColumnDef<T>[]>(() => {
    // Prefer explicit config if provided and non-empty
    if (this.columns().length > 0) {
      return this.columns();
    }

    // Map content children to ColumnDef structure
    return this.contentColumns().map(col => ({
      // We must cast because CqColumn is loosely typed (T unknown in template context)
      id: col.id(),
      key: col.key() as NestedKeyOf<T>,
      header: col.headerDef()?.template ?? col.header(),
      cell: col.cellDef()?.template as TemplateRef<ColumnCellContext<T>>,
      width: col.width(),
      align: col.align(),
      ariaLabel: col.ariaLabel()
    }));
  });

  private readonly viewportRef = viewChild(CdkVirtualScrollViewport);
  private readonly rowEls = viewChildren<ElementRef<HTMLElement>>('rowEl');

  readonly validatedRows = computed(() => {
    const data = this.rows();

    if (typeof ngDevMode !== 'undefined' && ngDevMode) {
      const seen = new Set<string | number>();
      const keyFn = this.rowKey();

      data.forEach((row, index) => {
        const key = keyFn(row, index);

        if (seen.has(key)) {
          throw new Error(`Duplicate row key detected: ${String(key)}`);
        }

        seen.add(key);
      });
    }

    return data;
  });

  readonly hasRows = computed(() => this.validatedRows().length > 0);

  trackRow = (index: number, row: T) => this.rowKey()(row, index);

  scrollToIndex(index: number) {
    if (index < 0 || index >= this.validatedRows().length) {
      return;
    }

    this.viewportRef()?.scrollToIndex(index, 'smooth');
    queueMicrotask(() => this.focusRow(index));
  }

  onRowKeydown(event: KeyboardEvent, rowIndex: number) {
    const total = this.validatedRows().length;

    if (!total) {
      return;
    }

    let nextIndex = rowIndex;

    switch (event.key) {
      case 'ArrowDown':
        nextIndex = Math.min(total - 1, rowIndex + 1);
        break;
      case 'ArrowUp':
        nextIndex = Math.max(0, rowIndex - 1);
        break;
      case 'Home':
        nextIndex = 0;
        break;
      case 'End':
        nextIndex = total - 1;
        break;
      case 'PageDown':
        nextIndex = Math.min(total - 1, rowIndex + this.pageStep());
        break;
      case 'PageUp':
        nextIndex = Math.max(0, rowIndex - this.pageStep());
        break;
      default:
        return;
    }

    if (nextIndex === rowIndex) {
      return;
    }

    event.preventDefault();
    this.scrollToIndex(nextIndex);
  }

  rowClassList(row: T): string {
    const value = this.rowClasses()?.(row);

    if (value == null) {
      return '';
    }

    if (typeof value === 'string') {
      return value;
    }

    if (Array.isArray(value)) {
      return value.join(' ');
    }

    return '';
  }

  rowContextMenuData(row: T, rowIndex: number): RowContextMenuContext<T> {
    return { row, rowIndex };
  }

  private focusRow(rowIndex: number) {
    const match = this.rowEls()
      .find((element) => element.nativeElement.dataset['rowIndex'] === String(rowIndex));

    match?.nativeElement.focus({ preventScroll: true });
  }

  private pageStep(): number {
    const viewport = this.viewportRef();

    if (viewport) {
      const visible = Math.floor(viewport.getViewportSize() / this.estimatedRowHeightPx());
      return Math.max(1, visible);
    }

    return 10;
  }

  columnTrackId = (column: ColumnDef<T>, index: number) => column.id ?? (column.key ? String(column.key) : String(index));

  headerLabel(column: ColumnDef<T>, columnIndex: number): string {
    if (typeof column.header === 'string') {
      return column.header;
    }

    return column.id ?? (column.key ? String(column.key) : `col-${columnIndex}`);
  }

  isTemplateHeader(header: ColumnDef<T>['header']): header is TemplateRef<ColumnHeaderContext<T>> {
    return !!header && typeof header !== 'string';
  }

  formatCell<TValue>(row: T, column: ColumnDef<T>, rowIndex: number): TValue | undefined | null {
    const key = column.key;
    let value = row as unknown as TValue;

    if (typeof key === 'string') {
      const parts = key.split('.');
      for (const part of parts) {
        if (value == null) {
          return;
        }
        value = (value as unknown as Record<string, unknown>)[part] as TValue;
      }
    }

    if (value == null) {
      return;
    }

    return value;
  }
}
