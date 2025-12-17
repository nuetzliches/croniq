import { Injectable, computed, effect, inject, signal } from '@angular/core';

import { CallerContext, CroniqRequestOptions } from 'data-access';

import { nowIso } from '../time/clock';
import { TenantContextState, TenantEnvironment } from './tenant-context.types';

const DEFAULT_TENANT_CONTEXT: TenantContextState = {
    tenantId: '',
    tenantName: '',
    environment: 'staging',
    region: '',
    blueprintVersion: '',
    policyCount: 0,
    lastAuditedAt: nowIso(),
    featureFlags: [],
    source: 'manual',
};

const TENANT_STORAGE_KEY = 'croniq.ui.tenant-context';

@Injectable({ providedIn: 'root' })
export class TenantContextService {
    private readonly state = signal<TenantContextState>(loadStoredTenantContext() ?? DEFAULT_TENANT_CONTEXT);

    constructor() {
        // Intentionally empty: tenant presets were removed.
        // Tenant context is now operator-controlled and/or API-backed.
        effect(() => {
            // keep effect hook so future derived syncing can be added without changing structure
            void this.state();
        });
    }

    readonly snapshot = this.state.asReadonly();
    readonly tenantLabel = computed(() => {
        const ctx = this.state();
        const name = ctx.tenantName?.trim() || ctx.tenantId?.trim() || '—';
        return `${name} · ${ctx.environment}`;
    });
    readonly tenantId = computed(() => this.state().tenantId);
    readonly environment = computed(() => this.state().environment);
    readonly featureFlags = computed(() => this.state().featureFlags);

    updateContext(patch: Partial<TenantContextState>): void {
        this.state.update((current) => {
            const next: TenantContextState = {
                ...current,
                ...patch,
                lastAuditedAt: patch.lastAuditedAt ?? current.lastAuditedAt,
                featureFlags: patch.featureFlags ? [...patch.featureFlags] : current.featureFlags,
            };
            persistTenantContext(next);
            return next;
        });
    }

    setTenantIdentity(tenantId: string, tenantName?: string | null): void {
        const normalizedId = tenantId.trim();
        const normalizedName = tenantName?.trim() || '';
        this.state.update((current) => {
            const next: TenantContextState = {
                ...current,
                tenantId: normalizedId,
                tenantName: normalizedName,
                source: 'manual',
                lastAuditedAt: nowIso(),
            };
            persistTenantContext(next);
            return next;
        });
    }

    setEnvironment(environment: TenantEnvironment): void {
        this.state.update((current) => {
            if (current.environment === environment) {
                return current;
            }
            const next: TenantContextState = { ...current, environment };
            persistTenantContext(next);
            return next;
        });
    }

    addFeatureFlag(flag: string): void {
        const normalized = flag.trim();
        if (!normalized) {
            return;
        }
        this.state.update((current) => {
            if (current.featureFlags.includes(normalized)) {
                return current;
            }
            const next: TenantContextState = {
                ...current,
                featureFlags: [...current.featureFlags, normalized],
            };
            persistTenantContext(next);
            return next;
        });
    }

    removeFeatureFlag(flag: string): void {
        this.state.update((current) => {
            if (!current.featureFlags.includes(flag)) {
                return current;
            }
            const next: TenantContextState = {
                ...current,
                featureFlags: current.featureFlags.filter((entry) => entry !== flag),
            };
            persistTenantContext(next);
            return next;
        });
    }

    createCallerContext(command: string, overrides: Partial<CallerContext> = {}): CallerContext {
        const ctx = this.state();
        return {
            source: overrides.source ?? ctx.source,
            command,
            tenantId: overrides.tenantId ?? ctx.tenantId,
            environment: overrides.environment ?? ctx.environment,
            actor: overrides.actor ?? 'ui',
        };
    }

    createRequestOptions(command: string, overrides?: Partial<CallerContext>): CroniqRequestOptions {
        return {
            context: this.createCallerContext(command, overrides),
        };
    }
}

function canUseStorage(): boolean {
    return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function loadStoredTenantContext(): TenantContextState | null {
    if (!canUseStorage()) {
        return null;
    }
    const rawValue = window.localStorage.getItem(TENANT_STORAGE_KEY);
    if (!rawValue) {
        return null;
    }
    try {
        const parsed = JSON.parse(rawValue) as Partial<TenantContextState>;
        return {
            ...DEFAULT_TENANT_CONTEXT,
            ...parsed,
            environment: ensureTenantEnvironment(parsed.environment) ?? DEFAULT_TENANT_CONTEXT.environment,
            featureFlags: normalizeFeatureFlags(parsed.featureFlags),
        };
    } catch {
        return null;
    }
}

function ensureTenantEnvironment(value: unknown): TenantEnvironment | null {
    return isTenantEnvironment(value) ? value : null;
}

function isTenantEnvironment(value: unknown): value is TenantEnvironment {
    return value === 'dev' || value === 'staging' || value === 'production';
}

function normalizeFeatureFlags(value: unknown): ReadonlyArray<string> {
    if (!Array.isArray(value)) {
        return DEFAULT_TENANT_CONTEXT.featureFlags;
    }
    return value
        .map((entry) => (typeof entry === 'string' ? entry.trim() : ''))
        .filter((entry) => entry.length > 0);
}

function persistTenantContext(state: TenantContextState): void {
    if (!canUseStorage()) {
        return;
    }
    try {
        window.localStorage.setItem(TENANT_STORAGE_KEY, JSON.stringify(state));
    } catch {
        // ignore persistence failures
    }
}
