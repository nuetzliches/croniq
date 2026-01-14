import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, computed, effect, inject, signal } from '@angular/core';
import { epochMsFromIso, nowMs } from '@core/time/clock';
import { ScheduleSummary, UpsertScheduleRequest } from '@croniq/api-schema';
import { ScheduleDialogComponent } from '@features/schedules/components/schedule-dialog/schedule-dialog.component';
import { ColumnCellContext, CqCellDefDirective, CqColumnComponent, DataGrid } from 'ui-kit';
import { SchedulesStore } from './schedules.store';

type ScheduleCalendarEntry = {
  entryId: string;
  scheduleId: string;
  name: string;
  cron: string;
  timezone: string;
  stateLabel: string;
  isActive: boolean;
  timeLabel: string;
  hasTime: boolean;
  fireAtMs?: number;
};

type ScheduleCalendarDay = {
  key: string;
  dayLabel: string;
  dateLabel: string;
  monthLabel: string;
  isToday: boolean;
  entries: ReadonlyArray<ScheduleCalendarEntry>;
  hasEntries: boolean;
};

type ScheduleCalendarModel = {
  days: ReadonlyArray<ScheduleCalendarDay>;
  unscheduled: ReadonlyArray<ScheduleCalendarEntry>;
  entryCount: number;
  rangeLabel: string;
};

const CALENDAR_DAY_COUNT = 7;

@Directive({
  selector: '[cqScheduleCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqScheduleCellDirective }],
})
export class CqScheduleCellDirective extends CqCellDefDirective<ScheduleSummary> {
  // Inherits ngTemplateContextGuard from base class
}

@Component({
  selector: 'cq-schedules-page',
  imports: [DatePipe, ScheduleDialogComponent, DataGrid, CqColumnComponent, CqScheduleCellDirective],
  templateUrl: './schedules-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [SchedulesStore],
})
export class SchedulesPage {
  private readonly store = inject(SchedulesStore);
  private readonly calendarAnchorMs = signal(startOfLocalDayMs(nowMs()));
  private readonly calendarOffsetWeeks = signal(0);

  // View State
  viewMode = signal<'list' | 'calendar' | 'dead-letters' | 'logs'>('list');

  // Data
  loading = this.store.loading;
  schedules = this.store.schedules;
  error = this.store.error;

  deadLetters = this.store.scheduleDeadLetters;
  deadLettersLoading = this.store.scheduleDeadLettersLoading;
  deadLettersError = this.store.scheduleDeadLettersError;

  executions = this.store.executions;
  executionsLoading = this.store.executionsLoading;
  executionsError = this.store.executionsError;

  private readonly calendarStartMs = computed(() =>
    addLocalDaysMs(this.calendarAnchorMs(), this.calendarOffsetWeeks() * CALENDAR_DAY_COUNT),
  );

  private readonly calendarModel = computed<ScheduleCalendarModel>(() =>
    buildCalendarModel(this.schedules(), this.calendarStartMs(), CALENDAR_DAY_COUNT, this.calendarAnchorMs()),
  );

  readonly calendarDays = computed(() => this.calendarModel().days);
  readonly calendarRangeLabel = computed(() => this.calendarModel().rangeLabel);
  readonly calendarUnscheduled = computed(() => this.calendarModel().unscheduled);
  readonly calendarUnscheduledCount = computed(() => this.calendarModel().unscheduled.length);
  readonly calendarHasEntries = computed(() => this.calendarModel().entryCount > 0);
  readonly calendarHasUnscheduled = computed(() => this.calendarModel().unscheduled.length > 0);
  readonly calendarIsEmpty = computed(() => !this.calendarHasEntries() && !this.calendarHasUnscheduled());
  readonly calendarShowEmpty = computed(() => this.calendarIsEmpty() && !this.loading());

  // Dialog State
  showDialog = signal(false);
  editingSchedule = signal<UpsertScheduleRequest | null>(null);
  loadingDetailId = signal<string | null>(null);

  constructor() {
    effect(() => {
      const loadingId = this.loadingDetailId();
      const detail = this.store.scheduleDetail();
      const isLoading = this.store.scheduleDetailLoading();

      if (loadingId && !isLoading && detail && detail.triggerId === loadingId) {
        // Detail loaded, open dialog
        const request: UpsertScheduleRequest = {
          triggerId: detail.triggerId,
          jobKey: detail.jobKey,
          cronExpression: detail.cronExpression,
          enabled: detail.enabled,
          description: detail.description,
          startAtUtc: detail.startAtUtc,
          endAtUtc: detail.endAtUtc,
        };
        this.editingSchedule.set(request);
        this.showDialog.set(true);
        this.loadingDetailId.set(null);
      }
    });
  }

  // Actions
  setViewMode(mode: 'list' | 'calendar' | 'dead-letters' | 'logs') {
    this.viewMode.set(mode);
  }

  showPreviousWeek() {
    this.calendarOffsetWeeks.update((value) => value - 1);
  }

  showNextWeek() {
    this.calendarOffsetWeeks.update((value) => value + 1);
  }

  showCurrentWeek() {
    this.calendarAnchorMs.set(startOfLocalDayMs(nowMs()));
    this.calendarOffsetWeeks.set(0);
  }

  scheduleRowKey = (row: ScheduleSummary, index: number) => row.id ?? `schedule-${index}`;

  scheduleRowClasses = (row: ScheduleSummary) =>
    row.state === 'active' ? undefined : ['opacity-80'];

  createSchedule() {
    this.editingSchedule.set(null);
    this.showDialog.set(true);
  }

  editSchedule(schedule: ScheduleSummary) {
    this.loadingDetailId.set(schedule.id);
    this.store.refreshScheduleDetail(schedule.id);
  }

  deleteSchedule(triggerId: string) {
    if (confirm('Are you sure you want to delete this schedule?')) {
      this.store.deleteSchedule(triggerId);
    }
  }

  replayDeadLetter(id: number) {
    this.store.replayScheduleDeadLetter(id);
  }

  onSave(request: UpsertScheduleRequest) {
    this.store.upsertSchedule(request);
    this.showDialog.set(false);
  }

  onCancel() {
    this.showDialog.set(false);
  }
}

const DAY_LABELS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const;
const MONTH_LABELS = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
] as const;

function buildCalendarModel(
  schedules: ReadonlyArray<ScheduleSummary>,
  startMs: number,
  dayCount: number,
  todayMs: number,
): ScheduleCalendarModel {
  const todayKey = localDateKey(todayMs);
  const dayKeys = new Set<string>();
  const dayMsList: number[] = [];

  for (let dayIndex = 0; dayIndex < dayCount; dayIndex += 1) {
    const dayMs = addLocalDaysMs(startMs, dayIndex);
    dayMsList.push(dayMs);
    dayKeys.add(localDateKey(dayMs));
  }

  const entriesByDay = new Map<string, ScheduleCalendarEntry[]>();
  const unscheduled: ScheduleCalendarEntry[] = [];
  let entryCount = 0;

  schedules.forEach((schedule, index) => {
    const nextFireMs = epochMsFromIso(schedule.nextFire ?? '');
    if (nextFireMs == null) {
      unscheduled.push(buildCalendarEntry(schedule, index, null));
      return;
    }

    const dayKey = localDateKey(nextFireMs);
    if (!dayKeys.has(dayKey)) {
      return;
    }

    const entry = buildCalendarEntry(schedule, index, nextFireMs);
    const bucket = entriesByDay.get(dayKey);
    if (bucket) {
      bucket.push(entry);
    } else {
      entriesByDay.set(dayKey, [entry]);
    }
    entryCount += 1;
  });

  const days = dayMsList.map((dayMs) => {
    const key = localDateKey(dayMs);
    const entries = (entriesByDay.get(key) ?? []).slice().sort(sortCalendarEntries);
    const labelParts = formatDayParts(dayMs);

    return {
      key,
      dayLabel: labelParts.dayLabel,
      dateLabel: labelParts.dateLabel,
      monthLabel: labelParts.monthLabel,
      isToday: key === todayKey,
      entries,
      hasEntries: entries.length > 0,
    };
  });

  return {
    days,
    unscheduled,
    entryCount,
    rangeLabel: formatRangeLabel(startMs, dayCount),
  };
}

function buildCalendarEntry(
  schedule: ScheduleSummary,
  index: number,
  fireAtMs: number | null,
): ScheduleCalendarEntry {
  const scheduleId = schedule.id?.trim() || `schedule-${index}`;
  const name = schedule.name?.trim() || scheduleId;
  const cron = schedule.cron?.trim() || 'n/a';
  const timezone = schedule.timezone?.trim() || 'UTC';
  const isActive = schedule.state === 'active';
  const timeLabel = fireAtMs == null ? 'n/a' : formatTimeLabel(fireAtMs);
  const entryId = fireAtMs == null ? `${scheduleId}:unscheduled` : `${scheduleId}:${fireAtMs}`;

  return {
    entryId,
    scheduleId,
    name,
    cron,
    timezone,
    stateLabel: isActive ? 'Active' : 'Paused',
    isActive,
    timeLabel,
    hasTime: fireAtMs != null,
    fireAtMs: fireAtMs ?? undefined,
  };
}

function sortCalendarEntries(a: ScheduleCalendarEntry, b: ScheduleCalendarEntry): number {
  if (a.fireAtMs == null && b.fireAtMs == null) {
    return a.name.localeCompare(b.name);
  }
  if (a.fireAtMs == null) {
    return 1;
  }
  if (b.fireAtMs == null) {
    return -1;
  }
  if (a.fireAtMs === b.fireAtMs) {
    return a.name.localeCompare(b.name);
  }
  return a.fireAtMs - b.fireAtMs;
}

function startOfLocalDayMs(epochMs: number): number {
  const date = new Date(epochMs);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

function addLocalDaysMs(epochMs: number, days: number): number {
  const date = new Date(epochMs);
  date.setDate(date.getDate() + days);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

function localDateKey(epochMs: number): string {
  const date = new Date(epochMs);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function formatDayParts(epochMs: number): { dayLabel: string; dateLabel: string; monthLabel: string } {
  const date = new Date(epochMs);
  const dayLabel = DAY_LABELS[date.getDay()] ?? 'Day';
  const dateLabel = String(date.getDate()).padStart(2, '0');
  const monthLabel = MONTH_LABELS[date.getMonth()] ?? '';
  return { dayLabel, dateLabel, monthLabel };
}

function formatRangeLabel(startMs: number, dayCount: number): string {
  const startLabel = formatMonthDayLabel(startMs);
  const endLabel = formatMonthDayLabel(addLocalDaysMs(startMs, Math.max(0, dayCount - 1)));
  return `${startLabel} - ${endLabel}`;
}

function formatMonthDayLabel(epochMs: number): string {
  const date = new Date(epochMs);
  const monthLabel = MONTH_LABELS[date.getMonth()] ?? '';
  const day = String(date.getDate()).padStart(2, '0');
  return `${monthLabel} ${day}`;
}

function formatTimeLabel(epochMs: number): string {
  const date = new Date(epochMs);
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  return `${hours}:${minutes}`;
}
