import { ComponentFixture, TestBed } from '@angular/core/testing';

import { WebhooksPage } from './webhooks-page';

describe('WebhooksPage', () => {
  let component: WebhooksPage;
  let fixture: ComponentFixture<WebhooksPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [WebhooksPage]
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
