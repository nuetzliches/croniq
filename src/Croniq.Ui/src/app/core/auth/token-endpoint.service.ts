import { Injectable, inject } from '@angular/core';
import { z } from 'zod';

import type { IssueTokenRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';

import { tryIsoFromUnknown } from '../time/clock';
import { AuthSessionService } from './auth-session.service';

const issuedTenantTokenResponseSchema = z
    .preprocess((input) => {
        if (typeof input === 'string') {
            const trimmed = input.trim();
            return trimmed ? { value: trimmed } : input;
        }
        return input;
    }, z
        .object({
            token: z.string().trim().min(1).optional(),
            value: z.string().trim().min(1).optional(),
            accessToken: z.string().trim().min(1).optional(),
            apiKey: z.string().trim().min(1).optional(),
            expiresAt: z.unknown().optional(),
        })
        .passthrough()
        .superRefine((data, ctx) => {
            if (!data.token && !data.value && !data.accessToken && !data.apiKey) {
                ctx.addIssue({
                    code: z.ZodIssueCode.custom,
                    message: 'Expected token response to contain token/value/accessToken (legacy: apiKey).',
                });
            }
        })
        .transform((data) => ({
            value: (data.token ?? data.value ?? data.accessToken ?? data.apiKey) as string,
            expiresAt: tryIsoFromUnknown(data.expiresAt),
        }))
    );

type IssuedTenantToken = z.infer<typeof issuedTenantTokenResponseSchema>;

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
        const fallbackExpiry = tryIsoFromUnknown(params.fallbackExpiry);
        const resolvedExpiry = token?.expiresAt ?? fallbackExpiry;
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

    private extractToken(response: unknown): IssuedTenantToken | null {
        const parsed = issuedTenantTokenResponseSchema.safeParse(response);
        return parsed.success ? parsed.data : null;
    }
}
