import { DatePipe } from '@angular/common';
import { CdkMenu } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { RouterLink } from '@angular/router';
import { bindQueryParam } from '@shared/routing/selection-sync';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { CqCellDefDirective, CqColumnComponent, CqContextMenuItemDirective, CqIconComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';
import { Runner, RunnersStore } from './runners.store';

type RunnerStatusFilter = 'all' | Runner['status'];

const STATUS_OPTIONS: ReadonlyArray<{ value: RunnerStatusFilter; label: string }> = [
  { value: 'all', label: 'All statuses' },
  { value: 'Online', label: 'Online' },
  { value: 'Offline', label: 'Offline' },
  { value: 'Draining', label: 'Draining' },
];


@Directive({
  selector: '[cqRunnerCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqRunnerCellDirective }],
})
export class CqRunnerCellDirective extends CqCellDefDirective<Runner> {
  // Inherits ngTemplateContextGuard from base class.
}

@Component({
  selector: 'cq-runners-page',
  imports: [CdkMenu, DatePipe, RouterLink, DataGrid, CqColumnComponent, CqRunnerCellDirective, CqInputDirective, CqSelectDirective, CqContextMenuItemDirective, CqIconComponent],
  templateUrl: './runners-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [RunnersStore],
})
export class RunnersPage {
  private readonly store = inject(RunnersStore);
  private readonly shellPanel = inject(ShellPanelService);
  private readonly panelTemplate = viewChild<TemplateRef<unknown>>('runnersFilterPanel');
  private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('runnersFilterCollapsed');

  // Data
  readonly runners = this.store.runners;
  readonly loading = this.store.loading;
  readonly error = this.store.error;
  readonly actionError = this.store.actionError;

  readonly runnerSearch = signal('');
  readonly statusFilter = signal<RunnerStatusFilter>('all');
  readonly selectedTags = signal<ReadonlyArray<string>>([]);
  readonly selectedRunnerId = bindQueryParam({ paramKey: 'runnerId' });
  readonly statusOptions = STATUS_OPTIONS;

  readonly tagOptions = computed(() => {
    const tags = new Set<string>();
    this.runners().forEach((runner) => runner.tags.forEach((tag) => tags.add(tag)));
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  });

  readonly filteredRunners = computed(() => {
    const query = this.runnerSearch().trim().toLowerCase();
    const statusFilter = this.statusFilter();
    const selectedTags = new Set(this.selectedTags());

    return this.runners().filter((runner) => {
      if (statusFilter !== 'all' && runner.status !== statusFilter) {
        return false;
      }

      if (selectedTags.size > 0 && !runner.tags.some((tag) => selectedTags.has(tag))) {
        return false;
      }

      if (!query) {
        return true;
      }

      return (
        runner.id.toLowerCase().includes(query) ||
        runner.hostname.toLowerCase().includes(query) ||
        runner.tags.some((tag) => tag.toLowerCase().includes(query))
      );
    });
  });

  readonly selectedRunner = computed(() => {
    const raw = this.selectedRunnerId();
    if (raw === null || raw === undefined) {
      return null;
    }
    const id = typeof raw === 'string' ? raw : String(raw);
    return this.runners().find((runner) => runner.id === id) ?? null;
  });

  // Metrics
  readonly activeRunnersCount = this.store.activeRunnersCount;
  readonly totalCapacity = this.store.totalCapacity;
  readonly busyThreads = this.store.busyThreads;
  readonly presenceTransportLabel = this.store.presenceTransportLabel;

  runnerRowKey = (row: Runner, index: number) => row.id ?? `runner-${index}`;

  runnerRowClasses = (row: Runner) =>
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
        'Refine the runner list view.',
        collapsedTemplate ?? null,
      );
      onCleanup(() => this.shellPanel.clearPanel(template));
    });

  }

  refresh() {
    this.store.refresh();
  }

  setRunnerSearch(query: string): void {
    this.runnerSearch.set(query);
  }

  setStatusFilter(status: RunnerStatusFilter): void {
    this.statusFilter.set(status);
  }

  resetFilters(): void {
    this.runnerSearch.set('');
    this.statusFilter.set('all');
    this.selectedTags.set([]);
  }

  isTagSelected(tag: string): boolean {
    return this.selectedTags().includes(tag);
  }

  toggleTagSelection(tag: string, checked: boolean): void {
    this.selectedTags.update((current) =>
      checked
        ? Array.from(new Set([...current, tag]))
        : current.filter((entry) => entry !== tag),
    );
  }

  // Actions
  drainRunner(id: string) {
    this.store.drainRunner(id);
  }

  deregisterRunner(id: string) {
    this.store.deregisterRunner(id);
  }
}
