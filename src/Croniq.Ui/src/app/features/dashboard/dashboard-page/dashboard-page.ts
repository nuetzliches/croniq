import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { RouterLink } from '@angular/router';

type MetricCard = {
  label: string;
  value: string;
  trend?: string;
  status?: 'healthy' | 'warning' | 'critical';
  subtext?: string;
};

type DeadLetter = {
  jobKey: string;
  reason: string;
  time: string;
};

type UpcomingSchedule = {
  jobKey: string;
  fireTime: string;
};

@Component({
  selector: 'cq-dashboard-page',
  imports: [RouterLink],
  templateUrl: './dashboard-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DashboardPage {
  readonly metrics = signal<ReadonlyArray<MetricCard>>([
    { label: 'Active Runners', value: '8', status: 'healthy', subtext: 'All systems operational' },
    { label: 'Throughput (RPM)', value: '1,240', trend: '↑ 12%', subtext: 'vs last hour' },
    { label: 'Error Rate (1h)', value: '0.05%', status: 'healthy', subtext: 'Below threshold' },
  ]);

  readonly recentFailures = signal<ReadonlyArray<DeadLetter>>([
    { jobKey: 'payment-sync', reason: 'Timeout', time: '2m ago' },
    { jobKey: 'email-send', reason: '500 Error', time: '15m ago' },
    { jobKey: 'data-export', reason: 'Connection Refused', time: '1h ago' },
  ]);

  readonly upcomingSchedules = signal<ReadonlyArray<UpcomingSchedule>>([
    { jobKey: 'daily-report', fireTime: 'in 5m' },
    { jobKey: 'cleanup-logs', fireTime: 'in 1h' },
    { jobKey: 'billing-cycle', fireTime: 'Tomorrow 00:00' },
  ]);
}
