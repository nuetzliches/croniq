import { Injectable, computed, effect, inject, signal } from '@angular/core';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { DEFAULT_UI_PREFERENCES, isUiTableDensityPreference, isUiThemePreference, normalizeUiPreferences, type UiPreferences, type UiTableDensityPreference, type UiThemePreference } from './ui-preferences.types';
import { UiPreferencesStorage } from './ui-preferences-storage.service';

export type PreferencesSaveState = 'idle' | 'saving' | 'error';

@Injectable({ providedIn: 'root' })
export class UiPreferencesService {
  private readonly tenantContext = inject(TenantContextService);
  private readonly storage = inject(UiPreferencesStorage);
  private readonly preferencesSignal = signal<UiPreferences>(DEFAULT_UI_PREFERENCES);
  private readonly saveStateSignal = signal<PreferencesSaveState>('idle');
  private readonly lastSavedAtSignal = signal<string | null>(null);
  private loadSequence = 0;
  private saveSequence = 0;

  readonly preferences = this.preferencesSignal.asReadonly();
  readonly theme = computed(() => this.preferencesSignal().theme);
  readonly tableDensity = computed(() => this.preferencesSignal().tableDensity);
  readonly saveState = this.saveStateSignal.asReadonly();
  readonly lastSavedAt = this.lastSavedAtSignal.asReadonly();
  readonly storageMode = this.storage.mode;

  constructor() {
    this.applyPreferences(this.preferencesSignal());

    effect(() => {
      const tenantId = this.tenantContext.tenantId();
      if (!tenantId) {
        return;
      }

      const sequence = ++this.loadSequence;
      void this.loadPreferences(tenantId, sequence);
    });
  }

  updateTheme(theme: UiThemePreference): void {
    if (!isUiThemePreference(theme)) {
      return;
    }
    this.updatePreferences({ theme });
  }

  updateTableDensity(tableDensity: UiTableDensityPreference): void {
    if (!isUiTableDensityPreference(tableDensity)) {
      return;
    }
    this.updatePreferences({ tableDensity });
  }

  resetToDefaults(): void {
    this.setPreferences(DEFAULT_UI_PREFERENCES);
    void this.persistPreferences(this.preferencesSignal());
  }

  private updatePreferences(patch: Partial<UiPreferences>): void {
    const current = this.preferencesSignal();
    const next = normalizeUiPreferences({ ...current, ...patch });
    if (current.theme === next.theme && current.tableDensity === next.tableDensity) {
      return;
    }

    this.setPreferences(next);
    void this.persistPreferences(next);
  }

  private setPreferences(preferences: UiPreferences): void {
    this.preferencesSignal.set(preferences);
    this.applyPreferences(preferences);
  }

  private async loadPreferences(tenantId: string, sequence: number): Promise<void> {
    const record = await this.storage.load(tenantId);
    if (sequence !== this.loadSequence) {
      return;
    }

    if (!record) {
      this.setPreferences(DEFAULT_UI_PREFERENCES);
      this.lastSavedAtSignal.set(null);
      this.saveStateSignal.set('idle');
      return;
    }

    this.setPreferences(normalizeUiPreferences(record.preferences));
    this.lastSavedAtSignal.set(record.updatedAt ?? null);
    this.saveStateSignal.set('idle');
  }

  private async persistPreferences(preferences: UiPreferences): Promise<void> {
    const tenantId = this.tenantContext.tenantId();
    if (!tenantId) {
      return;
    }

    const sequence = ++this.saveSequence;
    this.saveStateSignal.set('saving');

    try {
      const updatedAt = await this.storage.save(tenantId, preferences);
      if (sequence !== this.saveSequence) {
        return;
      }
      this.lastSavedAtSignal.set(updatedAt);
      this.saveStateSignal.set('idle');
    } catch {
      if (sequence !== this.saveSequence) {
        return;
      }
      this.saveStateSignal.set('error');
    }
  }

  private applyPreferences(preferences: UiPreferences): void {
    if (typeof document === 'undefined') {
      return;
    }

    // Imperative DOM update to apply global theme and density.
    const root = document.documentElement;
    root.setAttribute('data-theme', preferences.theme);
    root.setAttribute('data-density', preferences.tableDensity);
  }
}
