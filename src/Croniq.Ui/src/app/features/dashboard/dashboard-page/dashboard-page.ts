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
  readonly misfireHeatmap = this.store.misfireHeatmap;

  getSparklinePath(values: number[] | undefined): string {
    if (!values || values.length < 2) return '';

    const width = 100;
    const height = 30;
    // Pad min/max slightly to avoid cutting off stroke
    const min = Math.min(...values);
    const max = Math.max(...values);
    const range = max - min || 1;

    const points = values.map((val, i) => {
      const x = (i / (values.length - 1)) * width;
      const y = height - ((val - min) / range) * height; // Invert Y
      return `${x},${y}`;
    });

    return `M ${points.join(' L ')}`;
  }
}
