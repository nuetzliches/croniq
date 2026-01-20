import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivityConnectionState, WebhookDeadLetterView, WebhookEndpointView, WebhooksStore } from '@features/webhooks/webhooks.store';
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
  readonly activityLiveUpdatesEnabled = signal(true);
  readonly activityConnectionState = signal<ActivityConnectionState>('connected');

  selectHook = vi.fn();
  setActivityQuery = vi.fn();
  setActivityLiveUpdatesEnabled = vi.fn();
  refreshActivity = vi.fn();

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

  it('summarizes timeline activity for legend entries', () => {
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

    const legend = component.activityLegendEntries();
    expect(legend).toEqual([
      {
        status: 'success',
        label: 'Success',
        count: 1,
        percentLabel: '25%',
      },
      {
        status: 'warning',
        label: 'Warning',
        count: 1,
        percentLabel: '25%',
      },
      {
        status: 'failed',
        label: 'Failed',
        count: 2,
        percentLabel: '50%',
      },
    ]);

    expect(component.activityChartOptions()).not.toBeNull();
  });
});
