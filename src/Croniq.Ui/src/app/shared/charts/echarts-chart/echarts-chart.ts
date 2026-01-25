import { ChangeDetectionStrategy, Component, ElementRef, effect, input, output, signal, viewChild } from '@angular/core';
import * as echarts from 'echarts/core';
import { BarChart, LineChart, ScatterChart } from 'echarts/charts';
import { DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, ToolboxComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsCoreOption } from 'echarts/core';

echarts.use([
  BarChart,
  LineChart,
  ScatterChart,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  DataZoomComponent,
  ToolboxComponent,
  CanvasRenderer,
]);

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
  readonly enableZoomSelect = input(false);
  readonly chartClick = output<unknown>();
  readonly chartZoom = output<unknown>();

  private readonly hostRef = viewChild<ElementRef<HTMLDivElement>>('host');
  private readonly chartSignal = signal<echarts.ECharts | null>(null);
  private readonly clickHandler = (event: unknown) => {
    this.chartClick.emit(event);
  };
  private readonly zoomHandler = (event: unknown) => {
    this.chartZoom.emit(event);
  };

  constructor() {
    effect((onCleanup) => {
      const host = this.hostRef()?.nativeElement;
      if (!host) {
        return;
      }

      let chart: echarts.ECharts | null = null;
      let resizeObserver: ResizeObserver | null = null;
      let rafId: number | null = null;

      const hasSize = () => host.clientWidth > 0 && host.clientHeight > 0;
      const tryInit = () => {
        if (chart || !hasSize()) {
          return;
        }
        chart = echarts.init(host);
        this.chartSignal.set(chart);
      };

      const scheduleCheck = () => {
        if (typeof requestAnimationFrame === 'undefined' || rafId !== null) {
          return;
        }
        rafId = requestAnimationFrame(() => {
          rafId = null;
          tryInit();
          chart?.resize();
        });
      };

      if (typeof ResizeObserver !== 'undefined') {
        resizeObserver = new ResizeObserver(() => {
          tryInit();
          chart?.resize();
        });
        resizeObserver.observe(host);
      }

      tryInit();
      if (!chart) {
        scheduleCheck();
        if (typeof window !== 'undefined') {
          const onLoad = () => tryInit();
          window.addEventListener('load', onLoad, { once: true });
          onCleanup(() => window.removeEventListener('load', onLoad));
        }
      }

      onCleanup(() => {
        resizeObserver?.disconnect();
        if (rafId !== null && typeof cancelAnimationFrame !== 'undefined') {
          cancelAnimationFrame(rafId);
        }
        chart?.dispose();
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
      if (this.enableZoomSelect()) {
        chart.dispatchAction({
          type: 'takeGlobalCursor',
          key: 'dataZoomSelect',
          dataZoomSelectActive: true,
        });
      }
    });

    effect((onCleanup) => {
      const chart = this.chartSignal();
      if (!chart) {
        return;
      }

      chart.on('click', this.clickHandler);
      chart.on('datazoom', this.zoomHandler);
      onCleanup(() => {
        chart.off('click', this.clickHandler);
        chart.off('datazoom', this.zoomHandler);
      });
    });

    effect(() => {
      const chart = this.chartSignal();
      if (!chart) {
        return;
      }

      if (this.loading()) {
        const palette = resolveLoadingPalette();
        chart.showLoading('default', {
          text: 'Loading...',
          color: palette.accent,
          textColor: palette.text,
          maskColor: palette.mask,
        });
      } else {
        chart.hideLoading();
      }
    });
  }
}

type LoadingPalette = {
  text: string;
  accent: string;
  mask: string;
};

function resolveLoadingPalette(): LoadingPalette {
  if (typeof window === 'undefined') {
    return {
      text: '#f8fafc',
      accent: '#a78bfa',
      mask: 'rgba(15,23,42,0.72)',
    };
  }

  const styles = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const surface = read('--cq-surface', '#0f172a');
  const text = read('--cq-text-primary', '#f8fafc');
  const accent = read('--cq-accent-3', '#a78bfa');
  const mask = toRgba(surface, 0.72);

  return {
    text,
    accent,
    mask,
  };
}

function toRgba(value: string, alpha: number): string {
  const trimmed = value.trim();
  if (trimmed.startsWith('rgb')) {
    const match = trimmed.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*[\d.]+)?\)/);
    if (match) {
      return `rgba(${match[1]}, ${match[2]}, ${match[3]}, ${alpha})`;
    }
  }

  if (trimmed.startsWith('#')) {
    const hex = trimmed.slice(1);
    const expanded = hex.length === 3
      ? hex.split('').map((entry) => entry + entry).join('')
      : hex;
    if (expanded.length === 6) {
      const r = Number.parseInt(expanded.slice(0, 2), 16);
      const g = Number.parseInt(expanded.slice(2, 4), 16);
      const b = Number.parseInt(expanded.slice(4, 6), 16);
      if (Number.isFinite(r) && Number.isFinite(g) && Number.isFinite(b)) {
        return `rgba(${r}, ${g}, ${b}, ${alpha})`;
      }
    }
  }

  return `rgba(15, 23, 42, ${alpha})`;
}
