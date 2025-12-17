import { Injectable, inject } from '@angular/core';
import { z } from 'zod';

import type { PasswordLoginRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';

import { tryIsoFromUnknown } from '../time/clock';
import { AuthSessionService } from './auth-session.service';

const passwordLoginResponseSchema = z
    .preprocess((input) => {
        if (typeof input === 'string') {
            const trimmed = input.trim();
            return trimmed ? { accessToken: trimmed } : input;
        }
        return input;
    },
        z
            .object({
                accessToken: z.string().trim().min(1).optional(),
                token: z.string().trim().min(1).optional(),
                value: z.string().trim().min(1).optional(),
                refreshToken: z.string().trim().min(1).optional(),
                expiresAt: z.unknown().optional(),
                // Backend currently returns `expiresIn` (seconds). Keep `expiresInSeconds` for compatibility.
                expiresIn: z.number().int().positive().optional(),
                expiresInSeconds: z.number().int().positive().optional(),
                tenantReference: z.string().trim().min(1).optional().nullable(),
                tenantId: z.string().trim().min(1).optional().nullable(),
            })
            .passthrough()
            .superRefine((data, ctx) => {
                if (!data.accessToken && !data.token && !data.value) {
                    ctx.addIssue({
                        code: z.ZodIssueCode.custom,
                        message: 'Expected login response to contain accessToken/token/value.',
                    });
                }
            })
            .transform((data) => {
                const resolvedAccessToken = (data.accessToken ?? data.token ?? data.value) as string;
                const expiryFromField = tryIsoFromUnknown(data.expiresAt);
                const expiresInSeconds = data.expiresInSeconds ?? data.expiresIn;
                const expiryFromSeconds =
                    !expiryFromField && typeof expiresInSeconds === 'number'
                        ? new Date(Date.now() + expiresInSeconds * 1000).toISOString()
                        : null;
                return {
                    accessToken: resolvedAccessToken,
                    refreshToken: data.refreshToken ?? null,
                    expiresAt: expiryFromField ?? expiryFromSeconds,
                    tenantReference: (data.tenantReference ?? data.tenantId ?? null) as string | null,
                    raw: data as unknown,
                };
            }),
    );

type PasswordLoginResponse = z.infer<typeof passwordLoginResponseSchema>;

export interface PasswordLoginParams {
    username: string;
    password: string;
    tenantReference?: string | null;
    scopes?: string[];
    audience?: string | null;
}

export interface PasswordLoginResult {
    storedInSession: boolean;
    token: string;
    expiresAt: string | null;
    refreshTokenPresent: boolean;
    tenantId: string | null;
    tenantReference: string | null;
    raw: unknown;
}

@Injectable({ providedIn: 'root' })
export class PasswordAuthService {
    private readonly apiClient = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly authSession = inject(AuthSessionService);

    async login(params: PasswordLoginParams): Promise<PasswordLoginResult> {
        const payload: PasswordLoginRequest = {
            username: params.username,
            password: params.password,
            // Per new auth concept: tenant/environment are server-configured.
            tenantReference: params.tenantReference ?? null,
            environmentTag: null,
            scopes: params.scopes && params.scopes.length ? params.scopes : null,
            audience: params.audience ?? null,
        };

        const response = await this.apiClient.passwordLogin(payload);
        const parsed = this.extract(response);
        if (!parsed) {
            throw new Error('Login failed: unsupported response shape (missing access token).');
        }

        this.authSession.storeSessionToken(parsed.accessToken, { expiresAt: parsed.expiresAt });
        if (parsed.refreshToken) {
            this.authSession.storeRefreshToken(parsed.refreshToken);
        } else {
            this.authSession.clearRefreshToken();
        }

        const tenantId = tryExtractTenantIdFromJwt(parsed.accessToken);

        return {
            storedInSession: true,
            token: parsed.accessToken,
            expiresAt: parsed.expiresAt ?? null,
            refreshTokenPresent: Boolean(parsed.refreshToken),
            tenantId,
            tenantReference: parsed.tenantReference ?? null,
            raw: parsed.raw,
        };
    }

    private extract(response: unknown): PasswordLoginResponse | null {
        const parsed = passwordLoginResponseSchema.safeParse(response);
        return parsed.success ? parsed.data : null;
    }
}

function tryExtractTenantIdFromJwt(token: string): string | null {
    const trimmed = token.trim();
    const parts = trimmed.split('.');
    if (parts.length !== 3) {
        return null;
    }

    const payloadJson = base64UrlDecodeToString(parts[1]);
    if (!payloadJson) {
        return null;
    }

    try {
        const payload = JSON.parse(payloadJson) as Record<string, unknown>;
        const tenant = payload['tenant'];
        return typeof tenant === 'string' && tenant.trim().length > 0 ? tenant.trim() : null;
    } catch {
        return null;
    }
}

function base64UrlDecodeToString(value: string): string | null {
    const normalized = value.replace(/-/g, '+').replace(/_/g, '/');
    const padLength = (4 - (normalized.length % 4)) % 4;
    const padded = normalized + '='.repeat(padLength);
    try {
        return atob(padded);
    } catch {
        return null;
    }
}
