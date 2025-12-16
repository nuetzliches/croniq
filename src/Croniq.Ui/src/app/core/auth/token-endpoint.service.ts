import { Injectable, inject } from '@angular/core';

import type { IssueTokenRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';

import { tryIsoFromUnknown } from '../time/clock';
import { AuthSessionService } from './auth-session.service';

export interface IssueTenantTokenParams {
    tenantId: string;
    clientId: string;
    environmentTag?: string | null;
    scopes?: string[];
    ttlHours?: number | null;
    audience?: string | null;
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
        const payload: IssueTokenRequest = {
            clientId: params.clientId,
            scopes: params.scopes && params.scopes.length ? params.scopes : null,
            audience: params.audience ?? null,
            ttlMinutes: typeof params.ttlHours === 'number' ? params.ttlHours * 60 : null,
        };

        const response = await this.apiClient.issueTenantToken(
            { tenantId: params.tenantId, environment: params.environmentTag ?? undefined },
            payload,
        );
        const token = this.extractToken(response);
        const fallbackExpiry = params.fallbackExpiry ?? null;
        const resolvedExpiry = tryIsoFromUnknown(token?.expiresAt ?? fallbackExpiry);
        let storedInSession = false;

        if (token && params.persistInSession) {
            this.authSession.storeSessionToken(token.value, { expiresAt: resolvedExpiry });
            storedInSession = true;
        }

        return {
            storedInSession,
            token: token?.value ?? null,
            expiresAt: resolvedExpiry,
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
        const expiresAt = tryIsoFromUnknown(candidate['expiresAt']);
        return { value: rawValue, expiresAt };
    }

    private pickTokenValue(source: Record<string, unknown>): string | null {
        const candidates: Array<unknown> = [source['token'], source['apiKey'], source['value']];
        const match = candidates.find((entry) => typeof entry === 'string' && entry.trim().length > 0);
        return (match as string | undefined)?.trim() ?? null;
    }
}
