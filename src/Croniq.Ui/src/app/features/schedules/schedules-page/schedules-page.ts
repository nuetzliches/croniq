import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
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

  // Actions
  setViewMode(mode: 'list' | 'calendar' | 'dead-letters' | 'logs') {
    this.viewMode.set(mode);
  }

  createSchedule() {
    this.editingSchedule.set(null);
    this.showDialog.set(true);
  }

  editSchedule(schedule: ScheduleSummary) {
    // Map schedule to UpsertScheduleRequest
    // Note: Some fields like jobKey are missing in summary and should ideally be fetched via detail
    const request: UpsertScheduleRequest = {
      triggerId: schedule.id,
      jobKey: schedule.name, // Fallback to name for now, assuming it might be the key
      cronExpression: schedule.cron,
      enabled: schedule.state === 'active',
      description: '',
    };
    this.editingSchedule.set(request);
    this.showDialog.set(true);
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
