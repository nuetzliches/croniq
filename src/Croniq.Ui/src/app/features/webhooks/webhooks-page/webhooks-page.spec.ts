import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { WebhookDeadLetterView, WebhookEndpointView, WebhooksStore } from '@features/webhooks/webhooks.store';
import { WebhooksPage } from './webhooks-page';

class WebhooksStoreStub {
  readonly endpoints = signal<ReadonlyArray<WebhookEndpointView>>([]);
  readonly actionLog = signal([]);
  readonly loading = signal(false);
  readonly deadLetterCount = signal(0);
  readonly deadLetters = signal<ReadonlyArray<WebhookDeadLetterView>>([]);
  readonly ipRules = signal([]);
  readonly rotatedSecret = signal<string | null>(null);
  readonly capabilities = signal(null);
  readonly lastError = signal<string | null>(null);
  readonly readPermissionDenied = signal(false);
  readonly writePermissionDenied = signal(false);
  readonly activeCount = signal(0);
  readonly activityTimeline = signal([]);
  readonly activityBuckets = signal([]);
  readonly activityLoading = signal(false);
  readonly activityBackendReady = signal(false);
  readonly activityError = signal<string | null>(null);

  selectHook = vi.fn();
  setActivityQuery = vi.fn();

  refreshEndpoints = vi.fn();
  upsertEndpoint = vi.fn();
  deleteEndpoint = vi.fn();
  rotateSecret = vi.fn();
  createIpRule = vi.fn();
  deleteIpRule = vi.fn();
  replayDeadLetter = vi.fn();
  invokeWebhook = vi.fn();
  clearRotatedSecret = vi.fn();
}

describe('WebhooksPage', () => {
  let component: WebhooksPage;
  let fixture: ComponentFixture<WebhooksPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [WebhooksPage],
      providers: [provideZonelessChangeDetection()],
    })
      .overrideComponent(WebhooksPage, {
        set: {
          providers: [{ provide: WebhooksStore, useClass: WebhooksStoreStub }],
        },
      })
      .compileComponents();

    fixture = TestBed.createComponent(WebhooksPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('summarizes timeline activity for charting', () => {
    const store = fixture.componentRef.injector.get(WebhooksStore) as unknown as WebhooksStoreStub;

    store.endpoints.set([
      {
        hookKey: 'alpha',
        jobKey: 'job-a',
        environment: 'dev',
        requireSignature: true,
        status: 'active',
        lastDeliveryAt: '2026-01-20T09:15:00.000Z',
        ipRuleCount: 0,
      },
      {
        hookKey: 'beta',
        jobKey: 'job-b',
        environment: 'dev',
        requireSignature: true,
        status: 'degraded',
        lastDeliveryAt: '2026-01-20T10:20:00.000Z',
        ipRuleCount: 0,
      },
    ]);

    store.deadLetters.set([
      {
        id: '1',
        hookKey: 'alpha',
        jobKey: 'job-a',
        occurredAt: '2026-01-20T09:30:00.000Z',
        reason: 'Delivery failed.',
      },
      {
        id: '2',
        hookKey: 'beta',
        jobKey: 'job-b',
        occurredAt: '2026-01-20T10:05:00.000Z',
        reason: 'Delivery failed.',
      },
    ]);

    const buckets = component.activityBuckets();
    expect(buckets.map((bucket) => bucket.bucketStart)).toEqual([
      '2026-01-20T09:00:00.000Z',
      '2026-01-20T10:00:00.000Z',
    ]);
    expect(buckets.map((bucket) => bucket.total)).toEqual([2, 2]);
    expect(buckets.map((bucket) => bucket.errors)).toEqual([1, 1]);

    const summary = component.activitySummary();
    expect(summary).toEqual({
      total: 4,
      errors: 2,
      errorRateLabel: '50%',
      bucketCount: 2,
    });

    const options = component.activityChartOptions() as {
      series?: Array<{ name?: string; data?: Array<[string, number]> }>;
      tooltip?: { formatter?: unknown };
    } | null;

    expect(options).not.toBeNull();
    const series = options?.series ?? [];
    const totalSeries = series.find((entry) => entry.name === 'Total');
    const errorSeries = series.find((entry) => entry.name === 'Errors');

    const resolveSeriesValue = (entry: unknown): [string, number] => {
      if (entry && typeof entry === 'object' && 'value' in entry) {
        return (entry as { value: [string, number] }).value;
      }
      if (Array.isArray(entry) && entry.length === 2 && typeof entry[0] === 'string' && typeof entry[1] === 'number') {
        return [entry[0], entry[1]];
      }
      return ['unknown', 0];
    };

    const totalValues = (totalSeries?.data ?? []).map(resolveSeriesValue);
    const errorValues = (errorSeries?.data ?? []).map(resolveSeriesValue);

    expect(totalValues).toEqual([
      ['2026-01-20T09:00:00.000Z', 2],
      ['2026-01-20T10:00:00.000Z', 2],
    ]);
    expect(errorValues).toEqual([
      ['2026-01-20T09:00:00.000Z', 1],
      ['2026-01-20T10:00:00.000Z', 1],
    ]);

    const formatter = options?.tooltip?.formatter;
    expect(typeof formatter).toBe('function');
    const tooltip = (formatter as (params: unknown) => string)([
      {
        axisValue: '2026-01-20T09:00:00.000Z',
        seriesName: 'Total',
        value: ['2026-01-20T09:00:00.000Z', 2],
        marker: '',
      },
      {
        axisValue: '2026-01-20T09:00:00.000Z',
        seriesName: 'Errors',
        value: ['2026-01-20T09:00:00.000Z', 1],
        marker: '',
      },
    ]);
    expect(tooltip).toContain('Error rate');
  });
});
