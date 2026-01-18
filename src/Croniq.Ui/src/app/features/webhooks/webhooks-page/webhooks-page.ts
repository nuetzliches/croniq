import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, computed, inject, linkedSignal, signal } from '@angular/core';
import { CdkMenu } from '@angular/cdk/menu';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { WebhookDialogComponent } from '@features/webhooks/components/webhook-dialog/webhook-dialog.component';
import { WebhookIpRulesDialogComponent } from '@features/webhooks/components/webhook-ip-rules-dialog/webhook-ip-rules-dialog.component';
import { WebhookRotateSecretDialogComponent } from '@features/webhooks/components/webhook-rotate-secret-dialog/webhook-rotate-secret-dialog.component';
import { WebhookCapabilitiesView, WebhookEndpointView, WebhooksStore } from '@features/webhooks/webhooks.store';
import { RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { Field, form } from '@angular/forms/signals';
import { CqCellDefDirective, CqColumnComponent, CqConfirmDialogComponent, CqConfirmDialogData, CqContextMenuItemDirective, CqDialogService, CqFormFieldComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';
import { filter } from 'rxjs';

type WebhookDialogData = {
  endpoint: UpsertWebhookEndpointRequest | null;
  capabilities: WebhookCapabilitiesView | null;
};

type WebhookStatusFilter = 'all' | WebhookEndpointView['status'];

type WebhookFilterModel = {
  hookKey: string;
  jobKey: string;
  status: WebhookStatusFilter;
  environment: string;
};

type OptionEntry = {
  value: string;
  label: string;
};

const ALL_ENVIRONMENTS = 'all';

const STATUS_OPTIONS: ReadonlyArray<{ value: WebhookStatusFilter; label: string }> = [
  { value: 'all', label: 'All statuses' },
  { value: 'active', label: 'Active' },
  { value: 'paused', label: 'Paused' },
  { value: 'degraded', label: 'Degraded' },
];

type PageInfo = {
  total: number;
  pageIndex: number;
  pageCount: number;
  start: number;
  end: number;
};

const EMPTY_PAGE_INFO: PageInfo = {
  total: 0,
  pageIndex: 0,
  pageCount: 0,
  start: 0,
  end: 0,
};

@Directive({
  selector: '[cqWebhookCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqWebhookCellDirective }],
})
export class CqWebhookCellDirective extends CqCellDefDirective<WebhookEndpointView> {
  // Inherits ngTemplateContextGuard from base class
}

@Component({
  selector: 'cq-webhooks-page',
  imports: [
    DatePipe,
    Field,
    DataGrid,
    CqColumnComponent,
    CqWebhookCellDirective,
    CqFormFieldComponent,
    CqInputDirective,
    CqSelectDirective,
    CqContextMenuItemDirective,
    CdkMenu,
  ],
  providers: [WebhooksStore],
  templateUrl: './webhooks-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhooksPage {
  private readonly store = inject(WebhooksStore);
  private readonly tenantContext = inject(TenantContextService);
  private readonly runtimeConfig = inject(RuntimeConfigService);
  private readonly dialog = inject(CqDialogService);

  readonly endpoints = this.store.endpoints;
  readonly loading = this.store.loading;
  readonly error = this.store.lastError;
  readonly readPermissionDenied = this.store.readPermissionDenied;
  readonly writePermissionDenied = this.store.writePermissionDenied;
  readonly rotatedSecret = this.store.rotatedSecret;
  readonly filterModel = signal(createDefaultFilters());
  readonly filterForm = form(this.filterModel, () => { });
  readonly statusOptions = STATUS_OPTIONS;

  private readonly filterSignature = computed(() => {
    const model = this.filterModel();
    return `${model.hookKey}|${model.jobKey}|${model.status}|${model.environment}`;
  });

  readonly pageSize = signal(25);
  readonly pageIndex = linkedSignal(() => {
    this.filterSignature();
    return 0;
  });

  readonly environmentOptions = computed<ReadonlyArray<OptionEntry>>(() => {
    const entries = new Set<string>();
    this.endpoints().forEach((endpoint) => {
      if (endpoint.environment) {
        entries.add(endpoint.environment);
      }
    });
    const sorted = Array.from(entries).sort();
    return [{ value: ALL_ENVIRONMENTS, label: 'All environments' }].concat(
      sorted.map((value) => ({ value, label: value })),
    );
  });

  readonly filteredEndpoints = computed(() => {
    const filters = this.filterModel();
    const hookFilter = filters.hookKey.trim().toLowerCase();
    const jobFilter = filters.jobKey.trim().toLowerCase();
    const statusFilter = filters.status === 'all' ? '' : filters.status;
    const environmentFilter = filters.environment === ALL_ENVIRONMENTS ? '' : filters.environment;

    return this.endpoints().filter((endpoint) => {
      if (hookFilter && !endpoint.hookKey.toLowerCase().includes(hookFilter)) {
        return false;
      }
      if (jobFilter && !endpoint.jobKey.toLowerCase().includes(jobFilter)) {
        return false;
      }
      if (statusFilter && endpoint.status !== statusFilter) {
        return false;
      }
      if (environmentFilter && endpoint.environment !== environmentFilter) {
        return false;
      }
      return true;
    });
  });

  readonly pageInfo = computed(() =>
    buildPageInfo(this.filteredEndpoints().length, this.pageIndex(), this.pageSize()),
  );

  readonly pagedEndpoints = computed(() => {
    const info = this.pageInfo();
    if (info.total === 0) {
      return [];
    }
    const startIndex = info.pageIndex * this.pageSize();
    return this.filteredEndpoints().slice(startIndex, startIndex + this.pageSize());
  });

  readonly pageSummary = computed(() => {
    const info = this.pageInfo();
    if (info.total === 0) {
      return '0 results';
    }
    return `Showing ${info.start}-${info.end} of ${info.total}`;
  });

  readonly pageLabel = computed(() => {
    const info = this.pageInfo();
    if (info.total === 0) {
      return 'Page 0 of 0';
    }
    return `Page ${info.pageIndex + 1} of ${info.pageCount}`;
  });

  readonly isFirstPage = computed(() => this.pageInfo().pageIndex <= 0);
  readonly isLastPage = computed(() => {
    const info = this.pageInfo();
    return info.pageCount === 0 || info.pageIndex >= info.pageCount - 1;
  });

  readonly filtersActive = computed(() => {
    const model = this.filterModel();
    return (
      model.hookKey.trim().length > 0
      || model.jobKey.trim().length > 0
      || model.status !== 'all'
      || model.environment !== ALL_ENVIRONMENTS
    );
  });

  webhookRowKey = (row: WebhookEndpointView, index: number) =>
    `${row.environment}:${row.hookKey || `webhook-${index}`}`;

  readonly selectedRowKey = signal<string | number | null>(null);

  readonly selectedEndpoint = computed(() => {
    const key = this.selectedRowKey();
    if (!key) {
      return null;
    }
    return this.endpoints().find((endpoint) => this.webhookRowKey(endpoint, 0) === key) ?? null;
  });

  readonly ingressUrl = computed(() => {
    const endpoint = this.selectedEndpoint();
    const tenantId = this.tenantContext.tenantId();
    const baseUrl = this.runtimeConfig.apiBaseUrl;
    if (!endpoint || !tenantId || !baseUrl) {
      return null;
    }
    try {
      return new URL(`/tenants/${tenantId}/webhooks/${encodeURIComponent(endpoint.hookKey)}`, baseUrl).toString();
    } catch {
      return null;
    }
  });

  webhookRowClasses = (row: WebhookEndpointView) =>
    row.status === 'active' ? undefined : ['opacity-80'];

  selectRow(event: { row: WebhookEndpointView }): void {
    const row = event.row;
    if (!row) {
      return;
    }
    this.selectedRowKey.set(this.webhookRowKey(row, 0));
  }

  copyIngressUrl(): void {
    const url = this.ingressUrl();
    if (!url) {
      return;
    }
    if (!navigator.clipboard?.writeText) {
      console.error('Clipboard API unavailable for webhook ingress URL copy.');
      return;
    }
    navigator.clipboard.writeText(url).catch((error: unknown) => {
      console.error('Unable to copy webhook ingress URL', error);
    });
  }

  readonly selectedIpRulesEndpoint = signal<WebhookEndpointView | null>(null);

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
    if (this.writePermissionDenied()) {
      return;
    }
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
      restoreFocus: true,
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

  openIpRulesDialog(endpoint: WebhookEndpointView): void {
    if (this.writePermissionDenied()) {
      return;
    }
    this.selectedIpRulesEndpoint.set(endpoint);
    this.dialog.open(WebhookIpRulesDialogComponent, {
      data: { endpoint },
      width: '560px',
      panelClass: 'bg-transparent',
      restoreFocus: true,
    }).closed.subscribe(() => this.selectedIpRulesEndpoint.set(null));
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

  statusLabel(status: WebhookEndpointView['status']): string {
    if (status === 'active') {
      return 'Active';
    }
    if (status === 'degraded') {
      return 'Degraded';
    }
    return 'Paused';
  }

  requestsPerMinuteLabel(endpoint: WebhookEndpointView): string {
    return typeof endpoint.requestsPerMinute === 'number'
      ? String(endpoint.requestsPerMinute)
      : 'Default';
  }

  resetFilters(): void {
    this.filterModel.set(createDefaultFilters());
  }

  previousPage(): void {
    if (this.isFirstPage()) {
      return;
    }
    this.pageIndex.update((value) => Math.max(0, value - 1));
  }

  nextPage(): void {
    if (this.isLastPage()) {
      return;
    }
    this.pageIndex.update((value) => value + 1);
  }

  dismissRotatedSecret(): void {
    this.store.clearRotatedSecret();
  }

  copyRotatedSecret(): void {
    const secret = this.rotatedSecret();
    if (!secret) {
      return;
    }
    if (!navigator.clipboard?.writeText) {
      console.error('Clipboard API unavailable for webhook secret copy.');
      return;
    }
    navigator.clipboard.writeText(secret).catch((error: unknown) => {
      console.error('Unable to copy webhook secret', error);
    });
  }

  rotateSecret(endpoint: WebhookEndpointView): void {
    if (this.writePermissionDenied()) {
      return;
    }
    this.dialog
      .open<RotateWebhookSecretRequest>(WebhookRotateSecretDialogComponent, {
        data: { hookKey: endpoint.hookKey },
        width: '420px',
        panelClass: 'bg-transparent',
      })
      .closed.pipe(filter((payload): payload is RotateWebhookSecretRequest => !!payload))
      .subscribe((payload) => {
        const tenantId = this.tenantContext.tenantId();
        const environment = this.tenantContext.environment();
        if (!tenantId) {
          return;
        }
        this.store.rotateSecret(
          {
            tenantId,
            environment,
            hookKey: endpoint.hookKey,
          },
          payload,
        );
      });
  }

  deleteEndpoint(endpoint: WebhookEndpointView): void {
    if (this.writePermissionDenied()) {
      return;
    }
    this.dialog.open<boolean>(CqConfirmDialogComponent, {
      data: {
        title: 'Delete webhook',
        message: `Delete webhook ${endpoint.hookKey}?`,
        confirmLabel: 'Delete',
        variant: 'danger',
      } satisfies CqConfirmDialogData,
      width: '420px',
      panelClass: 'bg-transparent',
      restoreFocus: true,
    }).closed.pipe(filter(Boolean)).subscribe(() => {
      const tenantId = this.tenantContext.tenantId();
      const environment = this.tenantContext.environment();
      if (!tenantId) {
        return;
      }
      this.store.deleteEndpoint({
        tenantId,
        environment,
        hookKey: endpoint.hookKey,
      });
    });
  }

  enableEndpoint(endpoint: WebhookEndpointView): void {
    if (this.writePermissionDenied()) {
      return;
    }
    const tenantId = this.tenantContext.tenantId();
    const environment = this.tenantContext.environment();
    if (!tenantId) {
      return;
    }
    this.store.setEndpointEnabled({ tenantId, environment }, endpoint, true);
  }

  disableEndpoint(endpoint: WebhookEndpointView): void {
    if (this.writePermissionDenied()) {
      return;
    }
    this.dialog.open<boolean>(CqConfirmDialogComponent, {
      data: {
        title: 'Disable webhook',
        message: `Disable webhook ${endpoint.hookKey}?`,
        confirmLabel: 'Disable',
        variant: 'danger',
      } satisfies CqConfirmDialogData,
      width: '420px',
      panelClass: 'bg-transparent',
      restoreFocus: true,
    }).closed.pipe(filter(Boolean)).subscribe(() => {
      const tenantId = this.tenantContext.tenantId();
      const environment = this.tenantContext.environment();
      if (!tenantId) {
        return;
      }
      this.store.setEndpointEnabled({ tenantId, environment }, endpoint, false);
    });
  }
}

function createDefaultFilters(): WebhookFilterModel {
  return {
    hookKey: '',
    jobKey: '',
    status: 'all',
    environment: ALL_ENVIRONMENTS,
  };
}

function buildPageInfo(total: number, pageIndex: number, pageSize: number): PageInfo {
  if (total <= 0 || pageSize <= 0) {
    return EMPTY_PAGE_INFO;
  }

  const pageCount = Math.ceil(total / pageSize);
  const safeIndex = Math.min(Math.max(pageIndex, 0), pageCount - 1);
  const start = safeIndex * pageSize + 1;
  const end = Math.min(total, start + pageSize - 1);

  return {
    total,
    pageIndex: safeIndex,
    pageCount,
    start,
    end,
  };
}
