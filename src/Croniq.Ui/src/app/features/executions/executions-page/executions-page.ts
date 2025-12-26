import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { CommonModule } from '@angular/common';

interface Execution {
  id: string;
  jobName: string;
  status: 'Success' | 'Failed' | 'Running' | 'Pending';
  startTime: Date;
  duration: string;
  trigger: string;
}

@Component({
  selector: 'cq-executions-page',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './executions-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ExecutionsPage {
  // Filters
  searchQuery = signal('');
  statusFilter = signal<string>('All');
  dateRangeFilter = signal<string>('24h');

  // Data
  executions = signal<Execution[]>([
    { id: 'exec-123', jobName: 'billing-sync', status: 'Success', startTime: new Date(), duration: '45s', trigger: 'Schedule' },
    { id: 'exec-124', jobName: 'email-digest', status: 'Running', startTime: new Date(), duration: '12s', trigger: 'Manual' },
    { id: 'exec-125', jobName: 'data-backup', status: 'Failed', startTime: new Date(), duration: '2m', trigger: 'Schedule' },
    { id: 'exec-126', jobName: 'report-gen', status: 'Pending', startTime: new Date(), duration: '-', trigger: 'Webhook' },
    { id: 'exec-127', jobName: 'cleanup-logs', status: 'Success', startTime: new Date(Date.now() - 3600000), duration: '1m 20s', trigger: 'Schedule' },
  ]);

  // Actions
  viewLogs(id: string) {
    console.log('View logs for', id);
  }

  cancelExecution(id: string) {
    console.log('Cancel execution', id);
  }
}
