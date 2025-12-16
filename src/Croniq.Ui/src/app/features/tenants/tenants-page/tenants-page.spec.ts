import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { TenantsStore } from '../tenants.store';
import { TenantsPage } from './tenants-page';

class TenantsStoreStub {
  readonly activity = signal([]);
  readonly lastLookup = signal(null);
  readonly busy = signal(false);
  readonly lastError = signal<string | null>(null);

  issueApiKey = vi.fn();
  rotateApiKey = vi.fn();
  deleteApiKey = vi.fn();
  lookupApiClient = vi.fn();
}

describe('TenantsPage', () => {
  let component: TenantsPage;
  let fixture: ComponentFixture<TenantsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [TenantsPage],
      providers: [provideZonelessChangeDetection(), { provide: TenantsStore, useClass: TenantsStoreStub }],
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
