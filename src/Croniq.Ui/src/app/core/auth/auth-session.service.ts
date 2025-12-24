import { Injectable, computed, effect, signal } from '@angular/core';
import { epochMsFromIso, nowIso, nowMs, tryIsoFromUnknown } from '@core/time/clock';
import type { CroniqCredentialSupplier } from 'data-access';

interface StoredSecret {
    value: string;
    expiresAt?: string | null;
    passwordChangeRequired?: boolean;
    issuedAt: string;
    lastUpdatedAt: string;
}

interface SecretUpdateOptions {
    expiresAt?: string | Date | null;
    passwordChangeRequired?: boolean;
}

const STORAGE_KEYS = {
    sessionToken: 'croniq.auth.session-token',
    tenantId: 'croniq.auth.tenant-id',
};

@Injectable({ providedIn: 'root' })
export class AuthSessionService implements CroniqCredentialSupplier {
    private readonly sessionTokenSignal = signal<StoredSecret | null>(loadSecret(STORAGE_KEYS.sessionToken));
    private readonly refreshTokenSignal = signal<string | null>(null);
    private readonly tenantIdSignal = signal<string | null>(loadString(STORAGE_KEYS.tenantId));

    readonly sessionToken = this.sessionTokenSignal.asReadonly();
    readonly sessionTokenExpired = computed(() => isSecretExpired(this.sessionTokenSignal()));
    readonly refreshToken = this.refreshTokenSignal.asReadonly();
    readonly tenantId = this.tenantIdSignal.asReadonly();
    readonly passwordChangeRequired = computed(() => Boolean(this.sessionTokenSignal()?.passwordChangeRequired));

    constructor() {
        effect(() => {
            if (this.sessionTokenExpired()) {
                this.clearSessionToken();
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

    clearAuthState(): void {
        this.clearSessionToken();
        this.clearRefreshToken();
        this.clearTenantId();
    }

    storeTenantId(value: string): void {
        const trimmed = value?.trim();
        if (!trimmed) {
            this.clearTenantId();
            return;
        }

        this.tenantIdSignal.set(trimmed);
        persistString(STORAGE_KEYS.tenantId, trimmed);
    }

    clearTenantId(): void {
        this.tenantIdSignal.set(null);
        clearString(STORAGE_KEYS.tenantId);
    }

    storeRefreshToken(value: string): void {
        const trimmed = value?.trim();
        this.refreshTokenSignal.set(trimmed ? trimmed : null);
    }

    clearRefreshToken(): void {
        this.refreshTokenSignal.set(null);
    }

    getSessionToken(): string | null {
        return this.sessionTokenSignal()?.value ?? null;
    }

    private createSecret(value: string, options: SecretUpdateOptions): StoredSecret {
        const expiresAt = options.expiresAt ? tryIsoFromUnknown(options.expiresAt) : null;
        const now = nowIso();
        return {
            value: value.trim(),
            expiresAt,
            passwordChangeRequired: options.passwordChangeRequired,
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

function loadString(key: string): string | null {
    if (!canUseSessionStorage()) {
        return null;
    }
    const raw = window.sessionStorage.getItem(key);
    if (!raw) {
        return null;
    }
    const trimmed = raw.trim();
    return trimmed ? trimmed : null;
}

function persistString(key: string, value: string): void {
    if (!canUseSessionStorage()) {
        return;
    }
    try {
        window.sessionStorage.setItem(key, value);
    } catch {
        // Ignore persistence failures so ephemeral sessions never break UX.
    }
}

function clearString(key: string): void {
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
    const expiry = epochMsFromIso(secret.expiresAt);
    return expiry != null ? expiry <= nowMs() : false;
}
