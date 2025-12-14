import { HttpClient } from '@angular/common/http';
import { Injectable, inject, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { TenantPreset } from './tenant-context.types';

const FALLBACK_PRESETS: ReadonlyArray<TenantPreset> = [
    {
        id: 'cron-lab',
        label: 'Cron Lab',
        tenantName: 'Cron Lab',
        defaultEnvironment: 'staging',
        region: 'us-east-1',
        blueprintVersion: 'v2025.12.02',
        policyCount: 7,
        featureFlags: ['webhooks-beta', 'deferred-tenants', 'legacy-fallbacks'],
        source: 'Croniq.Ui',
    },
    {
        id: 'northwind-labs',
        label: 'Northwind Labs',
        tenantName: 'Northwind Labs',
        defaultEnvironment: 'production',
        region: 'eu-central-1',
        blueprintVersion: 'v2025.11.18',
        policyCount: 12,
        featureFlags: ['webhooks-beta'],
        source: 'Croniq.Ui',
    },
    {
        id: 'legacy-east',
        label: 'Legacy East',
        tenantName: 'Legacy East',
        defaultEnvironment: 'dev',
        region: 'us-east-2',
        blueprintVersion: 'v2025.07.04',
        policyCount: 4,
        featureFlags: ['legacy-fallbacks'],
        source: 'CLI',
    },
];

@Injectable({ providedIn: 'root' })
export class TenantDirectoryService {
    private readonly http = inject(HttpClient);
    private readonly presetsSignal = signal<ReadonlyArray<TenantPreset>>(FALLBACK_PRESETS);
    private readonly loadingSignal = signal(false);

    readonly presets = this.presetsSignal.asReadonly();
    readonly loading = this.loadingSignal.asReadonly();

    constructor() {
        queueMicrotask(() => {
            void this.refresh();
        });
    }

    async refresh(): Promise<void> {
        if (this.loadingSignal()) {
            return;
        }
        this.loadingSignal.set(true);
        try {
            const payload = await firstValueFrom(this.http.get<unknown>('assets/tenant-presets.json'));
            const parsed = normalizeTenantPresets(payload);
            if (parsed.length) {
                this.presetsSignal.set(parsed);
            }
        } catch (error) {
            console.error('Unable to load tenant presets', error);
        } finally {
            this.loadingSignal.set(false);
        }
    }
}

function normalizeTenantPresets(value: unknown): ReadonlyArray<TenantPreset> {
    if (!Array.isArray(value)) {
        return FALLBACK_PRESETS;
    }
    const parsed: TenantPreset[] = [];
    value.forEach((entry, index) => {
        if (!entry || typeof entry !== 'object') {
            return;
        }
        const record = entry as Record<string, unknown>;
        const id = typeof record['id'] === 'string' ? record['id'] : `tenant-${index}`;
        const label = typeof record['label'] === 'string' ? record['label'] : id;
        const tenantName = typeof record['tenantName'] === 'string' ? record['tenantName'] : label;
        const defaultEnvironment = parseEnvironment(record['defaultEnvironment']);
        parsed.push({
            id,
            label,
            tenantName,
            defaultEnvironment,
            region: typeof record['region'] === 'string' ? record['region'] : 'unknown',
            blueprintVersion:
                typeof record['blueprintVersion'] === 'string' ? record['blueprintVersion'] : 'v0.0.0',
            policyCount: typeof record['policyCount'] === 'number' ? record['policyCount'] : 0,
            featureFlags: parseFeatureFlags(record['featureFlags']),
            source: typeof record['source'] === 'string' ? record['source'] : 'Croniq.Ui',
        });
    });
    return parsed.length ? parsed : FALLBACK_PRESETS;
}

function parseFeatureFlags(value: unknown): ReadonlyArray<string> {
    if (!Array.isArray(value)) {
        return [];
    }
    return value
        .map((entry) => (typeof entry === 'string' ? entry.trim() : ''))
        .filter((entry) => entry.length > 0);
}

function parseEnvironment(value: unknown): TenantPreset['defaultEnvironment'] {
    if (value === 'dev' || value === 'staging' || value === 'production') {
        return value;
    }
    return 'staging';
}
