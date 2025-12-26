import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { CommonModule } from '@angular/common';

interface Schedule {
  triggerId: string;
  jobKey: string;
  cronExpression: string;
  nextFireTime?: string;
  enabled: boolean;
}

@Component({
  selector: 'cq-schedules-page',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './schedules-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SchedulesPage {
  // View State
  viewMode = signal<'list' | 'calendar'>('list');
  loading = signal<boolean>(false);

  // Data
  schedules = signal<Schedule[]>([
    { triggerId: 't-12345', jobKey: 'invoice-gen', cronExpression: '0 0 * * *', nextFireTime: 'Tomorrow 00:00', enabled: true },
    { triggerId: 't-67890', jobKey: 'email-digest', cronExpression: '0 9 * * 1', nextFireTime: 'Mon 09:00', enabled: true },
    { triggerId: 't-11223', jobKey: 'data-backup', cronExpression: '0 2 * * *', nextFireTime: 'Tomorrow 02:00', enabled: false },
  ]);

  // Actions
  setViewMode(mode: 'list' | 'calendar') {
    this.viewMode.set(mode);
  }

  createSchedule() {
    console.log('Create schedule');
  }
}
