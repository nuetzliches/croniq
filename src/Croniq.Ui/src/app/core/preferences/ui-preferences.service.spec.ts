import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { UiPreferencesService } from './ui-preferences.service';
import type { PreferencesStorageMode } from './ui-preferences-storage.service';
import { UiPreferencesStorage } from './ui-preferences-storage.service';
import type { UiPreferences, UiPreferencesRecord } from './ui-preferences.types';

class TenantContextStub {
  tenantId = signal('tenant-a');
}

class PreferencesStorageStub {
  mode = signal<PreferencesStorageMode>('indexeddb');
  load = vi.fn<(tenantId: string) => Promise<UiPreferencesRecord | null>>();
  save = vi.fn<(tenantId: string, preferences: UiPreferences) => Promise<string>>();
  clear = vi.fn<(tenantId: string) => Promise<void>>();
}

const flushPromises = () => new Promise<void>((resolve) => queueMicrotask(() => resolve()));

describe('UiPreferencesService', () => {
  let storage: PreferencesStorageStub;

  beforeEach(() => {
    storage = new PreferencesStorageStub();
    storage.load.mockResolvedValue(null);
    storage.save.mockResolvedValue('2026-01-10T12:01:00.000Z');

    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        UiPreferencesService,
        { provide: UiPreferencesStorage, useValue: storage },
        { provide: TenantContextService, useClass: TenantContextStub },
      ],
    });
  });

  afterEach(() => {
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.removeAttribute('data-density');
  });

  it('applies stored preferences on load', async () => {
    storage.load.mockResolvedValue({
      tenantId: 'tenant-a',
      preferences: { theme: 'ops-light', tableDensity: 'compact' },
      updatedAt: '2026-01-10T12:00:00.000Z',
    });

    const service = TestBed.inject(UiPreferencesService);
    TestBed.flushEffects();
    await flushPromises();

    expect(storage.load).toHaveBeenCalledWith('tenant-a');
    expect(service.preferences()).toEqual({ theme: 'ops-light', tableDensity: 'compact' });
    expect(document.documentElement.getAttribute('data-theme')).toBe('ops-light');
    expect(document.documentElement.getAttribute('data-density')).toBe('compact');
  });

  it('persists updates and updates the DOM', async () => {
    const service = TestBed.inject(UiPreferencesService);
    TestBed.flushEffects();
    await flushPromises();

    service.updateTheme('ops-light');
    await flushPromises();

    expect(storage.save).toHaveBeenCalledWith('tenant-a', {
      theme: 'ops-light',
      tableDensity: 'comfortable',
    });
    expect(service.lastSavedAt()).toBe('2026-01-10T12:01:00.000Z');
    expect(document.documentElement.getAttribute('data-theme')).toBe('ops-light');
  });
});
