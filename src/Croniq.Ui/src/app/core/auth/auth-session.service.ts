import { Injectable, computed, effect, signal } from '@angular/core';

import type { CroniqCredentialSupplier } from 'data-access';

import { epochMsFromIso, nowIso, nowMs, tryIsoFromUnknown } from '../time/clock';

interface StoredSecret {
    value: string;
    expiresAt?: string | null;
    issuedAt: string;
    lastUpdatedAt: string;
}

interface SecretUpdateOptions {
    expiresAt?: string | Date | null;
}

const STORAGE_KEYS = {
    sessionToken: 'croniq.auth.session-token',
};

@Injectable({ providedIn: 'root' })
export class AuthSessionService implements CroniqCredentialSupplier {
    private readonly sessionTokenSignal = signal<StoredSecret | null>(loadSecret(STORAGE_KEYS.sessionToken));
    private readonly refreshTokenSignal = signal<string | null>(null);

    readonly sessionToken = this.sessionTokenSignal.asReadonly();
    readonly sessionTokenExpired = computed(() => isSecretExpired(this.sessionTokenSignal()));
    readonly refreshToken = this.refreshTokenSignal.asReadonly();

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
        this.refreshTokenSignal.set(null);
        clearSecret(STORAGE_KEYS.sessionToken);
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
    const expiry = epochMsFromIso(secret.expiresAt);
    return expiry != null ? expiry <= nowMs() : false;
}
