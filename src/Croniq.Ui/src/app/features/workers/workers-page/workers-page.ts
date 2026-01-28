import { DatePipe } from '@angular/common';
import { CdkMenu } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { bindQueryParam } from '@shared/routing/selection-sync';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { CqCellDefDirective, CqColumnComponent, CqContextMenuItemDirective, CqIconComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';
import { Worker, WorkersStore } from './workers.store';

type WorkerStatusFilter = 'all' | Worker['status'];
type WorkerDispatchFilter = 'all' | Worker['dispatchState'];

const STATUS_OPTIONS: ReadonlyArray<{ value: WorkerStatusFilter; label: string }> = [
  { value: 'all', label: 'All statuses' },
  { value: 'Online', label: 'Online' },
  { value: 'Offline', label: 'Offline' },
  { value: 'Draining', label: 'Draining' },
];

const DISPATCH_OPTIONS: ReadonlyArray<{ value: WorkerDispatchFilter; label: string }> = [
  { value: 'all', label: 'All dispatch states' },
  { value: 'Connected', label: 'Connected' },
  { value: 'Fallback', label: 'Fallback' },
  { value: 'Unknown', label: 'Unknown' },
];

@Directive({
  selector: '[cqWorkerCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqWorkerCellDirective }],
})
export class CqWorkerCellDirective extends CqCellDefDirective<Worker> {
  // Inherits ngTemplateContextGuard from base class.
}

@Component({
  selector: 'cq-workers-page',
  imports: [
    CdkMenu,
    DatePipe,
    DataGrid,
    CqColumnComponent,
    CqWorkerCellDirective,
    CqInputDirective,
    CqSelectDirective,
    CqContextMenuItemDirective,
    CqIconComponent,
  ],
  templateUrl: './workers-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [WorkersStore],
})
export class WorkersPage {
  private readonly store = inject(WorkersStore);
  private readonly shellPanel = inject(ShellPanelService);
  private readonly panelTemplate = viewChild<TemplateRef<unknown>>('workersFilterPanel');
  private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('workersFilterCollapsed');

  // Data
  readonly workers = this.store.workers;
  readonly loading = this.store.loading;
  readonly error = this.store.error;

  readonly workerSearch = signal('');
  readonly statusFilter = signal<WorkerStatusFilter>('all');
  readonly dispatchFilter = signal<WorkerDispatchFilter>('all');
  readonly selectedTags = signal<ReadonlyArray<string>>([]);
  readonly selectedWorkerId = bindQueryParam({ paramKey: 'workerId' });
  readonly statusOptions = STATUS_OPTIONS;
  readonly dispatchOptions = DISPATCH_OPTIONS;

  readonly tagOptions = computed(() => {
    const tags = new Set<string>();
    this.workers().forEach((worker) => worker.tags.forEach((tag) => tags.add(tag)));
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  });

  readonly filteredWorkers = computed(() => {
    const query = this.workerSearch().trim().toLowerCase();
    const statusFilter = this.statusFilter();
    const dispatchFilter = this.dispatchFilter();
    const selectedTags = new Set(this.selectedTags());

    return this.workers().filter((worker) => {
      if (statusFilter !== 'all' && worker.status !== statusFilter) {
        return false;
      }

      if (dispatchFilter !== 'all' && worker.dispatchState !== dispatchFilter) {
        return false;
      }

      if (selectedTags.size > 0 && !worker.tags.some((tag) => selectedTags.has(tag))) {
        return false;
      }

      if (!query) {
        return true;
      }

      return (
        worker.id.toLowerCase().includes(query) ||
        worker.hostname.toLowerCase().includes(query) ||
        worker.tags.some((tag) => tag.toLowerCase().includes(query))
      );
    });
  });

  readonly selectedWorker = computed(() => {
    const raw = this.selectedWorkerId();
    if (raw === null || raw === undefined) {
      return null;
    }
    const id = typeof raw === 'string' ? raw : String(raw);
    return this.workers().find((worker) => worker.id === id) ?? null;
  });

  workerRowKey = (row: Worker, index: number) => row.id ?? `worker-${index}`;

  workerRowClasses = (row: Worker) =>
    row.status === 'Offline' ? ['opacity-70'] : undefined;

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
        'Refine the worker list view.',
        collapsedTemplate ?? null,
      );
      onCleanup(() => this.shellPanel.clearPanel(template));
    });
  }

  refresh() {
    this.store.refresh();
  }

  setWorkerSearch(query: string): void {
    this.workerSearch.set(query);
  }

  setStatusFilter(status: WorkerStatusFilter): void {
    this.statusFilter.set(status);
  }

  setDispatchFilter(state: WorkerDispatchFilter): void {
    this.dispatchFilter.set(state);
  }

  resetFilters(): void {
    this.workerSearch.set('');
    this.statusFilter.set('all');
    this.dispatchFilter.set('all');
    this.selectedTags.set([]);
  }

  isTagSelected(tag: string): boolean {
    return this.selectedTags().includes(tag);
  }

  toggleTagSelection(tag: string, checked: boolean): void {
    this.selectedTags.update((current) =>
      checked ? Array.from(new Set([...current, tag])) : current.filter((entry) => entry !== tag),
    );
  }

  // Actions
  drainWorker(id: string) {
    console.log('Drain worker', id);
  }

  deregisterWorker(id: string) {
    console.log('Deregister worker', id);
  }
}
