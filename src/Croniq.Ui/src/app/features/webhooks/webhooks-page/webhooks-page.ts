import { CdkMenu } from '@angular/cdk/menu';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, linkedSignal, signal, viewChild } from '@angular/core';
import { FormField, form } from '@angular/forms/signals';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { WebhookDialogComponent } from '@features/webhooks/components/webhook-dialog/webhook-dialog.component';
import { WebhookIpRulesDialogComponent } from '@features/webhooks/components/webhook-ip-rules-dialog/webhook-ip-rules-dialog.component';
import { WebhookRotateSecretDialogComponent } from '@features/webhooks/components/webhook-rotate-secret-dialog/webhook-rotate-secret-dialog.component';
import { ActivityBucket, ActivityConnectionState, WebhookActivityQuery, WebhookCapabilitiesView, WebhookDeadLetterView, WebhookEndpointView, WebhookRemoteHealthView, WebhookTimelineItemView, WebhooksStore } from '@features/webhooks/webhooks.store';
import { CqEchartsChartComponent } from '@shared/charts/echarts-chart/echarts-chart';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import type { SeriesOption } from 'echarts';
import type { EChartsCoreOption } from 'echarts/core';
import { filter } from 'rxjs';
import { CqCellDefDirective, CqColumnComponent, CqConfirmDialogComponent, CqConfirmDialogData, CqContextMenuItemDirective, CqDialogService, CqFormFieldComponent, CqIconComponent, CqInputDirective, CqSelectDirective, CqTextareaDirective, CqToggleDirective, DataGrid } from 'ui-kit';

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

const TIMELINE_PRESET_LOOKBACK_MS = {
  '5m': 5 * 60 * 1000,
  '30m': 30 * 60 * 1000,
  '3h': 3 * 60 * 60 * 1000,
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  // '21d': 21 * 24 * 60 * 60 * 1000,
} as const;

type TimelinePresetKey = keyof typeof TIMELINE_PRESET_LOOKBACK_MS;

const TIMELINE_PRESET_OPTIONS: ReadonlyArray<{ key: TimelinePresetKey; label: string }> = [
  { key: '5m', label: '5m' },
  { key: '30m', label: '30m' },
  { key: '3h', label: '3h' },
  { key: '24h', label: '24h' },
  { key: '7d', label: '7d' },
  // { key: '21d', label: '21d' },
];

const DEFAULT_TIMELINE_PRESET: TimelinePresetKey = '24h';
const DEFAULT_LOOKBACK_MS = TIMELINE_PRESET_LOOKBACK_MS[DEFAULT_TIMELINE_PRESET];

const TIMELINE_BUCKET_MS_OPTIONS = [
  60 * 1000,
  2 * 60 * 1000,
  5 * 60 * 1000,
  10 * 60 * 1000,
  15 * 60 * 1000,
  30 * 60 * 1000,
  60 * 60 * 1000,
  3 * 60 * 60 * 1000,
  6 * 60 * 60 * 1000,
  12 * 60 * 60 * 1000,
  24 * 60 * 60 * 1000,
] as const;
const MAX_TIMELINE_BUCKETS = 24;

const ACTIVITY_STATUS_DEFINITIONS: ReadonlyArray<{ status: WebhookTimelineItemView['status']; label: string }> = [
  { status: 'pending', label: 'Pending' },
  { status: 'leased', label: 'Leased' },
  { status: 'success', label: 'Success' },
  { status: 'warning', label: 'Warning' },
  { status: 'failed', label: 'Failed' },
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
    CqToggleDirective,
    CqContextMenuItemDirective,
    CdkMenu,
    CqEchartsChartComponent,
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
  readonly invokeLoading = this.store.invokeLoading;
  readonly deadLetters = this.store.deadLetters;
  readonly activityTimeline = this.store.activityTimeline;
  readonly activityLoading = this.store.activityLoading;
  readonly activityBackendReady = this.store.activityBackendReady;
  readonly activityError = this.store.activityError;
  readonly activityLiveUpdatesEnabled = this.store.activityLiveUpdatesEnabled;
  readonly activityConnectionState = this.store.activityConnectionState;
  readonly activityConnectionLabel = computed(() =>
    formatActivityConnectionLabel(this.activityConnectionState()),
  );
  readonly activityConnectionTone = computed(() =>
    resolveActivityConnectionTone(this.activityConnectionState()),
  );
  readonly capabilities = this.store.capabilities;
  readonly remoteHealth = this.store.remoteHealth;
  readonly remoteHealthLoading = this.store.remoteHealthLoading;
  readonly remoteMode = computed(() => this.capabilities()?.mode ?? 'Unknown');
  readonly remoteModeEnabled = computed(() => this.remoteMode() === 'Remote');
  readonly remoteBaseUrl = computed(() => this.capabilities()?.remoteBaseUrl ?? null);
  readonly remoteBaseUrlLabel = computed(() => this.remoteBaseUrl() ?? 'Not configured');
  readonly remoteIngressBaseUrl = computed(() =>
    this.capabilities()?.remoteIngressBaseUrl ?? this.remoteBaseUrl(),
  );
  readonly remoteHealthTone = computed(() =>
    resolveRemoteHealthTone(this.remoteHealth(), this.remoteHealthLoading()),
  );
  readonly remoteHealthLabel = computed(() =>
    formatRemoteHealthLabel(this.remoteHealth(), this.remoteHealthLoading()),
  );
  readonly remoteHealthCheckedAt = computed(() => this.remoteHealth()?.checkedAt ?? null);
  readonly remoteHealthDetailLabel = computed(() => formatRemoteHealthDetail(this.remoteHealth()));
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
  readonly timelineLookbackMs = signal(DEFAULT_LOOKBACK_MS);
  readonly timelinePresets = TIMELINE_PRESET_OPTIONS;
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

  readonly activityQuery = linkedSignal<WebhookActivityQuery>(() =>
    buildActivityQuery(
      this.filterModel(),
      this.selectedHookKeys(),
      this.selectedJobKeys(),
      this.timelineToIso(),
    ),
  );

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
      || this.timelineLookbackMs() !== DEFAULT_LOOKBACK_MS
      || !!this.timelineToIso()
    );
  });

  readonly timelineRangeActive = computed(() => !!this.timelineToIso());

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

  readonly internalIngressUrl = computed(() =>
    this.resolveIngressUrlFromBase(this.runtimeConfig.apiBaseUrl),
  );

  readonly remoteIngressUrl = computed(() =>
    this.remoteModeEnabled() ? this.resolveIngressUrlFromBase(this.remoteIngressBaseUrl()) : null,
  );

  readonly ingressUrl = computed(() => {
    const remoteUrl = this.remoteModeEnabled() ? this.remoteIngressUrl() : null;
    return remoteUrl ?? this.internalIngressUrl();
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

  copyIngressUrl(target?: 'internal' | 'remote'): void {
    const url = target === 'internal'
      ? this.internalIngressUrl()
      : target === 'remote'
        ? this.remoteIngressUrl()
        : this.ingressUrl();
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

  refreshRemoteHealth(): void {
    this.store.refreshRemoteHealth();
  }

  private resolveIngressUrlFromBase(baseUrl: string | null): string | null {
    const endpoint = this.selectedEndpoint();
    const tenantId = resolveNonEmptyString(this.tenantContext.tenantId());
    if (!endpoint || !tenantId) {
      return null;
    }
    const environment = resolveNonEmptyString(endpoint.environment)
      ?? resolveNonEmptyString(this.tenantContext.environment());
    const normalizedBase = resolveNonEmptyString(baseUrl);
    if (!environment || !normalizedBase) {
      return null;
    }
    return buildIngressUrl(normalizedBase, tenantId, environment, endpoint.hookKey);
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
    this.setTimelinePreset(DEFAULT_TIMELINE_PRESET);
  }

  clearTimelineRange(): void {
    this.timelineFromIso.set(null);
    this.timelineToIso.set(null);
  }

  setTimelineFromLocal(_value: string): void {
    this.timelineFromIso.set(null);
  }

  setTimelineToLocal(value: string): void {
    this.timelineToIso.set(localDateTimeInputToIso(value));
  }

  setTimelinePreset(kind: TimelinePresetKey): void {
    this.timelineLookbackMs.set(TIMELINE_PRESET_LOOKBACK_MS[kind]);
    this.timelineFromIso.set(null);
    this.timelineToIso.set(null);
  }

  setActivityLiveUpdates(enabled: boolean): void {
    this.store.setActivityLiveUpdatesEnabled(enabled);
  }

  retryActivity(): void {
    this.store.refreshActivity();
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

  readonly fallbackTimelineItems = computed<ReadonlyArray<WebhookTimelineItemView>>(() => {
    const filters = this.filterModel();
    const restrictToEndpointSet = filters.status !== 'all' || filters.environment !== ALL_ENVIRONMENTS;
    const endpoints = this.filteredEndpoints();
    const deadLetters = this.filteredDeadLetters();

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
        source: 'ingress',
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
          source: 'ingress',
        });
      });

    return timeline.sort((a, b) => Date.parse(b.occurredAt) - Date.parse(a.occurredAt));
  });

  readonly timelineItems = computed<ReadonlyArray<WebhookTimelineItemView>>(() => {
    const items = this.activityBackendReady()
      ? this.activityTimeline()
      : this.fallbackTimelineItems();
    const range = resolveTimelineRangeMs(this.timelineLookbackMs(), this.timelineToIso());
    return filterTimelineItemsByRange(items, range);
  });

  readonly activityBuckets = computed<ReadonlyArray<ActivityBucket>>(() => {
    const items = this.timelineItems();
    if (items.length === 0) {
      return [];
    }
    const range = resolveTimelineRangeMs(this.timelineLookbackMs(), this.timelineToIso());
    const bucketMs = resolveTimelineBucketMs(this.timelineLookbackMs());
    return buildActivityBuckets(
      items,
      new Date(range.fromMs).toISOString(),
      new Date(range.toMs).toISOString(),
      bucketMs,
    );
  });

  readonly activityTotalEntries = computed(() => this.timelineItems().length);
  readonly activityBucketCount = computed(() => this.activityBuckets().length);

  readonly activityChartOptions = computed<EChartsCoreOption | null>(() => {
    const items = this.timelineItems();
    if (items.length === 0) {
      return null;
    }
    const buckets = this.activityBuckets();
    const range = resolveTimelineRangeMs(this.timelineLookbackMs(), this.timelineToIso());
    const bucketMs = resolveTimelineBucketMs(this.timelineLookbackMs());
    const chartBuckets = buckets.length
      ? buckets
      : buildActivityBuckets(
        items,
        new Date(range.fromMs).toISOString(),
        new Date(range.toMs).toISOString(),
        bucketMs,
      );
    return buildTimelineChartOptions(items, chartBuckets, bucketMs);
  });

  constructor() {
    this.setTimelinePreset(DEFAULT_TIMELINE_PRESET);

    effect((onCleanup) => {
      // Auto-clear the "to" filter after the selected day ends.
      const toIso = this.timelineToIso();
      if (!toIso) {
        return;
      }
      const endOfDayMs = resolveLocalEndOfDayMs(toIso);
      if (endOfDayMs === null) {
        this.timelineToIso.set(null);
        return;
      }
      const delayMs = endOfDayMs - Date.now();
      if (delayMs <= 0) {
        this.timelineToIso.set(null);
        return;
      }
      const timeoutId = setTimeout(() => this.timelineToIso.set(null), delayMs);
      onCleanup(() => clearTimeout(timeoutId));
    });

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
      // Keep the store activity stream in sync with the active filters.
      this.store.setActivityQuery(this.activityQuery());
    });
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

  invokeWebhook(): void {
    if (this.writePermissionDenied() || this.invokeLoading()) {
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

  invokeWebhookFromHeader(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    this.invokeWebhook();
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

function resolveTimelineRangeMs(lookbackMs: number, toIso: string | null): { fromMs: number; toMs: number } {
  const resolvedLookback = Math.max(0, lookbackMs);
  const toMs = tryParseIsoToMs(toIso);
  const anchorMs = typeof toMs === 'number' ? toMs : Date.now();
  return {
    fromMs: anchorMs - resolvedLookback,
    toMs: anchorMs,
  };
}

function resolveTimelineBucketMs(lookbackMs: number): number {
  const rangeMs = Math.max(1, Math.abs(lookbackMs));
  for (const candidate of TIMELINE_BUCKET_MS_OPTIONS) {
    if (Math.ceil(rangeMs / candidate) <= MAX_TIMELINE_BUCKETS) {
      return candidate;
    }
  }
  return TIMELINE_BUCKET_MS_OPTIONS[TIMELINE_BUCKET_MS_OPTIONS.length - 1];
}

function filterTimelineItemsByRange(
  items: ReadonlyArray<WebhookTimelineItemView>,
  range: { fromMs: number; toMs: number },
): ReadonlyArray<WebhookTimelineItemView> {
  return items.filter((entry) => {
    const occurredAtMs = Date.parse(entry.occurredAt);
    if (!Number.isFinite(occurredAtMs)) {
      return true;
    }
    if (occurredAtMs < range.fromMs) {
      return false;
    }
    if (occurredAtMs > range.toMs) {
      return false;
    }
    return true;
  });
}

function resolveLocalEndOfDayMs(value: string): number | null {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) {
    return null;
  }
  const date = new Date(timestamp);
  date.setHours(23, 59, 59, 999);
  return date.getTime();
}

function createDefaultFilters(): WebhookFilterModel {
  return {
    status: 'all',
    environment: ALL_ENVIRONMENTS,
  };
}

function buildActivityQuery(
  model: WebhookFilterModel,
  hookKeys: ReadonlyArray<string>,
  jobKeys: ReadonlyArray<string>,
  toIso: string | null,
): WebhookActivityQuery {
  return {
    environment: model.environment === ALL_ENVIRONMENTS ? null : model.environment,
    hookKeys,
    jobKeys,
    toUtc: toIso,
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

function buildIngressUrl(baseUrl: string, tenantId: string, environment: string, hookKey: string): string | null {
  const normalizedBase = normalizeBaseForJoin(baseUrl);
  if (!normalizedBase) {
    return null;
  }
  const relative = `tenants/${encodeURIComponent(tenantId)}/environments/${encodeURIComponent(environment)}/webhooks/${encodeURIComponent(hookKey)}`;
  if (normalizedBase.startsWith('/')) {
    return `${normalizedBase.replace(/\/+$/, '')}/${relative}`;
  }
  try {
    return new URL(relative, normalizedBase).toString();
  } catch {
    return null;
  }
}

function normalizeBaseForJoin(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return '';
  }
  return trimmed.endsWith('/') ? trimmed : `${trimmed}/`;
}

function resolveNonEmptyString(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

type ActivityConnectionTone = 'success' | 'warning' | 'danger' | 'muted';

function resolveActivityConnectionTone(state: ActivityConnectionState): ActivityConnectionTone {
  if (state === 'connected') {
    return 'success';
  }
  if (state === 'retrying') {
    return 'warning';
  }
  if (state === 'offline') {
    return 'danger';
  }
  return 'muted';
}

function formatActivityConnectionLabel(state: ActivityConnectionState): string {
  if (state === 'connected') {
    return 'Connected';
  }
  if (state === 'retrying') {
    return 'Retrying';
  }
  if (state === 'offline') {
    return 'Offline';
  }
  if (state === 'paused') {
    return 'Paused';
  }
  return 'Idle';
}

type RemoteHealthTone = ActivityConnectionTone;

function resolveRemoteHealthTone(
  health: WebhookRemoteHealthView | null,
  isLoading: boolean,
): RemoteHealthTone {
  if (isLoading) {
    return 'warning';
  }
  if (!health) {
    return 'muted';
  }

  switch (health.status) {
    case 'ok':
      return 'success';
    case 'unhealthy':
      return 'danger';
    case 'unreachable':
    case 'unavailable':
      return 'warning';
    case 'not-configured':
      return 'muted';
    default:
      return 'muted';
  }
}

function formatRemoteHealthLabel(
  health: WebhookRemoteHealthView | null,
  isLoading: boolean,
): string {
  if (isLoading) {
    return 'Checking...';
  }
  if (!health) {
    return 'No check yet';
  }

  switch (health.status) {
    case 'ok':
      return 'Healthy';
    case 'unhealthy':
      return 'Unhealthy';
    case 'unreachable':
      return 'Unreachable';
    case 'unavailable':
      return 'Unavailable';
    case 'not-configured':
      return 'Not configured';
    default:
      return 'Unknown';
  }
}

function formatRemoteHealthDetail(health: WebhookRemoteHealthView | null): string | null {
  if (!health) {
    return null;
  }

  const detail = health.detail?.trim() ?? '';
  const statusCode = typeof health.statusCode === 'number' ? health.statusCode : null;
  if (!detail) {
    if (typeof statusCode === 'number' && statusCode >= 400) {
      return `HTTP ${statusCode}`;
    }
    return null;
  }

  return typeof statusCode === 'number' ? `HTTP ${statusCode}: ${detail}` : detail;
}

type ChartPalette = {
  pending: string;
  leased: string;
  success: string;
  warning: string;
  failed: string;
  muted: string;
  border: string;
  surface: string;
  text: string;
};

type BucketStatusCount = {
  pending: number;
  leased: number;
  success: number;
  warning: number;
  failed: number;
  total: number;
};

type TimelineBucketRange = {
  startMs: number;
  endMs: number;
  label: string;
};

type TooltipSeriesEntry = {
  axisValue?: unknown;
  axisValueLabel?: unknown;
  seriesName?: unknown;
  value?: unknown;
  marker?: unknown;
  data?: unknown;
  dataIndex?: unknown;
};

function buildActivityBuckets(
  items: ReadonlyArray<WebhookTimelineItemView>,
  fromIso: string | null,
  toIso: string | null,
  bucketMs: number,
): ReadonlyArray<ActivityBucket> {
  if (items.length === 0) {
    return [];
  }

  const resolvedBucketMs = Math.max(1, Math.floor(bucketMs));
  const fromMs = tryParseIsoToMs(fromIso);
  const toMs = tryParseIsoToMs(toIso);
  const timestamps = items
    .map((entry) => Date.parse(entry.occurredAt))
    .filter((value) => Number.isFinite(value)) as number[];

  if (timestamps.length === 0) {
    return [];
  }

  const resolvedFrom = typeof fromMs === 'number' ? fromMs : Math.min(...timestamps);
  const resolvedTo = typeof toMs === 'number' ? toMs : Math.max(...timestamps);

  if (!Number.isFinite(resolvedFrom) || !Number.isFinite(resolvedTo) || resolvedTo < resolvedFrom) {
    return [];
  }

  const start = resolvedFrom;
  const end = resolvedTo;
  const bucketCount = Math.max(1, Math.ceil((end - start) / resolvedBucketMs));

  const buckets = new Map<number, ActivityBucket>();
  for (let i = 0; i < bucketCount; i += 1) {
    const bucketStart = start + i * resolvedBucketMs;
    const bucketEnd = i === bucketCount - 1 ? end : bucketStart + resolvedBucketMs;
    buckets.set(bucketStart, {
      bucketStart: new Date(bucketStart).toISOString(),
      bucketEnd: new Date(bucketEnd).toISOString(),
      total: 0,
      errors: 0,
      warnings: 0,
      pending: 0,
      leased: 0,
      deadLetters: 0,
    });
  }

  items.forEach((entry) => {
    const timestamp = Date.parse(entry.occurredAt);
    if (!Number.isFinite(timestamp) || timestamp < start || timestamp > end) {
      return;
    }
    const bucketIndex = Math.min(bucketCount - 1, Math.floor((timestamp - start) / resolvedBucketMs));
    const bucketStart = start + bucketIndex * resolvedBucketMs;
    const bucket = buckets.get(bucketStart);
    if (!bucket) {
      return;
    }
    bucket.total += 1;
    if (entry.kind === 'deadLetter') {
      bucket.deadLetters += 1;
    }
    if (entry.status === 'failed') {
      bucket.errors += 1;
    } else if (entry.status === 'warning') {
      bucket.warnings += 1;
    } else if (entry.status === 'pending') {
      bucket.pending += 1;
    } else if (entry.status === 'leased') {
      bucket.leased += 1;
    }
  });

  return Array.from(buckets.values());
}

function buildTimelineChartOptions(
  items: ReadonlyArray<WebhookTimelineItemView>,
  buckets: ReadonlyArray<ActivityBucket>,
  bucketMs: number,
): EChartsCoreOption {
  const palette = resolveChartPalette();
  const bucketRanges = resolveTimelineBucketRanges(items, buckets, bucketMs);
  if (bucketRanges.length === 0) {
    return {
      animation: false,
      series: [],
    };
  }

  const orderedItems = sortTimelineItems(items);
  const categories = bucketRanges.map((range) => range.label);
  const labelInterval = resolveAxisLabelInterval(categories.length);
  const bucketCounts = buildBucketStatusCounts(orderedItems, bucketRanges);
  const series = buildTimelineStatusSeries(bucketCounts, palette);
  const maxStack = Math.max(1, ...bucketCounts.map((entry) => entry.total));
  const yAxisInterval = resolveYAxisInterval(maxStack);

  return {
    animation: false,
    legend: {
      top: 0,
      left: 0,
      data: ACTIVITY_STATUS_DEFINITIONS.map((definition) => definition.label),
      icon: 'circle',
      itemWidth: 10,
      itemHeight: 10,
      textStyle: {
        color: palette.muted,
        fontSize: 11,
      },
      formatter: buildLegendSummaryFormatter(bucketCounts),
    },
    grid: {
      left: 24,
      right: 16,
      top: 48,
      bottom: 48,
      outerBoundsMode: 'same',
      outerBoundsContain: 'axisLabel',
    },
    tooltip: {
      trigger: 'axis',
      axisPointer: {
        type: 'shadow',
      },
      backgroundColor: palette.surface,
      borderColor: palette.border,
      borderWidth: 1,
      textStyle: {
        color: palette.text,
      },
      extraCssText: 'border-radius: 10px; box-shadow: 0 10px 24px rgba(0,0,0,0.35);',
      formatter: (params: unknown) => formatTimelineBucketTooltip(params, bucketRanges, bucketCounts),
    },
    xAxis: {
      type: 'category',
      data: categories,
      axisLabel: {
        color: palette.muted,
        rotate: 0,
        interval: labelInterval,
      },
      axisLine: {
        lineStyle: {
          color: palette.border,
        },
      },
      axisTick: {
        show: false,
      },
    },
    yAxis: {
      type: 'value',
      min: 0,
      max: maxStack,
      interval: yAxisInterval,
      axisLabel: {
        color: palette.muted,
      },
      splitLine: {
        lineStyle: {
          color: palette.border,
          opacity: 0.25,
        },
      },
    },
    series,
  };
}

function buildBucketStatusCounts(
  items: ReadonlyArray<WebhookTimelineItemView>,
  bucketRanges: ReadonlyArray<TimelineBucketRange>,
): ReadonlyArray<BucketStatusCount> {
  if (bucketRanges.length === 0) {
    return [];
  }

  const buckets = bucketRanges.map(() => ({
    pending: 0,
    leased: 0,
    success: 0,
    warning: 0,
    failed: 0,
    total: 0,
  }));

  items.forEach((entry) => {
    const bucketIndex = resolveBucketIndex(entry.occurredAt, bucketRanges);
    if (bucketIndex < 0) {
      return;
    }
    const bucket = buckets[bucketIndex];
    bucket[entry.status] += 1;
    bucket.total += 1;
  });

  return buckets;
}

function buildTimelineStatusSeries(
  bucketCounts: ReadonlyArray<BucketStatusCount>,
  palette: ChartPalette,
): SeriesOption[] {
  if (bucketCounts.length === 0) {
    return [];
  }

  return ACTIVITY_STATUS_DEFINITIONS.map((definition) => ({
    name: definition.label,
    type: 'bar',
    stack: 'activity',
    barMaxWidth: 18,
    barCategoryGap: '32%',
    data: bucketCounts.map((entry) => entry[definition.status]),
    itemStyle: {
      color: resolveStatusColor(definition.status, palette),
    },
    label: {
      show: false,
    },
    emphasis: {
      focus: 'series',
    },
  }));
}

function buildLegendSummaryFormatter(
  bucketCounts: ReadonlyArray<BucketStatusCount>,
): (name: string) => string {
  const totals: Record<WebhookTimelineItemView['status'], number> = {
    pending: 0,
    leased: 0,
    success: 0,
    warning: 0,
    failed: 0,
  };

  bucketCounts.forEach((bucket) => {
    totals.pending += bucket.pending;
    totals.leased += bucket.leased;
    totals.success += bucket.success;
    totals.warning += bucket.warning;
    totals.failed += bucket.failed;
  });

  const totalCount = totals.pending + totals.leased + totals.success + totals.warning + totals.failed;
  const labelTotals = new Map<string, number>();
  ACTIVITY_STATUS_DEFINITIONS.forEach((definition) => {
    labelTotals.set(definition.label, totals[definition.status]);
  });

  return (name: string): string => {
    const count = labelTotals.get(name);
    if (count === undefined) {
      return name;
    }

    const percent = totalCount > 0 ? Math.round((count / totalCount) * 100) : 0;
    return `${name} ${count} (${percent}%)`;
  };
}

function resolveTimelineBucketRanges(
  items: ReadonlyArray<WebhookTimelineItemView>,
  buckets: ReadonlyArray<ActivityBucket>,
  bucketMs: number,
): ReadonlyArray<TimelineBucketRange> {
  const resolvedBucketMs = Math.max(1, Math.floor(bucketMs));
  const sourceBuckets = buckets.length > 0
    ? buckets
    : buildActivityBuckets(items, null, null, resolvedBucketMs);

  if (sourceBuckets.length === 0) {
    return [];
  }

  const parsed = sourceBuckets
    .map((bucket) => {
      const startMs = tryParseIsoToMs(bucket.bucketStart);
      const endMs = bucket.bucketEnd ? tryParseIsoToMs(bucket.bucketEnd) : null;
      if (startMs === null) {
        return null;
      }
      return {
        startMs,
        endMs: endMs ?? null,
      };
    })
    .filter((entry): entry is { startMs: number; endMs: number | null } => !!entry)
    .sort((left, right) => left.startMs - right.startMs);

  if (parsed.length === 0) {
    return [];
  }

  return parsed.map((entry, index) => {
    const nextStart = parsed[index + 1]?.startMs;
    let endMs = entry.endMs ?? (typeof nextStart === 'number' ? nextStart : entry.startMs + resolvedBucketMs);
    if (!Number.isFinite(endMs) || endMs <= entry.startMs) {
      endMs = entry.startMs + resolvedBucketMs;
    }

    return {
      startMs: entry.startMs,
      endMs,
      label: formatTimelineAxisLabel(entry.startMs),
    };
  });
}

function resolveBucketIndex(occurredAt: string, bucketRanges: ReadonlyArray<TimelineBucketRange>): number {
  const occurredMs = Date.parse(occurredAt);
  if (!Number.isFinite(occurredMs)) {
    return -1;
  }

  for (let i = 0; i < bucketRanges.length; i += 1) {
    const range = bucketRanges[i];
    if (occurredMs >= range.startMs && occurredMs < range.endMs) {
      return i;
    }
  }

  return -1;
}

function resolveStatusColor(status: WebhookTimelineItemView['status'], palette: ChartPalette): string {
  if (status === 'failed') {
    return palette.failed;
  }
  if (status === 'warning') {
    return palette.warning;
  }
  if (status === 'pending') {
    return palette.pending;
  }
  if (status === 'leased') {
    return palette.leased;
  }
  return palette.success;
}

function sortTimelineItems(items: ReadonlyArray<WebhookTimelineItemView>): ReadonlyArray<WebhookTimelineItemView> {
  return items
    .slice()
    .sort((left, right) => {
      const leftMs = Date.parse(left.occurredAt);
      const rightMs = Date.parse(right.occurredAt);
      if (Number.isFinite(leftMs) && Number.isFinite(rightMs)) {
        return leftMs - rightMs;
      }
      return left.occurredAt.localeCompare(right.occurredAt);
    });
}

function resolveAxisLabelInterval(count: number): number {
  if (count <= 8) {
    return 0;
  }
  return Math.ceil(count / 8);
}

function resolveYAxisInterval(maxValue: number): number {
  const safeMax = Math.max(1, Math.floor(maxValue));
  const maxLabels = 10;
  return Math.max(1, Math.ceil(safeMax / (maxLabels - 1)));
}

function formatTimelineBucketTooltip(
  params: unknown,
  bucketRanges: ReadonlyArray<TimelineBucketRange>,
  bucketCounts: ReadonlyArray<BucketStatusCount>,
): string {
  const entries = normalizeTooltipEntries(params);
  const bucketIndex = resolveTooltipBucketIndex(entries);
  if (bucketIndex === null) {
    return '';
  }

  const bucket = bucketRanges[bucketIndex];
  const counts = bucketCounts[bucketIndex];
  if (!bucket || !counts) {
    return '';
  }

  const rangeLabel = formatTimelineBucketRange(bucket);

  return `
    <div style="min-width: 200px;">
      <div style="font-weight:600;margin-bottom:6px;">${escapeTooltipValue(rangeLabel)}</div>
      <div style="display:flex;justify-content:space-between;gap:8px;">
        <span>Total</span>
        <span>${counts.total}</span>
      </div>
      <div style="display:flex;justify-content:space-between;gap:8px;">
        <span>Pending</span>
        <span>${counts.pending}</span>
      </div>
      <div style="display:flex;justify-content:space-between;gap:8px;">
        <span>Leased</span>
        <span>${counts.leased}</span>
      </div>
      <div style="display:flex;justify-content:space-between;gap:8px;">
        <span>Success</span>
        <span>${counts.success}</span>
      </div>
      <div style="display:flex;justify-content:space-between;gap:8px;">
        <span>Warning</span>
        <span>${counts.warning}</span>
      </div>
      <div style="display:flex;justify-content:space-between;gap:8px;">
        <span>Failed</span>
        <span>${counts.failed}</span>
      </div>
    </div>
  `;
}

function normalizeTooltipEntries(params: unknown): TooltipSeriesEntry[] {
  if (Array.isArray(params)) {
    return params as TooltipSeriesEntry[];
  }
  if (params) {
    return [params as TooltipSeriesEntry];
  }
  return [];
}

function resolveTooltipBucketIndex(entries: TooltipSeriesEntry[]): number | null {
  const entry = entries.find((item) => typeof item.dataIndex === 'number');
  if (!entry || typeof entry.dataIndex !== 'number') {
    return null;
  }
  return entry.dataIndex;
}

function formatTimelineBucketRange(bucket: TimelineBucketRange): string {
  const startIso = new Date(bucket.startMs).toISOString();
  const endIso = new Date(bucket.endMs).toISOString();
  const startLabel = formatTimelineDate(startIso);
  const endLabel = formatTimelineDate(endIso);
  return `${startLabel} — ${endLabel}`;
}

function formatTimelineAxisLabel(value: string | number): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) {
    return String(value);
  }
  const dateLabel = date.toLocaleDateString(undefined, { month: '2-digit', day: '2-digit' });
  const timeLabel = date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12: false });
  return `${dateLabel}\n${timeLabel}`;
}

function formatTimelineDate(value: string): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) {
    return value;
  }
  const dateLabel = date.toLocaleDateString(undefined, { month: '2-digit', day: '2-digit' });
  const timeLabel = date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12: false });
  return `${dateLabel} ${timeLabel}`;
}

function escapeTooltipValue(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function resolveChartPalette(): ChartPalette {
  if (typeof window === 'undefined') {
    return {
      pending: '#93c5fd',
      leased: '#f9a8d4',
      success: '#34d399',
      warning: '#facc15',
      failed: '#fb7181',
      muted: '#94a3b8',
      border: '#27344d',
      surface: '#162033',
      text: '#f8fafc',
    };
  }
  const styles = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const surface = read('--cq-surface-3', '#162033');
  return {
    pending: read('--cq-graph-1', '#93c5fd'),
    leased: read('--cq-graph-2', '#f9a8d4'),
    success: read('--cq-success', '#34d399'),
    warning: read('--cq-warning', '#facc15'),
    failed: read('--cq-danger-2', '#fb7181'),
    muted: read('--cq-text-secondary', '#94a3b8'),
    border: read('--cq-border', '#27344d'),
    surface,
    text: read('--cq-text-primary', '#f8fafc'),
  };
}
