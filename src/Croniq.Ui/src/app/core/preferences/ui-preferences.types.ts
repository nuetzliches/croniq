export const UI_THEME_VALUES = ['ops-dark', 'ops-light'] as const;
export type UiThemePreference = (typeof UI_THEME_VALUES)[number];

export const UI_TABLE_DENSITY_VALUES = ['comfortable', 'compact'] as const;
export type UiTableDensityPreference = (typeof UI_TABLE_DENSITY_VALUES)[number];

export interface UiPreferences {
  theme: UiThemePreference;
  tableDensity: UiTableDensityPreference;
}

export interface UiPreferencesRecord {
  tenantId: string;
  preferences: UiPreferences;
  updatedAt: string;
}

export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  theme: 'ops-dark',
  tableDensity: 'comfortable',
};

export function isUiThemePreference(value: unknown): value is UiThemePreference {
  return typeof value === 'string' && UI_THEME_VALUES.includes(value as UiThemePreference);
}

export function isUiTableDensityPreference(value: unknown): value is UiTableDensityPreference {
  return typeof value === 'string' && UI_TABLE_DENSITY_VALUES.includes(value as UiTableDensityPreference);
}

export function normalizeUiPreferences(
  value: Partial<UiPreferences> | null | undefined,
): UiPreferences {
  return {
    theme: isUiThemePreference(value?.theme) ? value!.theme : DEFAULT_UI_PREFERENCES.theme,
    tableDensity: isUiTableDensityPreference(value?.tableDensity)
      ? value!.tableDensity
      : DEFAULT_UI_PREFERENCES.tableDensity,
  };
}
