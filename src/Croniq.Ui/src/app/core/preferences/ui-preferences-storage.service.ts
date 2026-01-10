import { Injectable, InjectionToken, inject, signal } from '@angular/core';
import { nowIso } from '@core/time/clock';
import type { UiPreferences, UiPreferencesRecord } from './ui-preferences.types';

export type PreferencesStorageMode = 'indexeddb' | 'memory';

export interface PreferencesCipher {
  encrypt(payload: string): Promise<string>;
  decrypt(payload: string): Promise<string>;
}

export const UI_PREFERENCES_CIPHER = new InjectionToken<PreferencesCipher>('UI_PREFERENCES_CIPHER');

const NOOP_CIPHER: PreferencesCipher = {
  encrypt: async (payload) => payload,
  decrypt: async (payload) => payload,
};

type StoredPreferencesRecord = {
  tenantId: string;
  payload: string;
  encrypted: boolean;
  updatedAt: string;
  version: number;
};

const DB_NAME = 'croniq-ui';
const DB_VERSION = 1;
const STORE_NAME = 'ui-preferences';
const RECORD_VERSION = 1;

@Injectable({ providedIn: 'root' })
export class UiPreferencesStorage {
  private readonly cipher = inject(UI_PREFERENCES_CIPHER, { optional: true }) ?? NOOP_CIPHER;
  private readonly modeSignal = signal<PreferencesStorageMode>(
    canUseIndexedDb() ? 'indexeddb' : 'memory',
  );
  private readonly memoryStore = new Map<string, StoredPreferencesRecord>();
  private dbPromise: Promise<IDBDatabase> | null =
    this.modeSignal() === 'indexeddb' ? openDatabase() : null;

  readonly mode = this.modeSignal.asReadonly();

  async load(tenantId: string): Promise<UiPreferencesRecord | null> {
    const normalizedId = tenantId.trim();
    if (!normalizedId) {
      return null;
    }

    const record = await this.readRecord(normalizedId);
    if (!record || record.version !== RECORD_VERSION) {
      return null;
    }

    const preferences = await this.decodePreferences(record);
    if (!preferences) {
      return null;
    }

    return {
      tenantId: record.tenantId,
      preferences,
      updatedAt: record.updatedAt,
    };
  }

  async save(tenantId: string, preferences: UiPreferences): Promise<string> {
    const normalizedId = tenantId.trim();
    if (!normalizedId) {
      return nowIso();
    }

    const updatedAt = nowIso();
    const payload = JSON.stringify(preferences);
    const encoded = await this.encodePayload(payload);
    const record: StoredPreferencesRecord = {
      tenantId: normalizedId,
      payload: encoded.payload,
      encrypted: encoded.encrypted,
      updatedAt,
      version: RECORD_VERSION,
    };

    await this.writeRecord(record);
    return updatedAt;
  }

  async clear(tenantId: string): Promise<void> {
    const normalizedId = tenantId.trim();
    if (!normalizedId) {
      return;
    }

    if (this.modeSignal() === 'memory') {
      this.memoryStore.delete(normalizedId);
      return;
    }

    const db = await this.openDb();
    if (!db) {
      this.memoryStore.delete(normalizedId);
      return;
    }

    try {
      const tx = db.transaction(STORE_NAME, 'readwrite');
      await requestToPromise(tx.objectStore(STORE_NAME).delete(normalizedId));
    } catch {
      this.fallbackToMemory();
      this.memoryStore.delete(normalizedId);
    }
  }

  private async openDb(): Promise<IDBDatabase | null> {
    if (this.modeSignal() !== 'indexeddb') {
      return null;
    }

    if (!this.dbPromise) {
      this.dbPromise = openDatabase();
    }

    try {
      return await this.dbPromise;
    } catch {
      this.fallbackToMemory();
      return null;
    }
  }

  private async readRecord(tenantId: string): Promise<StoredPreferencesRecord | null> {
    if (this.modeSignal() === 'memory') {
      return this.memoryStore.get(tenantId) ?? null;
    }

    const db = await this.openDb();
    if (!db) {
      return this.memoryStore.get(tenantId) ?? null;
    }

    try {
      const tx = db.transaction(STORE_NAME, 'readonly');
      const record = await requestToPromise(tx.objectStore(STORE_NAME).get(tenantId));
      return record ?? null;
    } catch {
      this.fallbackToMemory();
      return this.memoryStore.get(tenantId) ?? null;
    }
  }

  private async writeRecord(record: StoredPreferencesRecord): Promise<void> {
    if (this.modeSignal() === 'memory') {
      this.memoryStore.set(record.tenantId, record);
      return;
    }

    const db = await this.openDb();
    if (!db) {
      this.memoryStore.set(record.tenantId, record);
      return;
    }

    try {
      const tx = db.transaction(STORE_NAME, 'readwrite');
      await requestToPromise(tx.objectStore(STORE_NAME).put(record));
    } catch {
      this.fallbackToMemory();
      this.memoryStore.set(record.tenantId, record);
    }
  }

  private async decodePreferences(record: StoredPreferencesRecord): Promise<UiPreferences | null> {
    const raw = await this.decodePayload(record);
    if (!raw) {
      return null;
    }

    try {
      return JSON.parse(raw) as UiPreferences;
    } catch {
      return null;
    }
  }

  private async decodePayload(record: StoredPreferencesRecord): Promise<string | null> {
    if (!record.encrypted) {
      return record.payload;
    }

    if (this.cipher === NOOP_CIPHER) {
      return null;
    }

    try {
      return await this.cipher.decrypt(record.payload);
    } catch {
      return null;
    }
  }

  private async encodePayload(payload: string): Promise<{ payload: string; encrypted: boolean }> {
    if (this.cipher === NOOP_CIPHER) {
      return { payload, encrypted: false };
    }

    try {
      const encrypted = await this.cipher.encrypt(payload);
      return { payload: encrypted, encrypted: true };
    } catch {
      return { payload, encrypted: false };
    }
  }

  private fallbackToMemory(): void {
    if (this.modeSignal() === 'memory') {
      return;
    }
    this.modeSignal.set('memory');
    this.dbPromise = null;
  }
}

function canUseIndexedDb(): boolean {
  return typeof window !== 'undefined' && typeof window.indexedDB !== 'undefined';
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: 'tenantId' });
      }
    };

    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error('Unable to open IndexedDB for UI preferences.'));
  });
}

function requestToPromise<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error('IndexedDB request failed for UI preferences.'));
  });
}
