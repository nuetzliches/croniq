import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { WebhooksStore } from '@features/webhooks/webhooks.store';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';

@Component({
  selector: 'cq-webhooks-page',
  imports: [],
  providers: [WebhooksStore],
  templateUrl: './webhooks-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhooksPage {
  private readonly store = inject(WebhooksStore);
  private readonly tenantContext = inject(TenantContextService);

  readonly endpoints = this.store.endpoints;
  readonly loading = this.store.loading;
  readonly error = this.store.lastError;

  refresh(): void {
    const tenantId = this.tenantContext.tenantId();
    const environment = this.tenantContext.environment();
    if (tenantId) {
      void this.store.refreshEndpoints({ tenantId, environment });
    }
  }
}
