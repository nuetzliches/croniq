import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, linkedSignal, signal, viewChild } from '@angular/core';
import { CdkMenu } from '@angular/cdk/menu';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { WebhookDialogComponent } from '@features/webhooks/components/webhook-dialog/webhook-dialog.component';
import { WebhookIpRulesDialogComponent } from '@features/webhooks/components/webhook-ip-rules-dialog/webhook-ip-rules-dialog.component';
import { WebhookRotateSecretDialogComponent } from '@features/webhooks/components/webhook-rotate-secret-dialog/webhook-rotate-secret-dialog.component';
import { WebhookCapabilitiesView, WebhookDeadLetterView, WebhookEndpointView, WebhooksStore } from '@features/webhooks/webhooks.store';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { FormField, form } from '@angular/forms/signals';
import { CqCellDefDirective, CqColumnComponent, CqConfirmDialogComponent, CqConfirmDialogData, CqContextMenuItemDirective, CqDialogService, CqFormFieldComponent, CqIconComponent, CqInputDirective, CqSelectDirective, CqTextareaDirective, DataGrid } from 'ui-kit';
import { filter } from 'rxjs';

type WebhookDialogData = {
  endpoint: UpsertWebhookEndpointRequest | null;
  capabilities: WebhookCapabilitiesView | null;
};

type WebhookStatusFilter = 'all' | WebhookEndpointView['status'];

type WebhookFilterModel = {
  status: WebhookStatusFilter;
  environment: string;
};

type OptionEntry = {
  value: string;
  label: string;
};

type TimelineItemKind = 'delivery' | 'deadLetter';

type WebhookTimelineItemView = {
  id: string;
  kind: TimelineItemKind;
  status: 'success' | 'failed' | 'warning';
  label: string;
  occurredAt: string;
  hookKey: string;
  jobKey?: string;
  environment?: string;
  endpointStatus?: WebhookEndpointView['status'];
  reason?: string;
  deadLetterId?: string;
  endpointRowKey?: string;
};

type HookFilterEntry = {
  hookKey: string;
  status: WebhookEndpointView['status'];
  deadLetterCount: number;
};

type JobFilterEntry = {
  jobKey: string;
  status: WebhookEndpointView['status'];
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
    FormField,
    DataGrid,
    CqColumnComponent,
    CqWebhookCellDirective,
    CqFormFieldComponent,
    CqIconComponent,
    CqInputDirective,
    CqSelectDirective,
    CqTextareaDirective,
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
  private readonly shellPanel = inject(ShellPanelService);
  private readonly panelTemplate = viewChild<TemplateRef<unknown>>('webhooksFilterPanel');
  private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('webhooksFilterCollapsed');

  readonly endpoints = this.store.endpoints;
  readonly loading = this.store.loading;
  readonly error = this.store.lastError;
  readonly readPermissionDenied = this.store.readPermissionDenied;
  readonly writePermissionDenied = this.store.writePermissionDenied;
  readonly rotatedSecret = this.store.rotatedSecret;
  readonly deadLetters = this.store.deadLetters;
  readonly filterModel = signal(createDefaultFilters());
  readonly filterForm = form(this.filterModel, () => { });
  readonly statusOptions = STATUS_OPTIONS;
  readonly invokePayload = signal('');
  readonly invokePayloadTouched = signal(false);
  readonly invokeNotice = signal<string | null>(null);
  readonly hookSearch = signal('');
  readonly jobSearch = signal('');
  readonly selectedHookKeys = signal<ReadonlyArray<string>>([]);
  readonly selectedJobKeys = signal<ReadonlyArray<string>>([]);
  readonly timelineFromIso = signal<string | null>(null);
  readonly timelineToIso = signal<string | null>(null);
  readonly kpiSuccessRate = computed(() => {
    const endpoints = this.endpoints().length;
    if (endpoints === 0) {
      return 'N/A';
    }
    const deadLetters = this.deadLetters().length;
    const failureRatio = Math.min(1, deadLetters / endpoints);
    const successRate = Math.max(0, Math.round((1 - failureRatio) * 100));
    return `${successRate}%`;
  });
  readonly kpiLatency = computed(() => 'N/A');
  readonly kpiRateLimitRejections = computed(() => 'N/A');
  readonly kpiDeadLetters = computed(() => this.deadLetters().length);

  private readonly filterSignature = computed(() => {
    const model = this.filterModel();
    const hooks = [...this.selectedHookKeys()].sort().join(',');
    const jobs = [...this.selectedJobKeys()].sort().join(',');
    const fromIso = this.timelineFromIso() ?? '';
    const toIso = this.timelineToIso() ?? '';
    return `${model.status}|${model.environment}|${hooks}|${jobs}|${fromIso}|${toIso}`;
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
    const statusFilter = filters.status === 'all' ? '' : filters.status;
    const environmentFilter = filters.environment === ALL_ENVIRONMENTS ? '' : filters.environment;
    const selectedHooks = new Set(this.selectedHookKeys());
    const selectedJobs = new Set(this.selectedJobKeys());

    return this.endpoints().filter((endpoint) => {
      if (statusFilter && endpoint.status !== statusFilter) {
        return false;
      }
      if (environmentFilter && endpoint.environment !== environmentFilter) {
        return false;
      }
      if (selectedHooks.size > 0 && !selectedHooks.has(endpoint.hookKey)) {
        return false;
      }
      if (selectedJobs.size > 0 && !selectedJobs.has(endpoint.jobKey)) {
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
      this.selectedHookKeys().length > 0
      || this.selectedJobKeys().length > 0
      || model.status !== 'all'
      || model.environment !== ALL_ENVIRONMENTS
      || !!this.timelineFromIso()
      || !!this.timelineToIso()
    );
  });

  readonly timelineRangeActive = computed(() => !!this.timelineFromIso() || !!this.timelineToIso());

  readonly timelineFromLocal = computed(() => isoToLocalDateTimeInput(this.timelineFromIso()));
  readonly timelineToLocal = computed(() => isoToLocalDateTimeInput(this.timelineToIso()));

  readonly hookEntries = computed<ReadonlyArray<HookFilterEntry>>(() => {
    const entries = new Map<string, WebhookEndpointView[]>();
    this.endpoints().forEach((endpoint) => {
      const existing = entries.get(endpoint.hookKey);
      if (existing) {
        existing.push(endpoint);
      } else {
        entries.set(endpoint.hookKey, [endpoint]);
      }
    });

    const deadLetterCounts = new Map<string, number>();
    this.deadLetters().forEach((entry) => {
      deadLetterCounts.set(entry.hookKey, (deadLetterCounts.get(entry.hookKey) ?? 0) + 1);
    });

    return Array.from(entries.entries())
      .map(([hookKey, list]) => ({
        hookKey,
        status: deriveStatus(list),
        deadLetterCount: deadLetterCounts.get(hookKey) ?? 0,
      }))
      .sort((a, b) => a.hookKey.localeCompare(b.hookKey));
  });

  readonly visibleHookEntries = computed(() => {
    const term = this.hookSearch().trim().toLowerCase();
    if (!term) {
      return this.hookEntries();
    }
    return this.hookEntries().filter((entry) => entry.hookKey.toLowerCase().includes(term));
  });

  readonly jobEntries = computed<ReadonlyArray<JobFilterEntry>>(() => {
    const entries = new Map<string, WebhookEndpointView[]>();
    this.endpoints().forEach((endpoint) => {
      const existing = entries.get(endpoint.jobKey);
      if (existing) {
        existing.push(endpoint);
      } else {
        entries.set(endpoint.jobKey, [endpoint]);
      }
    });

    return Array.from(entries.entries())
      .map(([jobKey, list]) => ({
        jobKey,
        status: deriveStatus(list),
      }))
      .sort((a, b) => a.jobKey.localeCompare(b.jobKey));
  });

  readonly visibleJobEntries = computed(() => {
    const term = this.jobSearch().trim().toLowerCase();
    if (!term) {
      return this.jobEntries();
    }
    return this.jobEntries().filter((entry) => entry.jobKey.toLowerCase().includes(term));
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

  isEndpointPaused(endpoint: WebhookEndpointView): boolean {
    const current = this.endpoints().find((entry) =>
      entry.hookKey === endpoint.hookKey
      && entry.environment === endpoint.environment,
    );
    const status = current?.status ?? endpoint.status;
    return status === 'paused';
  }

  resetFilters(): void {
    this.filterModel.set(createDefaultFilters());
    this.selectedHookKeys.set([]);
    this.selectedJobKeys.set([]);
    this.hookSearch.set('');
    this.jobSearch.set('');
    this.timelineFromIso.set(null);
    this.timelineToIso.set(null);
  }

  clearTimelineRange(): void {
    this.timelineFromIso.set(null);
    this.timelineToIso.set(null);
  }

  setTimelineFromLocal(value: string): void {
    this.timelineFromIso.set(localDateTimeInputToIso(value));
  }

  setTimelineToLocal(value: string): void {
    this.timelineToIso.set(localDateTimeInputToIso(value));
  }

  setTimelinePreset(kind: '24h' | '7d' | '30d'): void {
    const now = Date.now();
    const deltaMs = kind === '24h'
      ? 24 * 60 * 60 * 1000
      : kind === '7d'
        ? 7 * 24 * 60 * 60 * 1000
        : 30 * 24 * 60 * 60 * 1000;

    this.timelineToIso.set(new Date(now).toISOString());
    this.timelineFromIso.set(new Date(now - deltaMs).toISOString());
  }

  setHookSearch(value: string): void {
    this.hookSearch.set(value);
  }

  setJobSearch(value: string): void {
    this.jobSearch.set(value);
  }

  isHookSelected(hookKey: string): boolean {
    return this.selectedHookKeys().includes(hookKey);
  }

  isJobSelected(jobKey: string): boolean {
    return this.selectedJobKeys().includes(jobKey);
  }

  toggleHookSelection(hookKey: string, checked: boolean): void {
    this.selectedHookKeys.update((current) =>
      checked
        ? Array.from(new Set([...current, hookKey]))
        : current.filter((entry) => entry !== hookKey),
    );
  }

  toggleJobSelection(jobKey: string, checked: boolean): void {
    this.selectedJobKeys.update((current) =>
      checked
        ? Array.from(new Set([...current, jobKey]))
        : current.filter((entry) => entry !== jobKey),
    );
  }

  readonly filteredDeadLetters = computed(() => {
    const selectedHooks = new Set(this.selectedHookKeys());
    const selectedJobs = new Set(this.selectedJobKeys());
    return this.deadLetters().filter((entry) => {
      if (selectedHooks.size > 0 && !selectedHooks.has(entry.hookKey)) {
        return false;
      }
      if (selectedJobs.size > 0 && entry.jobKey && !selectedJobs.has(entry.jobKey)) {
        return false;
      }
      if (selectedJobs.size > 0 && !entry.jobKey) {
        return false;
      }
      return true;
    });
  });

  readonly selectedDeadLetterId = signal<string | null>(null);

  readonly selectedDeadLetter = computed(() => {
    const id = this.selectedDeadLetterId();
    if (!id) {
      return null;
    }
    return this.deadLetters().find((entry) => entry.id === id) ?? null;
  });

  readonly timelineItems = computed<ReadonlyArray<WebhookTimelineItemView>>(() => {
    const filters = this.filterModel();
    const restrictToEndpointSet = filters.status !== 'all' || filters.environment !== ALL_ENVIRONMENTS;
    const endpoints = this.filteredEndpoints();
    const deadLetters = this.filteredDeadLetters();

    const fromMs = tryParseIsoToMs(this.timelineFromIso());
    const toMs = tryParseIsoToMs(this.timelineToIso());

    const endpointKeys = new Set(endpoints.map((endpoint) => `${endpoint.hookKey}|${endpoint.jobKey}`));
    const timeline: WebhookTimelineItemView[] = [];

    endpoints.forEach((endpoint) => {
      if (!endpoint.lastDeliveryAt) {
        return;
      }
      timeline.push({
        id: `delivery:${this.webhookRowKey(endpoint, 0)}`,
        kind: 'delivery',
        status: endpoint.status === 'degraded' ? 'warning' : 'success',
        label: 'Last delivery',
        occurredAt: endpoint.lastDeliveryAt,
        hookKey: endpoint.hookKey,
        jobKey: endpoint.jobKey,
        environment: endpoint.environment,
        endpointStatus: endpoint.status,
        reason: endpoint.status === 'degraded' ? 'Endpoint reported degraded status.' : undefined,
        endpointRowKey: this.webhookRowKey(endpoint, 0),
      });
    });

    deadLetters
      .filter((entry) => {
        if (endpointKeys.size === 0) {
          return !restrictToEndpointSet;
        }
        if (entry.jobKey) {
          return endpointKeys.has(`${entry.hookKey}|${entry.jobKey}`);
        }
        return endpoints.some((endpoint) => endpoint.hookKey === entry.hookKey);
      })
      .forEach((entry) => {
        timeline.push({
          id: `deadletter:${entry.id}`,
          kind: 'deadLetter',
          status: 'failed',
          label: 'Dead letter',
          occurredAt: entry.occurredAt,
          hookKey: entry.hookKey,
          jobKey: entry.jobKey,
          reason: entry.reason ?? 'No reason provided.',
          deadLetterId: entry.id,
        });
      });

    return timeline
      .filter((entry) => {
        const occurredAtMs = Date.parse(entry.occurredAt);
        if (!Number.isFinite(occurredAtMs)) {
          return true;
        }
        if (typeof fromMs === 'number' && occurredAtMs < fromMs) {
          return false;
        }
        if (typeof toMs === 'number' && occurredAtMs > toMs) {
          return false;
        }
        return true;
      })
      .sort((a, b) => Date.parse(b.occurredAt) - Date.parse(a.occurredAt));
  });

  readonly selectedTimelineItemId = signal<string | null>(null);

  readonly selectedTimelineItem = computed(() => {
    const id = this.selectedTimelineItemId();
    if (!id) {
      return null;
    }
    return this.timelineItems().find((entry) => entry.id === id) ?? null;
  });

  constructor() {
    effect((onCleanup) => {
      const template = this.panelTemplate();
      const collapsedTemplate = this.collapsedTemplate();
      if (!template) {
        return;
      }
      this.shellPanel.setPanel(
        template,
        'Filters & settings',
        'Refine the endpoints list.',
        collapsedTemplate ?? null,
      );
      onCleanup(() => this.shellPanel.clearPanel(template));
    });

    effect(() => {
      const endpoint = this.selectedEndpoint();
      if (!endpoint) {
        this.invokePayload.set('');
        this.invokePayloadTouched.set(false);
        return;
      }
      if (this.invokePayloadTouched()) {
        return;
      }
      this.invokePayload.set(createDefaultInvokePayload(endpoint));
    });

    effect(() => {
      const currentId = this.selectedTimelineItemId();
      if (!currentId) {
        return;
      }
      if (!this.timelineItems().some((entry) => entry.id === currentId)) {
        this.selectedTimelineItemId.set(null);
      }
    });
  }

  selectDeadLetter(entry: WebhookDeadLetterView): void {
    this.selectedDeadLetterId.set(entry.id);
  }

  selectTimelineItem(entry: WebhookTimelineItemView): void {
    this.selectedTimelineItemId.set(entry.id);

    if (entry.kind === 'deadLetter' && entry.deadLetterId) {
      this.selectedDeadLetterId.set(entry.deadLetterId);

      const match = this.filteredEndpoints().find((endpoint) =>
        endpoint.hookKey === entry.hookKey
        && (!entry.jobKey || endpoint.jobKey === entry.jobKey),
      );
      if (match) {
        this.selectedRowKey.set(this.webhookRowKey(match, 0));
      }
      return;
    }

    if (entry.kind === 'delivery' && entry.endpointRowKey) {
      this.selectedRowKey.set(entry.endpointRowKey);
    }
  }


  setInvokePayload(value: string): void {
    this.invokePayloadTouched.set(true);
    this.invokePayload.set(value);
  }

  copyInvokePayload(): void {
    const payload = this.invokePayload();
    if (!payload || !navigator.clipboard?.writeText) {
      return;
    }
    navigator.clipboard.writeText(payload).catch((error: unknown) => {
      console.error('Unable to copy webhook payload', error);
    });
  }

  copyCurlSnippet(): void {
    const url = this.ingressUrl();
    const payload = this.invokePayload();
    if (!url || !navigator.clipboard?.writeText) {
      return;
    }
    const escapedPayload = escapeSingleQuotes(payload || '{}');
    const curl = `curl -X POST "${url}" -H "Content-Type: application/json" -d '${escapedPayload}'`;
    navigator.clipboard.writeText(curl).catch((error: unknown) => {
      console.error('Unable to copy cURL snippet', error);
    });
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

  replayDeadLetter(entry: WebhookDeadLetterView): void {
    if (this.writePermissionDenied()) {
      return;
    }
    const deadLetterId = Number(entry.id);
    if (!Number.isFinite(deadLetterId)) {
      return;
    }
    this.dialog.open<boolean>(CqConfirmDialogComponent, {
      data: {
        title: 'Replay dead letter',
        message: `Replay dead letter ${entry.id}?`,
        confirmLabel: 'Replay',
      } satisfies CqConfirmDialogData,
      width: '420px',
      panelClass: 'bg-transparent',
    }).closed.pipe(filter(Boolean)).subscribe(() => {
      const tenantId = this.tenantContext.tenantId();
      const environment = this.tenantContext.environment();
      if (!tenantId) {
        return;
      }
      this.store.replayDeadLetter({ tenantId, environment, deadLetterId });
    });
  }

  timelineStatusClass(entry: { status: 'success' | 'failed' | 'warning' }): string {
    if (entry.status === 'success') {
      return 'text-success';
    }
    if (entry.status === 'warning') {
      return 'text-warning';
    }
    return 'text-danger';
  }

  timelineStatusGlyph(entry: { status: 'success' | 'failed' | 'warning' }): string {
    return entry.status === 'success' ? '●' : entry.status === 'warning' ? '▲' : '■';
  }

  invokeWebhook(): void {
    if (this.writePermissionDenied()) {
      return;
    }
    const endpoint = this.selectedEndpoint();
    if (!endpoint) {
      return;
    }
    if (endpoint.status === 'paused') {
      this.invokeNotice.set('Webhook is disabled. Enable it before invoking.');
      setTimeout(() => this.invokeNotice.set(null), 3500);
      return;
    }
    this.store.invokeWebhook({ hookKey: endpoint.hookKey });
    this.invokeNotice.set('Invocation requested.');
    setTimeout(() => this.invokeNotice.set(null), 2500);
  }
}

function tryParseIsoToMs(value: string | null): number | null {
  if (!value) {
    return null;
  }
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

function isoToLocalDateTimeInput(value: string | null): string {
  if (!value) {
    return '';
  }
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) {
    return '';
  }
  const pad = (entry: number) => String(entry).padStart(2, '0');
  const yyyy = date.getFullYear();
  const mm = pad(date.getMonth() + 1);
  const dd = pad(date.getDate());
  const hh = pad(date.getHours());
  const min = pad(date.getMinutes());
  return `${yyyy}-${mm}-${dd}T${hh}:${min}`;
}

function localDateTimeInputToIso(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  const date = new Date(trimmed);
  if (!Number.isFinite(date.getTime())) {
    return null;
  }
  return date.toISOString();
}

function createDefaultFilters(): WebhookFilterModel {
  return {
    status: 'all',
    environment: ALL_ENVIRONMENTS,
  };
}

function deriveStatus(entries: ReadonlyArray<WebhookEndpointView>): WebhookEndpointView['status'] {
  if (entries.some((entry) => entry.status === 'degraded')) {
    return 'degraded';
  }
  if (entries.some((entry) => entry.status === 'paused')) {
    return 'paused';
  }
  return 'active';
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

function createDefaultInvokePayload(endpoint: WebhookEndpointView): string {
  return JSON.stringify(
    {
      event: 'test',
      hookKey: endpoint.hookKey,
      jobKey: endpoint.jobKey,
      environment: endpoint.environment,
      timestamp: new Date().toISOString(),
    },
    null,
    2,
  );
}

function escapeSingleQuotes(value: string): string {
  return value.replace(/'/g, `'"'"'`);
}
