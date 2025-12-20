import { Tab, TabContent, TabList, TabPanel, Tabs } from '@angular/aria/tabs';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, effect, inject, signal } from '@angular/core';
import { ScheduleState } from '@croniq/api-schema';
import type { UpsertScheduleRequest } from '@croniq/api-schema';
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
  id: 'list' | 'edit' | 'deadletters';
  label: string;
};

type ScheduleDraft = {
  triggerId: string;
  jobKey: string;
  cronExpression: string;
  enabled: boolean;
  startAtUtc: string;
  endAtUtc: string;
  description: string;
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

  readonly detailTabs: ReadonlyArray<DetailTab> = [
    { id: 'list', label: 'List' },
    { id: 'edit', label: 'Edit / Create' },
    { id: 'deadletters', label: 'Dead letters' },
  ];
  readonly selectedTab = signal<string>(this.detailTabs[0]?.id ?? '');

  setSelectedTab(nextTab: string | null | undefined): void {
    this.selectedTab.set(nextTab ?? this.detailTabs[0]?.id ?? '');
    if (this.selectedTab() === 'deadletters') {
      this.store.refreshScheduleDeadLetters();
    }
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

  readonly scheduleDetail = this.store.scheduleDetail;
  readonly scheduleDetailLoading = this.store.scheduleDetailLoading;
  readonly scheduleDetailError = this.store.scheduleDetailError;
  readonly deleteScheduleLoading = this.store.deleteScheduleLoading;
  readonly deleteScheduleError = this.store.deleteScheduleError;
  readonly upsertScheduleLoading = this.store.upsertScheduleLoading;
  readonly upsertScheduleError = this.store.upsertScheduleError;

  readonly scheduleDeadLetters = this.store.scheduleDeadLetters;
  readonly scheduleDeadLettersLoading = this.store.scheduleDeadLettersLoading;
  readonly scheduleDeadLettersError = this.store.scheduleDeadLettersError;
  readonly scheduleDeadLetterCount = this.store.scheduleDeadLetterCount;

  readonly stateFilter = signal<ScheduleFilter>('all');
  readonly searchTerm = signal('');

  readonly selectedTriggerId = signal<string | null>(null);
  readonly draft = signal<ScheduleDraft>(createEmptyDraft());
  readonly formError = signal<string | null>(null);
  private readonly prefillPendingTriggerId = signal<string | null>(null);
  private readonly seededCronFromSummary = signal(false);

  readonly selectedScheduleSummary = computed(() => {
    const selected = this.selectedTriggerId();
    if (!selected) {
      return null;
    }
    return this.schedules().find((schedule) => schedule.id === selected) ?? null;
  });

  readonly canDelete = computed(() => {
    const id = this.selectedTriggerId();
    return !!id?.trim();
  });

  readonly detailTriggerIdText = computed(() => this.scheduleDetail()?.triggerId ?? '—');
  readonly detailStateText = computed(() => this.scheduleDetail()?.state ?? '—');
  readonly detailNameText = computed(() => this.scheduleDetail()?.name ?? '—');
  readonly detailJobKeyText = computed(() => this.scheduleDetail()?.jobKey ?? '—');
  readonly detailCronText = computed(() => {
    const detail = this.scheduleDetail();
    if (!detail) {
      return '—';
    }
    return detail.cronExpression ?? detail.cron ?? '—';
  });
  readonly detailEnabledText = computed(() => {
    const enabled = this.scheduleDetail()?.enabled;
    if (typeof enabled !== 'boolean') {
      return '—';
    }
    return enabled ? 'Enabled' : 'Disabled';
  });
  readonly detailStartAtUtcText = computed(() => this.scheduleDetail()?.startAtUtc ?? '—');
  readonly detailEndAtUtcText = computed(() => this.scheduleDetail()?.endAtUtc ?? '—');
  readonly detailDescriptionText = computed(() => this.scheduleDetail()?.description ?? '—');

  constructor() {
    effect(
      () => {
        const pendingId = this.prefillPendingTriggerId();
        if (!pendingId) {
          return;
        }

        const detail = this.scheduleDetail();
        if (!detail || detail.triggerId !== pendingId) {
          return;
        }

        this.draft.update((current) => {
          const next: ScheduleDraft = { ...current };

          if (!next.jobKey.trim() && detail.jobKey) {
            next.jobKey = detail.jobKey;
          }

          const detailCron = detail.cronExpression ?? detail.cron;
          if (detailCron && (!next.cronExpression.trim() || this.seededCronFromSummary())) {
            next.cronExpression = detailCron;
          }

          if (typeof detail.enabled === 'boolean') {
            next.enabled = detail.enabled;
          }

          if (!next.startAtUtc.trim() && detail.startAtUtc) {
            next.startAtUtc = detail.startAtUtc;
          }

          if (!next.endAtUtc.trim() && detail.endAtUtc) {
            next.endAtUtc = detail.endAtUtc;
          }

          if (!next.description.trim() && detail.description) {
            next.description = detail.description;
          }

          if (!next.triggerId.trim() && detail.triggerId) {
            next.triggerId = detail.triggerId;
          }

          return next;
        });

        this.prefillPendingTriggerId.set(null);
        this.seededCronFromSummary.set(false);
      },
      { allowSignalWrites: true },
    );
  }

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

  startCreate(): void {
    this.selectedTriggerId.set(null);
    this.formError.set(null);
    this.draft.set(createEmptyDraft());
    this.prefillPendingTriggerId.set(null);
    this.seededCronFromSummary.set(false);
    this.selectedTab.set('edit');
  }

  openSchedule(triggerId: string): void {
    const trimmed = triggerId.trim();
    if (!trimmed) {
      return;
    }

    this.selectedTriggerId.set(trimmed);
    this.formError.set(null);
    this.selectedTab.set('edit');

    this.prefillPendingTriggerId.set(trimmed);
    this.store.refreshScheduleDetail(trimmed);

    const summary = this.schedules().find((schedule) => schedule.id === trimmed) ?? null;
    this.seededCronFromSummary.set(!!summary?.cron);
    this.draft.set({
      triggerId: trimmed,
      jobKey: '',
      cronExpression: summary?.cron ?? '',
      enabled: true,
      startAtUtc: '',
      endAtUtc: '',
      description: '',
    });
  }

  refreshSelectedSchedule(): void {
    const triggerId = this.selectedTriggerId();
    if (!triggerId?.trim()) {
      return;
    }
    this.store.refreshScheduleDetail(triggerId);
  }

  updateDraftTriggerId(value: unknown): void {
    this.draft.update((current) => ({ ...current, triggerId: typeof value === 'string' ? value : '' }));
  }

  updateDraftJobKey(value: unknown): void {
    this.draft.update((current) => ({ ...current, jobKey: typeof value === 'string' ? value : '' }));
  }

  updateDraftCronExpression(value: unknown): void {
    this.draft.update((current) => ({ ...current, cronExpression: typeof value === 'string' ? value : '' }));
  }

  updateDraftEnabled(value: boolean): void {
    this.draft.update((current) => ({ ...current, enabled: value }));
  }

  updateDraftStartAtUtc(value: unknown): void {
    this.draft.update((current) => ({ ...current, startAtUtc: typeof value === 'string' ? value : '' }));
  }

  updateDraftEndAtUtc(value: unknown): void {
    this.draft.update((current) => ({ ...current, endAtUtc: typeof value === 'string' ? value : '' }));
  }

  updateDraftDescription(value: unknown): void {
    this.draft.update((current) => ({ ...current, description: typeof value === 'string' ? value : '' }));
  }

  submitUpsert(): void {
    const draft = this.draft();
    const jobKey = draft.jobKey.trim();
    const cronExpression = draft.cronExpression.trim();

    if (!jobKey) {
      this.formError.set('Job key is required.');
      return;
    }
    if (!cronExpression) {
      this.formError.set('Cron expression is required.');
      return;
    }

    this.formError.set(null);

    const payload: UpsertScheduleRequest = {
      jobKey,
      cronExpression,
      triggerId: draft.triggerId.trim() ? draft.triggerId.trim() : null,
      enabled: draft.enabled,
      startAtUtc: draft.startAtUtc.trim() ? draft.startAtUtc.trim() : null,
      endAtUtc: draft.endAtUtc.trim() ? draft.endAtUtc.trim() : null,
      description: draft.description.trim() ? draft.description.trim() : null,
    };

    this.store.upsertSchedule(payload);
  }

  deleteSelectedSchedule(): void {
    const triggerId = this.selectedTriggerId();
    if (!triggerId?.trim()) {
      return;
    }
    this.store.deleteSchedule(triggerId);
  }

  refreshDeadLetters(): void {
    this.store.refreshScheduleDeadLetters();
  }

  replayDeadLetter(deadLetterId: number): void {
    this.store.replayScheduleDeadLetter(deadLetterId);
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

function createEmptyDraft(): ScheduleDraft {
  return {
    triggerId: '',
    jobKey: '',
    cronExpression: '',
    enabled: true,
    startAtUtc: '',
    endAtUtc: '',
    description: '',
  };
}
