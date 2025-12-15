import { Injectable, inject } from '@angular/core';

import type { IssueApiKeyRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';

import { AuthSessionService } from './auth-session.service';

export interface IssueTenantTokenParams {
    tenantId: string;
    clientId: string;
    environmentTag?: string | null;
    scopes?: string[];
    ttlHours?: number | null;
    label?: string | null;
    persistInSession?: boolean;
    fallbackExpiry?: string | null;
}

export interface IssueTenantTokenResult {
    storedInSession: boolean;
    token: string | null;
    expiresAt: string | null;
    raw: unknown;
}

@Injectable({ providedIn: 'root' })
export class TenantTokenEndpointService {
    private readonly apiClient = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly authSession = inject(AuthSessionService);

    async issueTenantToken(params: IssueTenantTokenParams): Promise<IssueTenantTokenResult> {
        const payload: IssueApiKeyRequest = {
            clientId: params.clientId,
            environmentTag: params.environmentTag ?? undefined,
            scopes: params.scopes && params.scopes.length ? params.scopes : undefined,
            ttlHours: params.ttlHours ?? undefined,
        };

        const response = await this.apiClient.issueTenantApiKey({ tenantId: params.tenantId }, payload);
        const token = this.extractToken(response);
        const fallbackExpiry = params.fallbackExpiry ?? null;
        let storedInSession = false;

        if (token && params.persistInSession) {
            const expiresAt = fallbackExpiry ?? token.expiresAt ?? null;
            this.authSession.storeApiKey(token.value, {
                expiresAt,
                label: params.label ?? null,
            });
            storedInSession = true;
        }

        return {
            storedInSession,
            token: token?.value ?? null,
            expiresAt: token?.expiresAt ?? fallbackExpiry,
            raw: response,
        };
    }

    private extractToken(response: unknown): { value: string; expiresAt: string | null } | null {
        if (!response || typeof response !== 'object') {
            return null;
        }
        const candidate = response as Record<string, unknown>;
        const rawValue = this.pickTokenValue(candidate);
        if (!rawValue) {
            return null;
        }
        const expiresAt = typeof candidate.expiresAt === 'string' ? candidate.expiresAt : null;
        return { value: rawValue, expiresAt };
    }

    private pickTokenValue(source: Record<string, unknown>): string | null {
        const candidates: Array<unknown> = [source['token'], source['apiKey'], source['value']];
        const match = candidates.find((entry) => typeof entry === 'string' && entry.trim().length > 0);
        return (match as string | undefined)?.trim() ?? null;
    }
}
