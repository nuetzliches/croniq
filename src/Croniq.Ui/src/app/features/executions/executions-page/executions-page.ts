import { CdkMenu } from '@angular/cdk/menu';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { RouterLink } from '@angular/router';
import { nowMs } from '@core/time/clock';
import { ExecutionResponse } from '@croniq/api-schema';
import { LogViewerDialogComponent } from '@features/executions/components/log-viewer-dialog/log-viewer-dialog.component';
import { ExecutionsStore } from '@features/executions/executions.store';
import { bindQueryParam } from '@shared/routing/selection-sync';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { CqCellDefDirective, CqColumnComponent, CqContextMenuItemDirective, CqDialogService, CqIconComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';

type ExecutionStatusFilter = 'all' | 'success' | 'failure' | 'canceled' | 'running' | 'pending' | 'unknown';
type ExecutionStatusKey = Exclude<ExecutionStatusFilter, 'all'>;
type ExecutionDateRange = '24h' | '7d' | '30d' | 'all';

type ExecutionRow = {
  executionId: string;
  jobKey: string;
  statusLabel: string;
  statusKey: ExecutionStatusKey;
  startedAtUtc?: string;
  startedAtMs: number;
  durationMs?: number;
  triggerId?: string;
  instanceId?: string;
  executionMode?: string;
  invocationSource?: string;
  errorType?: string;
};

const STATUS_OPTIONS: ReadonlyArray<{ value: ExecutionStatusFilter; label: string }> = [
  { value: 'all', label: 'All statuses' },
  { value: 'success', label: 'Success' },
  { value: 'failure', label: 'Failure' },
  { value: 'canceled', label: 'Canceled' },
  { value: 'running', label: 'Running' },
  { value: 'pending', label: 'Pending' },
  { value: 'unknown', label: 'Unknown' },
];

const DATE_RANGE_OPTIONS: ReadonlyArray<{ value: ExecutionDateRange; label: string }> = [
  { value: '24h', label: 'Last 24 hours' },
  { value: '7d', label: 'Last 7 days' },
  { value: '30d', label: 'Last 30 days' },
  { value: 'all', label: 'All time' },
];

@Directive({
  selector: '[cqExecutionCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqExecutionCellDirective }],
})
export class CqExecutionCellDirective extends CqCellDefDirective<ExecutionRow> {
  // Inherits ngTemplateContextGuard from base class.
}

@Component({
  selector: 'cq-executions-page',
  imports: [CdkMenu, DatePipe, RouterLink, DataGrid, CqColumnComponent, CqExecutionCellDirective, CqInputDirective, CqSelectDirective, CqContextMenuItemDirective, CqIconComponent],
  templateUrl: './executions-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [ExecutionsStore],
})
export class ExecutionsPage {
  private readonly store = inject(ExecutionsStore);
  private readonly dialog = inject(CqDialogService);

  private readonly shellPanel = inject(ShellPanelService);
  private readonly panelTemplate = viewChild<TemplateRef<unknown>>('executionsFilterPanel');
  private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('executionsFilterCollapsed');

  // Data
  readonly executions = this.store.executions;
  readonly loading = this.store.loading;
  readonly error = this.store.error;

  readonly executionSearch = signal('');
  readonly statusFilter = signal<ExecutionStatusFilter>('all');
  readonly dateRangeFilter = signal<ExecutionDateRange>('24h');
  readonly jobSearch = signal('');
  readonly selectedJobKeys = signal<ReadonlyArray<string>>([]);
  readonly selectedExecutionId = bindQueryParam({ paramKey: 'executionId' });
  readonly statusOptions = STATUS_OPTIONS;
  readonly dateRangeOptions = DATE_RANGE_OPTIONS;

  readonly executionRows = computed<ReadonlyArray<ExecutionRow>>(() =>
    this.executions().map((execution, index) => normalizeExecution(execution, index)),
  );

  readonly jobOptions = computed(() => {
    const entries = new Map<string, number>();
    this.executionRows().forEach((row) => {
      const key = row.jobKey.trim();
      if (!key || key.toLowerCase() === 'unknown job') {
        return;
      }
      entries.set(key, (entries.get(key) ?? 0) + 1);
    });
    return Array.from(entries.entries())
      .map(([jobKey, count]) => ({ jobKey, count }))
      .sort((a, b) => a.jobKey.localeCompare(b.jobKey));
  });

  readonly visibleJobOptions = computed(() => {
    const query = this.jobSearch().trim().toLowerCase();
    if (!query) {
      return this.jobOptions();
    }
    return this.jobOptions().filter((entry) => entry.jobKey.toLowerCase().includes(query));
  });

  readonly filteredExecutions = computed(() => {
    const query = this.executionSearch().trim().toLowerCase();
    const statusFilter = this.statusFilter();
    const dateRange = this.dateRangeFilter();
    const selectedJobs = new Set(this.selectedJobKeys());
    const cutoffMs = resolveDateRangeCutoff(dateRange);

    return this.executionRows().filter((row) => {
      if (statusFilter !== 'all' && row.statusKey !== statusFilter) {
        return false;
      }

      if (selectedJobs.size > 0 && !selectedJobs.has(row.jobKey)) {
        return false;
      }

      if (cutoffMs !== null) {
        if (row.startedAtMs <= 0 || row.startedAtMs < cutoffMs) {
          return false;
        }
      }

      if (!query) {
        return true;
      }

      return (
        row.executionId.toLowerCase().includes(query) ||
        row.jobKey.toLowerCase().includes(query) ||
        (row.triggerId ?? '').toLowerCase().includes(query) ||
        (row.instanceId ?? '').toLowerCase().includes(query) ||
        (row.invocationSource ?? '').toLowerCase().includes(query)
      );
    });
  });

  readonly selectedExecution = computed(() => {
    const raw = this.selectedExecutionId();
    if (raw === null || raw === undefined) {
      return null;
    }
    const id = typeof raw === 'string' ? raw : String(raw);
    return this.executionRows().find((row) => row.executionId === id) ?? null;
  });

  readonly filtersActive = computed(() =>
    !!this.executionSearch() ||
    this.statusFilter() !== 'all' ||
    this.dateRangeFilter() !== '24h' ||
    this.jobSearch().length > 0 ||
    this.selectedJobKeys().length > 0,
  );

  executionRowKey = (row: ExecutionRow, index: number) => row.executionId || `execution-${index}`;

  executionRowClasses = (row: ExecutionRow) =>
    row.statusKey === 'failure' ? ['opacity-90'] : undefined;

  constructor() {
    effect((onCleanup) => {
      const template = this.panelTemplate();
      const collapsedTemplate = this.collapsedTemplate();
      if (!template) {
        return;
      }
      this.shellPanel.setPanel(
        template,
        'Filters & settings',
        'Refine the executions list view.',
        collapsedTemplate ?? null,
      );
      onCleanup(() => this.shellPanel.clearPanel(template));
    });
  }

  refresh(): void {
    this.store.refresh();
  }

  setExecutionSearch(query: string): void {
    this.executionSearch.set(query);
  }

  setStatusFilter(value: string): void {
    this.statusFilter.set(normalizeStatusFilter(value));
  }

  setDateRangeFilter(value: string): void {
    this.dateRangeFilter.set(normalizeDateRange(value));
  }

  setJobSearch(query: string): void {
    this.jobSearch.set(query);
  }

  resetFilters(): void {
    this.executionSearch.set('');
    this.statusFilter.set('all');
    this.dateRangeFilter.set('24h');
    this.jobSearch.set('');
    this.selectedJobKeys.set([]);
  }

  isJobSelected(jobKey: string): boolean {
    return this.selectedJobKeys().includes(jobKey);
  }

  toggleJobSelection(jobKey: string, checked: boolean): void {
    this.selectedJobKeys.update((current) =>
      checked
        ? Array.from(new Set([...current, jobKey]))
        : current.filter((entry) => entry !== jobKey),
    );
  }

  formatExecutionMode(mode?: string): string {
    const normalized = (mode ?? '').trim().toLowerCase();
    return normalized || 'normal';
  }

  formatInvocationSource(source?: string, executionMode?: string): string {
    const normalized = (source ?? '').trim().toLowerCase() || 'schedule';
    const label = normalized
      .replace('webhook-ingress', 'webhook ingress')
      .replace('webhook-invoke', 'webhook invoke');
    const mode = this.formatExecutionMode(executionMode);
    return mode === 'test' ? `${label}:test` : label;
  }

  formatDuration(durationMs?: number): string {
    if (!durationMs || durationMs <= 0) {
      return '—';
    }
    if (durationMs < 1000) {
      return `${Math.round(durationMs)} ms`;
    }
    const seconds = durationMs / 1000;
    if (seconds < 60) {
      return `${seconds.toFixed(1)} s`;
    }
    const minutes = seconds / 60;
    if (minutes < 60) {
      return `${minutes.toFixed(1)} min`;
    }
    const hours = minutes / 60;
    return `${hours.toFixed(1)} h`;
  }

  viewLogs(id: string): void {
    this.dialog.open(LogViewerDialogComponent, {
      data: { executionId: id },
      width: '800px',
      panelClass: 'bg-transparent',
    });
  }
}

function normalizeExecution(execution: ExecutionResponse, index: number): ExecutionRow {
  const record = execution as Record<string, unknown>;
  const executionIdValue = typeof record['executionId'] === 'string'
    ? record['executionId']
    : typeof record['id'] === 'string'
      ? record['id']
      : `execution-${index}`;
  const executionId = executionIdValue.trim();

  const jobKey = typeof record['jobKey'] === 'string' ? record['jobKey'].trim() : 'Unknown job';
  const statusValue = typeof record['status'] === 'string' || typeof record['status'] === 'number'
    ? record['status']
    : undefined;
  const status = normalizeExecutionStatus(statusValue);

  const startedAtRaw = typeof record['startedAtUtc'] === 'string'
    ? record['startedAtUtc']
    : typeof record['startedAt'] === 'string'
      ? record['startedAt']
      : typeof record['startAtUtc'] === 'string'
        ? record['startAtUtc']
        : undefined;
  const startedAtUtc = startedAtRaw?.trim() || undefined;
  const startedAtMs = toEpochMs(startedAtUtc);

  const durationMs = typeof record['durationMs'] === 'number' ? record['durationMs'] : undefined;
  const triggerId = typeof record['triggerId'] === 'string'
    ? record['triggerId'].trim()
    : typeof record['trigger'] === 'string'
      ? record['trigger'].trim()
      : undefined;
  const instanceId = typeof record['instanceId'] === 'string' ? record['instanceId'].trim() : undefined;
  const executionMode = typeof record['executionMode'] === 'string' ? record['executionMode'].trim() : undefined;
  const invocationSource = typeof record['invocationSource'] === 'string' ? record['invocationSource'].trim() : undefined;
  const errorType = typeof record['errorType'] === 'string' ? record['errorType'].trim() : undefined;

  return {
    executionId,
    jobKey: jobKey || 'Unknown job',
    statusLabel: status.label,
    statusKey: status.key,
    startedAtUtc,
    startedAtMs,
    durationMs,
    triggerId: triggerId || undefined,
    instanceId: instanceId || undefined,
    executionMode: executionMode || undefined,
    invocationSource: invocationSource || undefined,
    errorType: errorType || undefined,
  };
}

function normalizeExecutionStatus(value: unknown): { label: string; key: ExecutionStatusKey } {
  const raw = typeof value === 'number' || typeof value === 'string' ? String(value) : '';
  const normalized = raw.trim().toLowerCase();
  if (normalized === '0' || normalized === 'success' || normalized === 'succeeded') {
    return { label: 'Success', key: 'success' };
  }
  if (normalized === '1' || normalized === 'failure' || normalized === 'failed' || normalized === 'error') {
    return { label: 'Failure', key: 'failure' };
  }
  if (normalized === '2' || normalized === 'canceled' || normalized === 'cancelled') {
    return { label: 'Canceled', key: 'canceled' };
  }
  if (normalized === '3' || normalized === 'running') {
    return { label: 'Running', key: 'running' };
  }
  if (normalized === '4' || normalized === 'pending' || normalized === 'queued') {
    return { label: 'Pending', key: 'pending' };
  }
  return { label: raw ? raw : 'Unknown', key: 'unknown' };
}

function normalizeStatusFilter(value: string): ExecutionStatusFilter {
  if (STATUS_OPTIONS.some((option) => option.value === value)) {
    return value as ExecutionStatusFilter;
  }
  return 'all';
}

function normalizeDateRange(value: string): ExecutionDateRange {
  if (DATE_RANGE_OPTIONS.some((option) => option.value === value)) {
    return value as ExecutionDateRange;
  }
  return '24h';
}

function resolveDateRangeCutoff(range: ExecutionDateRange): number | null {
  const now = nowMs();
  switch (range) {
    case '24h':
      return now - 24 * 60 * 60 * 1000;
    case '7d':
      return now - 7 * 24 * 60 * 60 * 1000;
    case '30d':
      return now - 30 * 24 * 60 * 60 * 1000;
    case 'all':
    default:
      return null;
  }
}

function toEpochMs(value?: string): number {
  if (!value) {
    return 0;
  }
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}
