import { ChangeDetectionStrategy, Component, signal } from '@angular/core';

type SummaryCard = {
  label: string;
  value: string;
  description: string;
};

@Component({
  selector: 'app-dashboard-page',
  imports: [],
  templateUrl: './dashboard-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DashboardPage {
  readonly summaryCards = signal<ReadonlyArray<SummaryCard>>([
    { label: 'Active schedules', value: '128', description: 'Enabled policies across tenants' },
    { label: 'Queue depth', value: '42', description: 'Waiting jobs in the last minute' },
    { label: 'Misfires today', value: '3', description: 'Automatically retried triggers' },
    { label: 'Avg. webhook latency', value: '210 ms', description: 'p95 delivery round trip' },
  ]);
}
