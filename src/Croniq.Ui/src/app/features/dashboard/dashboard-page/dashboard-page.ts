import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { DashboardStore } from './dashboard.store';

@Component({
  selector: 'cq-dashboard-page',
  imports: [RouterLink, DatePipe],
  templateUrl: './dashboard-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [DashboardStore],
})
export class DashboardPage {
  private readonly store = inject(DashboardStore);

  readonly loading = this.store.loading;
  readonly metrics = this.store.metrics;
  readonly recentFailures = this.store.recentFailures;
  readonly upcomingSchedules = this.store.upcomingSchedules;
}
