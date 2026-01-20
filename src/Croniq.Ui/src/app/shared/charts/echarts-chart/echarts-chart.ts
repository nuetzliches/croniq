import { ChangeDetectionStrategy, Component, ElementRef, effect, input, signal, viewChild } from '@angular/core';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsCoreOption } from 'echarts/core';

echarts.use([LineChart, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

@Component({
  selector: 'cq-echarts-chart',
  template: '<div #host class="h-full w-full"></div>',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'block h-full w-full',
    role: 'img',
    '[attr.aria-label]': 'ariaLabel()',
  },
})
export class CqEchartsChartComponent {
  readonly options = input<EChartsCoreOption | null>(null);
  readonly loading = input(false);
  readonly ariaLabel = input('Chart');

  private readonly hostRef = viewChild<ElementRef<HTMLDivElement>>('host');
  private readonly chartSignal = signal<echarts.ECharts | null>(null);

  constructor() {
    effect((onCleanup) => {
      const host = this.hostRef()?.nativeElement;
      if (!host) {
        return;
      }

      const chart = echarts.init(host);
      this.chartSignal.set(chart);

      const resizeObserver = typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(() => chart.resize())
        : null;

      if (resizeObserver) {
        resizeObserver.observe(host);
      }

      onCleanup(() => {
        resizeObserver?.disconnect();
        chart.dispose();
        this.chartSignal.set(null);
      });
    });

    effect(() => {
      const chart = this.chartSignal();
      if (!chart) {
        return;
      }

      const options = this.options();
      if (!options) {
        chart.clear();
        return;
      }

      chart.setOption(options, { notMerge: true, lazyUpdate: true });
    });

    effect(() => {
      const chart = this.chartSignal();
      if (!chart) {
        return;
      }

      if (this.loading()) {
        chart.showLoading('default', { text: 'Loading...' });
      } else {
        chart.hideLoading();
      }
    });
  }
}
