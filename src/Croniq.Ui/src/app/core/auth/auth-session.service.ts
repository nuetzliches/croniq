import { Injectable, computed, effect, signal } from '@angular/core';

import type { CroniqCredentialSupplier } from 'data-access';

interface StoredSecret {
    value: string;
    expiresAt?: string | null;
    label?: string | null;
    issuedAt: string;
    lastUpdatedAt: string;
}

interface SecretUpdateOptions {
    expiresAt?: string | Date | null;
    label?: string | null;
}

const STORAGE_KEYS = {
    sessionToken: 'croniq.auth.session-token',
    apiKey: 'croniq.auth.api-key',
};

@Injectable({ providedIn: 'root' })
export class AuthSessionService implements CroniqCredentialSupplier {
    private readonly sessionTokenSignal = signal<StoredSecret | null>(loadSecret(STORAGE_KEYS.sessionToken));
    private readonly apiKeySignal = signal<StoredSecret | null>(loadSecret(STORAGE_KEYS.apiKey));

    readonly sessionToken = this.sessionTokenSignal.asReadonly();
    readonly apiKey = this.apiKeySignal.asReadonly();
    readonly sessionTokenExpired = computed(() => isSecretExpired(this.sessionTokenSignal()));
    readonly apiKeyExpired = computed(() => isSecretExpired(this.apiKeySignal()));

    constructor() {
        effect(() => {
            if (this.sessionTokenExpired()) {
                this.clearSessionToken();
            }
        });
        effect(() => {
            if (this.apiKeyExpired()) {
                this.clearApiKey();
            }
        });
    }

    storeSessionToken(value: string, options: SecretUpdateOptions = {}): void {
        if (!value || !value.trim()) {
            this.clearSessionToken();
            return;
        }
        const payload = this.createSecret(value, options);
        this.sessionTokenSignal.set(payload);
        persistSecret(STORAGE_KEYS.sessionToken, payload);
    }

    clearSessionToken(): void {
        this.sessionTokenSignal.set(null);
        clearSecret(STORAGE_KEYS.sessionToken);
    }

    storeApiKey(value: string, options: SecretUpdateOptions = {}): void {
        if (!value || !value.trim()) {
            this.clearApiKey();
            return;
        }
        const payload = this.createSecret(value, options);
        this.apiKeySignal.set(payload);
        persistSecret(STORAGE_KEYS.apiKey, payload);
    }

    clearApiKey(): void {
        this.apiKeySignal.set(null);
        clearSecret(STORAGE_KEYS.apiKey);
    }

    async startOidcBootstrap(): Promise<void> {
        // Placeholder for PKCE/OIDC handshake. This keeps the hook in place so we can
        // swap the manual token input for a standards-based login once backend is ready.
        await Promise.resolve();
    }

    getSessionToken(): string | null {
        return this.sessionTokenSignal()?.value ?? null;
    }

    getApiKey(): string | null {
        return this.apiKeySignal()?.value ?? null;
    }

    private createSecret(value: string, options: SecretUpdateOptions): StoredSecret {
        const expiresAt = options.expiresAt
            ? typeof options.expiresAt === 'string'
                ? options.expiresAt
                : options.expiresAt.toISOString()
            : null;
        const now = new Date().toISOString();
        return {
            value: value.trim(),
            expiresAt,
            label: options.label?.trim() || null,
            issuedAt: now,
            lastUpdatedAt: now,
        };
    }
}

function canUseSessionStorage(): boolean {
    return typeof window !== 'undefined' && typeof window.sessionStorage !== 'undefined';
}

function loadSecret(key: string): StoredSecret | null {
    if (!canUseSessionStorage()) {
        return null;
    }
    const raw = window.sessionStorage.getItem(key);
    if (!raw) {
        return null;
    }
    try {
        const parsed = JSON.parse(raw) as StoredSecret;
        return parsed.value ? parsed : null;
    } catch {
        return null;
    }
}

function persistSecret(key: string, secret: StoredSecret): void {
    if (!canUseSessionStorage()) {
        return;
    }
    try {
        window.sessionStorage.setItem(key, JSON.stringify(secret));
    } catch {
        // Ignore persistence failures so ephemeral sessions never break UX.
    }
}

function clearSecret(key: string): void {
    if (!canUseSessionStorage()) {
        return;
    }
    try {
        window.sessionStorage.removeItem(key);
    } catch {
        // Ignore clear errors.
    }
}

function isSecretExpired(secret: StoredSecret | null): boolean {
    if (!secret?.expiresAt) {
        return false;
    }
    const expiry = Date.parse(secret.expiresAt);
    return Number.isFinite(expiry) ? expiry <= Date.now() : false;
}
