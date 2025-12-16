import { HttpClient } from '@angular/common/http';
import { Injectable, inject, isDevMode } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import {
    croniqUiRuntimeConfigSchema,
    resolveSwaggerUiUrl,
    type CroniqUiRuntimeConfig,
} from './api-config';

const DEFAULT_DEV_API_BASE_URL = 'http://localhost:5000';

@Injectable({ providedIn: 'root' })
export class RuntimeConfigService {
    private readonly http = inject(HttpClient);

    private config: CroniqUiRuntimeConfig = {};

    async load(): Promise<void> {
        try {
            const raw = await firstValueFrom(this.http.get<unknown>('assets/croniq-config.json'));
            this.config = croniqUiRuntimeConfigSchema.parse(raw);
        } catch (error) {
            if (isDevMode()) {
                console.warn('[Croniq.Ui] runtime config not loaded; falling back to defaults.', error);
            }
            this.config = {};
        }
    }

    private defaultApiBaseUrl(): string {
        const hostname = globalThis.location?.hostname;
        const isLocalhost = !hostname || hostname === 'localhost' || hostname === '127.0.0.1';
        return isLocalhost ? DEFAULT_DEV_API_BASE_URL : '';
    }

    get apiBaseUrl(): string {
        const resolved = this.config.apiBaseUrl?.trim() || this.defaultApiBaseUrl();
        return resolved.endsWith('/') ? resolved.replace(/\/+$/, '') : resolved;
    }

    get swaggerUiUrl(): string {
        return resolveSwaggerUiUrl(this.apiBaseUrl, this.config.swaggerUiUrl);
    }
}
