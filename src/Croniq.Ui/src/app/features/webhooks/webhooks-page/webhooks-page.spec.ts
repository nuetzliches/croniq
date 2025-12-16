import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { WebhooksStore } from '../webhooks.store';
import { WebhooksPage } from './webhooks-page';

class WebhooksStoreStub {
  readonly endpoints = signal([]);
  readonly actionLog = signal([]);
  readonly loading = signal(false);
  readonly deadLetterCount = signal(0);
  readonly lastError = signal<string | null>(null);
  readonly activeCount = signal(0);

  refreshEndpoints = vi.fn();
  upsertEndpoint = vi.fn();
  deleteEndpoint = vi.fn();
  rotateSecret = vi.fn();
  createIpRule = vi.fn();
  deleteIpRule = vi.fn();
  replayDeadLetter = vi.fn();
  invokeWebhook = vi.fn();
}

describe('WebhooksPage', () => {
  let component: WebhooksPage;
  let fixture: ComponentFixture<WebhooksPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [WebhooksPage],
      providers: [provideZonelessChangeDetection(), { provide: WebhooksStore, useClass: WebhooksStoreStub }],
    })
      .compileComponents();

    fixture = TestBed.createComponent(WebhooksPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
