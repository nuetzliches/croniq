import { Tab, TabContent, TabList, TabPanel, Tabs } from '@angular/aria/tabs';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { ScheduleState } from '@croniq/api-schema';
import { SchedulesStore } from './schedules.store';

type ScheduleFilter = ScheduleState | 'all';
type StatusOverview = {
  label: string;
  state: ScheduleState;
  count: number;
};
type FilterOption = {
  label: string;
  value: ScheduleFilter;
};

type DetailTab = {
  id: 'list';
  label: string;
};

@Component({
  selector: 'cq-schedules-page',
  imports: [DatePipe, Tabs, TabList, Tab, TabPanel, TabContent],
  providers: [SchedulesStore],
  templateUrl: './schedules-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SchedulesPage {
  private readonly store = inject(SchedulesStore);

  readonly detailTabs: ReadonlyArray<DetailTab> = [{ id: 'list', label: 'List' }];
  readonly selectedTab = signal<string>(this.detailTabs[0]?.id ?? '');

  setSelectedTab(nextTab: string | null | undefined): void {
    this.selectedTab.set(nextTab ?? this.detailTabs[0]?.id ?? '');
  }

  readonly filterOptions: ReadonlyArray<FilterOption> = [
    { label: 'All', value: 'all' },
    { label: 'Active', value: 'active' },
    { label: 'Paused', value: 'paused' },
    { label: 'Degraded', value: 'degraded' },
  ];

  readonly schedules = this.store.schedules;
  readonly loadError = this.store.error;
  readonly lastUpdated = this.store.lastUpdated;
  readonly isLoading = this.store.loading;

  readonly stateFilter = signal<ScheduleFilter>('all');
  readonly searchTerm = signal('');

  readonly filteredSchedules = computed(() => {
    const term = this.searchTerm().trim().toLowerCase();
    return this.schedules().filter((schedule) => {
      const matchesFilter = this.stateFilter() === 'all' || schedule.state === this.stateFilter();
      const matchesTerm = term
        ? [schedule.name, schedule.owner, schedule.tenant].some((value) =>
          value.toLowerCase().includes(term)
        )
        : true;
      return matchesFilter && matchesTerm;
    });
  });

  readonly statusOverview = computed<ReadonlyArray<StatusOverview>>(() => {
    const tally: Record<ScheduleState, number> = {
      active: 0,
      paused: 0,
      degraded: 0,
    };
    for (const schedule of this.schedules()) {
      tally[schedule.state] += 1;
    }
    return [
      { label: 'Active', state: 'active', count: tally.active },
      { label: 'Paused', state: 'paused', count: tally.paused },
      { label: 'Degraded', state: 'degraded', count: tally.degraded },
    ];
  });

  readonly totalSchedules = computed(() => this.schedules().length);

  setFilter(filter: ScheduleFilter): void {
    this.stateFilter.set(filter);
  }

  onSearch(term: string): void {
    this.searchTerm.set(term);
  }

  refreshSchedules(): void {
    void this.store.refresh();
  }

  formatDuration(ms: number): string {
    if (ms >= 1000) {
      return `${(ms / 1000).toFixed(1)}s`;
    }
    return `${ms}ms`;
  }

  formatAlerts(alerts: number): string {
    if (alerts === 0) {
      return '—';
    }
    return `${alerts} alert${alerts === 1 ? '' : 's'}`;
  }
}
