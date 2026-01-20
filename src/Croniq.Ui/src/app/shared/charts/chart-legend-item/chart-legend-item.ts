import { ChangeDetectionStrategy, Component, input } from '@angular/core';

export type ChartLegendIntent = 'success' | 'warning' | 'failed';

@Component({
  selector: 'cq-chart-legend-item',
  templateUrl: './chart-legend-item.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'inline-flex items-center gap-1.5',
  },
})
export class ChartLegendItem {
  readonly label = input.required<string>();
  readonly count = input.required<number>();
  readonly percentLabel = input.required<string>();
  readonly intent = input.required<ChartLegendIntent>();
}
