import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { TenantsPage } from './tenants-page';
import { TenantsStore } from '../tenants.store';

class TenantsStoreStub {
  readonly activity = signal([]);
  readonly lastLookup = signal(null);
  readonly busy = signal(false);
  readonly lastError = signal<string | null>(null);

  issueApiKey = jasmine.createSpy('issueApiKey');
  rotateApiKey = jasmine.createSpy('rotateApiKey');
  deleteApiKey = jasmine.createSpy('deleteApiKey');
  lookupApiClient = jasmine.createSpy('lookupApiClient');
}

describe('TenantsPage', () => {
  let component: TenantsPage;
  let fixture: ComponentFixture<TenantsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [TenantsPage],
      providers: [{ provide: TenantsStore, useClass: TenantsStoreStub }],
    })
      .compileComponents();

    fixture = TestBed.createComponent(TenantsPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
