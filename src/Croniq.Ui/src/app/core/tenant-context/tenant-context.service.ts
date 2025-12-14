import { Injectable, computed, effect, inject, signal } from '@angular/core';

import { CallerContext, CroniqRequestOptions } from 'data-access';

import { OperatorSession } from '../auth/operator-session';
import { TenantDirectoryService } from './tenant-directory.service';
import { TenantContextState, TenantEnvironment, TenantPreset } from './tenant-context.types';

const DEFAULT_TENANT_CONTEXT: TenantContextState = {
    tenantId: 'cron-lab',
    tenantName: 'Cron Lab',
    environment: 'staging',
    region: 'us-east-1',
    blueprintVersion: 'v2025.12.02',
    policyCount: 7,
    lastAuditedAt: new Date(Date.now() - 1000 * 60 * 12).toISOString(),
    featureFlags: ['webhooks-beta', 'deferred-tenants', 'legacy-fallbacks'],
    source: 'Croniq.Ui',
};

const TENANT_STORAGE_KEY = 'croniq.ui.tenant-context';

@Injectable({ providedIn: 'root' })
export class TenantContextService {
    private readonly operatorSession = inject(OperatorSession);
    private readonly tenantDirectory = inject(TenantDirectoryService);
    private readonly state = signal<TenantContextState>(loadStoredTenantContext() ?? DEFAULT_TENANT_CONTEXT);

    constructor() {
        effect(() => {
            this.syncStateWithPresets(this.tenantDirectory.presets());
        });
    }

    readonly snapshot = this.state.asReadonly();
    readonly tenantLabel = computed(() => {
        const ctx = this.state();
        return `${ctx.tenantName} · ${ctx.environment}`;
    });
    readonly tenantId = computed(() => this.state().tenantId);
    readonly environment = computed(() => this.state().environment);
    readonly featureFlags = computed(() => this.state().featureFlags);
    readonly presets = this.tenantDirectory.presets;

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

    applyPreset(tenantId: string): void {
        const preset = this.tenantDirectory.presets().find((entry) => entry.id === tenantId);
        if (!preset) {
            return;
        }
        this.state.update((current) => {
            const next: TenantContextState = {
                ...current,
                tenantId: preset.id,
                tenantName: preset.tenantName,
                environment: preset.defaultEnvironment,
                region: preset.region,
                blueprintVersion: preset.blueprintVersion,
                policyCount: preset.policyCount,
                featureFlags: [...preset.featureFlags],
                source: preset.source,
                lastAuditedAt: new Date().toISOString(),
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

    private syncStateWithPresets(presets: ReadonlyArray<TenantPreset>): void {
        if (!presets.length) {
            return;
        }
        const current = this.state();
        const preset = presets.find((entry) => entry.id === current.tenantId);
        if (!preset) {
            this.applyPreset(presets[0].id);
            return;
        }
        if (!isTenantEnvironment(current.environment)) {
            this.setEnvironment(preset.defaultEnvironment);
        }
    }

    createCallerContext(command: string, overrides: Partial<CallerContext> = {}): CallerContext {
        const ctx = this.state();
        const operatorActor = this.operatorSession.telemetryActor();
        return {
            source: overrides.source ?? ctx.source,
            command,
            tenantId: overrides.tenantId ?? ctx.tenantId,
            environment: overrides.environment ?? ctx.environment,
            actor: overrides.actor ?? operatorActor,
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
