import { HttpBackend, HttpClient } from '@angular/common/http';
import { Injectable, inject, isDevMode } from '@angular/core';
import { catchError, map, of, tap, type Observable } from 'rxjs';
import {
    croniqUiRuntimeConfigSchema,
    resolveSwaggerUiUrl,
    type CroniqUiRuntimeConfig,
    type WebhookActivityStreamMode,
} from './api-config';

const DEFAULT_DEV_API_BASE_URL = 'http://localhost:5080';

@Injectable({ providedIn: 'root' })
export class RuntimeConfigService {
    // Use a raw HttpClient to avoid app interceptors during config bootstrap.
    private readonly http = new HttpClient(inject(HttpBackend));

    private config: CroniqUiRuntimeConfig = {};

    get snapshot(): CroniqUiRuntimeConfig {
        return this.config;
    }

    load(): Observable<void> {
        return this.http.get<unknown>('assets/croniq-config.json').pipe(
            map((raw) => croniqUiRuntimeConfigSchema.parse(raw)),
            tap((parsed) => {
                this.config = parsed;
            }),
            map(() => void 0),
            catchError((error) => {
                if (isDevMode()) {
                    console.warn('[Croniq.Ui] runtime config not loaded; falling back to defaults.', error);
                }
                this.config = {};
                return of(void 0);
            }),
        );
    }

    private defaultApiBaseUrl(): string {
        if (!isDevMode()) {
            return '';
        }
        const hostname = globalThis.location?.hostname;
        const isLocalhost = !hostname || hostname === 'localhost' || hostname === '127.0.0.1';
        return isLocalhost ? DEFAULT_DEV_API_BASE_URL : '';
    }

    get apiBaseUrl(): string {
        const resolved = this.config.apiBaseUrl?.trim() || this.defaultApiBaseUrl();
        return this.normalizeUrlLike(resolved);
    }

    get swaggerUiUrl(): string {
        return resolveSwaggerUiUrl(this.apiBaseUrl, this.config.swaggerUiUrl);
    }

    get defaultTenantId(): string {
        return this.config.defaultTenantId?.trim() ?? '';
    }

    get authMode(): 'password' | 'oidc' {
        const mode = this.config.auth?.mode?.trim().toLowerCase();
        return mode === 'oidc' ? 'oidc' : 'password';
    }

    get webhooksActivityStreamMode(): WebhookActivityStreamMode {
        const mode = this.config.webhooks?.activityStream?.mode?.trim().toLowerCase();
        if (mode === 'sse' || mode === 'polling' || mode === 'grpc') {
            return mode;
        }
        return 'grpc';
    }

    get webhooksActivityGrpcBaseUrl(): string {
        const raw = this.config.webhooks?.activityStream?.grpcBaseUrl;
        return raw ? this.normalizeUrlLike(raw) : '';
    }

    get webhooksActivitySseBaseUrl(): string {
        const raw = this.config.webhooks?.activityStream?.sseBaseUrl;
        if (raw) {
            return this.normalizeUrlLike(raw);
        }
        return this.apiBaseUrl;
    }

    private normalizeUrlLike(value: string): string {
        const trimmed = value.trim();
        if (!trimmed) {
            return '';
        }
        return trimmed.endsWith('/') ? trimmed.replace(/\/+$/, '') : trimmed;
    }
}
