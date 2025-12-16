import { Injectable, computed, signal } from '@angular/core';

import { isoFromEpochMs, nowIso, nowMs } from '../time/clock';

export type OperatorScope =
    | 'schedules:read'
    | 'schedules:write'
    | 'jobs:trigger'
    | 'webhooks:manage'
    | 'tenants:rotate-keys'
    | 'tenants:read'
    | 'telemetry:read';

export type OperatorProfile = {
    id: string;
    displayName: string;
    email: string;
    avatarUrl?: string;
    scopes: ReadonlyArray<OperatorScope>;
    lastAuthenticatedAt: string;
    impersonating?: boolean;
};

const DEFAULT_OPERATOR: OperatorProfile = {
    id: 'ops.matta',
    displayName: 'Matta Rivera',
    email: 'matta.rivera@croniq.dev',
    scopes: ['schedules:read', 'jobs:trigger', 'webhooks:manage', 'tenants:rotate-keys', 'telemetry:read'],
    lastAuthenticatedAt: isoFromEpochMs(nowMs() - 1000 * 30),
};

const OPERATOR_STORAGE_KEY = 'croniq.ui.operator-profile';

@Injectable({ providedIn: 'root' })
export class OperatorSession {
    private readonly profileSignal = signal<OperatorProfile>(loadStoredOperatorProfile() ?? DEFAULT_OPERATOR);

    readonly profile = this.profileSignal.asReadonly();
    readonly telemetryActor = computed(() => {
        const profile = this.profileSignal();
        return profile.email || profile.id;
    });

    readonly scopeBadges = computed(() => this.profileSignal().scopes);

    updateProfile(patch: Partial<OperatorProfile>): void {
        this.profileSignal.update((current) => {
            const next: OperatorProfile = {
                ...current,
                ...patch,
                scopes: (patch.scopes ?? current.scopes).map((scope) => scope),
                lastAuthenticatedAt: patch.lastAuthenticatedAt ?? current.lastAuthenticatedAt,
            };
            persistOperatorProfile(next);
            return next;
        });
    }

    impersonate(displayName: string, email: string): void {
        this.profileSignal.update((current) => {
            const next: OperatorProfile = {
                ...current,
                displayName,
                email,
                impersonating: true,
                lastAuthenticatedAt: nowIso(),
            };
            persistOperatorProfile(next);
            return next;
        });
    }

    clearImpersonation(): void {
        this.profileSignal.update((current) => {
            const next: OperatorProfile = { ...current, impersonating: false };
            persistOperatorProfile(next);
            return next;
        });
    }
}

function canUseStorage(): boolean {
    return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function loadStoredOperatorProfile(): OperatorProfile | null {
    if (!canUseStorage()) {
        return null;
    }
    const rawProfile = window.localStorage.getItem(OPERATOR_STORAGE_KEY);
    if (!rawProfile) {
        return null;
    }
    try {
        const parsed = JSON.parse(rawProfile) as OperatorProfile;
        return {
            ...DEFAULT_OPERATOR,
            ...parsed,
            scopes: Array.isArray(parsed.scopes) && parsed.scopes.length ? parsed.scopes : DEFAULT_OPERATOR.scopes,
        };
    } catch {
        return null;
    }
}

function persistOperatorProfile(profile: OperatorProfile): void {
    if (!canUseStorage()) {
        return;
    }
    try {
        window.localStorage.setItem(OPERATOR_STORAGE_KEY, JSON.stringify(profile));
    } catch {
        // ignore persistence failures
    }
}
