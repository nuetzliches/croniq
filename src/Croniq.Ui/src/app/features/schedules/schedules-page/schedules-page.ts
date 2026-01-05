import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, effect, inject, signal } from '@angular/core';
import { ScheduleSummary, UpsertScheduleRequest } from '@croniq/api-schema';
import { ScheduleDialogComponent } from '@features/schedules/components/schedule-dialog/schedule-dialog.component';
import { SchedulesStore } from './schedules.store';

@Component({
  selector: 'cq-schedules-page',
  imports: [DatePipe, ScheduleDialogComponent],
  templateUrl: './schedules-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [SchedulesStore],
})
export class SchedulesPage {
  private readonly store = inject(SchedulesStore);

  // View State
  viewMode = signal<'list' | 'calendar' | 'dead-letters' | 'logs'>('list');

  // Data
  loading = this.store.loading;
  schedules = this.store.schedules;

  deadLetters = this.store.scheduleDeadLetters;
  deadLettersLoading = this.store.scheduleDeadLettersLoading;

  executions = this.store.executions;
  executionsLoading = this.store.executionsLoading;

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
