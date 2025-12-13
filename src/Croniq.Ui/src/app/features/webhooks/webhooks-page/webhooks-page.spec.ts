import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { WebhooksPage } from './webhooks-page';
import { WebhooksStore } from '../webhooks.store';

class WebhooksStoreStub {
  readonly endpoints = signal([]);
  readonly actionLog = signal([]);
  readonly loading = signal(false);
  readonly deadLetterCount = signal(0);
  readonly lastError = signal<string | null>(null);
  readonly activeCount = signal(0);

  refreshEndpoints = jasmine.createSpy('refreshEndpoints');
  upsertEndpoint = jasmine.createSpy('upsertEndpoint');
  deleteEndpoint = jasmine.createSpy('deleteEndpoint');
  rotateSecret = jasmine.createSpy('rotateSecret');
  createIpRule = jasmine.createSpy('createIpRule');
  deleteIpRule = jasmine.createSpy('deleteIpRule');
  replayDeadLetter = jasmine.createSpy('replayDeadLetter');
  invokeWebhook = jasmine.createSpy('invokeWebhook');
}

describe('WebhooksPage', () => {
  let component: WebhooksPage;
  let fixture: ComponentFixture<WebhooksPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [WebhooksPage],
      providers: [{ provide: WebhooksStore, useClass: WebhooksStoreStub }],
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
