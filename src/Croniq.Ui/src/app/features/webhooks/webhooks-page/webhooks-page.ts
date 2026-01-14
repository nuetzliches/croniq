import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Dialog } from '@angular/cdk/dialog';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { WebhookDialogComponent } from '@features/webhooks/components/webhook-dialog/webhook-dialog.component';
import { WebhookCapabilitiesView, WebhookEndpointView, WebhooksStore } from '@features/webhooks/webhooks.store';
import { UpsertWebhookEndpointRequest } from '@croniq/api-schema';

type WebhookDialogData = {
  endpoint: UpsertWebhookEndpointRequest | null;
  capabilities: WebhookCapabilitiesView | null;
};

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
  private readonly dialog = inject(Dialog);

  readonly endpoints = this.store.endpoints;
  readonly loading = this.store.loading;
  readonly error = this.store.lastError;

  private createDialogData(endpoint: UpsertWebhookEndpointRequest | null): WebhookDialogData {
    return {
      endpoint,
      capabilities: this.store.capabilities(),
    };
  }

  refresh(): void {
    const tenantId = this.tenantContext.tenantId();
    const environment = this.tenantContext.environment();
    if (tenantId) {
      void this.store.refreshEndpoints({ tenantId, environment });
    }
  }

  openWebhookDialog(endpoint?: WebhookEndpointView): void {
    const data: UpsertWebhookEndpointRequest | null = endpoint
      ? {
          hookKey: endpoint.hookKey,
          jobKey: endpoint.jobKey,
          enabled: endpoint.status === 'active',
          requireSignature: endpoint.requireSignature,
          allowUnsigned: !endpoint.requireSignature,
          requestsPerMinute: endpoint.requestsPerMinute ?? null,
          metadata: {},
        }
      : null;

    const ref = this.dialog.open<UpsertWebhookEndpointRequest>(WebhookDialogComponent, {
      data: this.createDialogData(data),
      width: '500px',
      panelClass: 'bg-transparent',
    });

    ref.closed.subscribe((result) => {
      if (result) {
        const tenantId = this.tenantContext.tenantId();
        const environment = this.tenantContext.environment();
        if (tenantId) {
          this.store.upsertEndpoint(
            {
              tenantId,
              environment,
            },
            result,
          );
        }
      }
    });
  }

  ipWhitelistLabel(endpoint: WebhookEndpointView): string {
    const count = endpoint.ipRuleCount;
    if (count === null || count === undefined) {
      return 'IP Whitelist: Unavailable';
    }
    if (count === 0) {
      return 'IP Whitelist: None';
    }
    return `IP Whitelist: ${count} rule${count === 1 ? '' : 's'}`;
  }
}
