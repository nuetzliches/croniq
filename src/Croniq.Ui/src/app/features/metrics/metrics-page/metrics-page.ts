import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { DomSanitizer, SafeResourceUrl } from '@angular/platform-browser';
import { RuntimeConfigService } from '@core/runtime-config.service';

@Component({
    selector: 'cq-metrics-page',
    standalone: true,
    template: `
    <div class="h-full flex flex-col space-y-4">
      <header class="flex items-center justify-between">
        <h1 class="text-lg font-semibold text-primary">System Metrics</h1>
        @if (grafanaUrl()) {
          <a [href]="grafanaUrl()" target="_blank" class="text-sm text-accent hover:underline">
            Open in Grafana ↗
          </a>
        }
      </header>

      <div class="flex-1 overflow-hidden rounded-xl border border-white/10 bg-surface-alt shadow-card relative">
        @if (safeUrl()) {
          <iframe 
            [src]="safeUrl()" 
            class="absolute inset-0 h-full w-full border-0"
            allowfullscreen>
          </iframe>
        } @else {
          <div class="flex h-full flex-col items-center justify-center p-8 text-center text-muted">
             <div class="text-4xl mb-4">📊</div>
             <p class="text-lg font-semibold text-primary">No Metrics Dashboard Configured</p>
             <p class="max-w-md">
               Configure <code>grafanaUrl</code> in <code>croniq-config.json</code> to embed your monitoring dashboard here.
             </p>
          </div>
        }
      </div>
    </div>
  `,
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MetricsPage {
    private readonly config = inject(RuntimeConfigService);
    private readonly sanitizer = inject(DomSanitizer);

    readonly grafanaUrl = computed(() => this.config.snapshot.grafanaUrl);

    readonly safeUrl = computed<SafeResourceUrl | null>(() => {
        const url = this.grafanaUrl();
        if (!url) return null;
        // Append minimal params if needed, e.g. &kiosk
        const embedUrl = url.includes('?') ? `${url}&kiosk` : `${url}?kiosk`;
        return this.sanitizer.bypassSecurityTrustResourceUrl(embedUrl);
    });
}
