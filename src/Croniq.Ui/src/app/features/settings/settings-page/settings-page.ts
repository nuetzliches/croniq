import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, linkedSignal } from '@angular/core';
import { FormField, form } from '@angular/forms/signals';
import { UiPreferencesService } from '@core/preferences/ui-preferences.service';
import { type UiPreferences, type UiTableDensityPreference, type UiThemePreference } from '@core/preferences/ui-preferences.types';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';

interface PreferencesFormModel {
  theme: UiThemePreference;
  tableDensity: UiTableDensityPreference;
}

function mapToFormModel(preferences: UiPreferences): PreferencesFormModel {
  return {
    theme: preferences.theme,
    tableDensity: preferences.tableDensity,
  };
}

@Component({
  selector: 'cq-settings-page',
  imports: [DatePipe, FormField],
  templateUrl: './settings-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsPage {
  private readonly preferences = inject(UiPreferencesService);
  private readonly tenantContext = inject(TenantContextService);

  readonly tenantId = this.tenantContext.tenantId;
  readonly environment = this.tenantContext.environment;
  readonly storageMode = this.preferences.storageMode;
  readonly saveState = this.preferences.saveState;
  readonly lastSavedAt = this.preferences.lastSavedAt;
  readonly storageLabel = computed(() =>
    this.storageMode() === 'indexeddb' ? 'IndexedDB' : 'Session memory',
  );

  readonly model = linkedSignal(() => mapToFormModel(this.preferences.preferences()));
  readonly preferencesForm = form(this.model, () => {});

  readonly saveMessage = computed(() => {
    const state = this.saveState();
    if (state === 'saving') {
      return 'Saving preferences...';
    }
    if (state === 'error') {
      return 'Save failed. Changes are kept in memory.';
    }
    return this.lastSavedAt() ? 'Preferences saved.' : 'Preferences are not saved yet.';
  });

  readonly storageMessage = computed(() => {
    return this.storageMode() === 'indexeddb'
      ? 'Stored in IndexedDB (persistent per tenant).'
      : 'Browser storage is unavailable. Preferences reset when this tab closes.';
  });

  onThemeChange(): void {
    this.preferences.updateTheme(this.model().theme);
  }

  onDensityChange(): void {
    this.preferences.updateTableDensity(this.model().tableDensity);
  }

  resetPreferences(): void {
    this.preferences.resetToDefaults();
  }
}
