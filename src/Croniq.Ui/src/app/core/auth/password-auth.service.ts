import { Injectable, inject } from '@angular/core';
import { tryIsoFromUnknown } from '@core/time/clock';
import type { PasswordChangePasswordRequest, PasswordLoginRequest, PasswordLogoutRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, type CroniqApiClient } from 'data-access';
import type { Observable } from 'rxjs';
import { catchError, map, of, throwError } from 'rxjs';
import { z } from 'zod';
import { AuthLogoutCleanupService } from './auth-logout-cleanup.service';
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
                tenantId: z.string().trim().min(1).optional().nullable(),
                passwordChangeRequired: z.boolean().optional(),
            })
            .passthrough()
            .superRefine((data, ctx) => {
                if (!data.accessToken && !data.token && !data.value) {
                    ctx.addIssue({
                        code: 'custom',
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
                    tenantId: (data.tenantId ?? null) as string | null,
                    passwordChangeRequired: Boolean(data.passwordChangeRequired),
                    raw: data as unknown,
                };
            }),
    );

type PasswordLoginResponse = z.infer<typeof passwordLoginResponseSchema>;

export interface PasswordLoginParams {
    username: string;
    password: string;
    tenantId: string;
    scopes?: string[];
    audience?: string | null;
}

export interface PasswordLoginResult {
    storedInSession: boolean;
    token: string;
    expiresAt: string | null;
    refreshTokenPresent: boolean;
    passwordChangeRequired: boolean;
    tenantId: string | null;
    raw: unknown;
}

export interface PasswordRefreshResult {
    storedInSession: boolean;
    token: string;
    expiresAt: string | null;
    refreshTokenPresent: boolean;
    passwordChangeRequired: boolean;
    raw: unknown;
}

export interface PasswordChangePasswordParams {
    currentPassword: string;
    newPassword: string;
}

@Injectable({ providedIn: 'root' })
export class PasswordAuthService {
    private readonly apiClient = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly authSession = inject(AuthSessionService);
    private readonly authCleanup = inject(AuthLogoutCleanupService);

    login(params: PasswordLoginParams): Observable<PasswordLoginResult> {
        const payload: PasswordLoginRequest = {
            username: params.username,
            password: params.password,
            tenantId: params.tenantId,
            environmentTag: null,
            scopes: params.scopes && params.scopes.length ? params.scopes : null,
            audience: params.audience ?? null,
        };

        return this.apiClient.passwordLogin(payload).pipe(
            map((response) => {
                const parsed = this.extract(response);
                if (!parsed) {
                    throw new Error('Login failed: unsupported response shape (missing access token).');
                }

                this.authSession.storeSessionToken(parsed.accessToken, {
                    expiresAt: parsed.expiresAt,
                    passwordChangeRequired: parsed.passwordChangeRequired,
                });
                if (parsed.refreshToken) {
                    this.authSession.storeRefreshToken(parsed.refreshToken);
                }

                let tenantId = parsed.tenantId;
                if (!tenantId && parsed.accessToken) {
                    tenantId = extractTenantIdFromToken(parsed.accessToken);
                }

                if (!tenantId) {
                    throw new Error('Login failed: missing tenantId in response.');
                }

                this.authSession.storeTenantId(tenantId);

                const refreshTokenPresent = Boolean(parsed.refreshToken ?? this.authSession.refreshToken());

                return {
                    storedInSession: true,
                    token: parsed.accessToken,
                    expiresAt: parsed.expiresAt ?? null,
                    refreshTokenPresent,
                    passwordChangeRequired: parsed.passwordChangeRequired,
                    tenantId,
                    raw: parsed.raw,
                };
            }),
        );
    }

    logout(): Observable<void> {
        const refreshToken = this.authSession.refreshToken()?.trim() ?? '';
        const tenantId = this.authSession.tenantId()?.trim() ?? '';

        if (refreshToken) {
            if (!tenantId) {
                // Best-effort: if we can't resolve tenantId, clear local state without calling the server.
                this.authCleanup.clearAll();
                return of(undefined);
            }

            const payload: PasswordLogoutRequest = {
                refreshToken,
                tenantId,
            };

            return this.apiClient.passwordLogout(payload).pipe(
                catchError(() => of(undefined)),
                map(() => {
                    // Best-effort: even if the server rejects logout, we still clear local state.
                    this.authCleanup.clearAll();
                }),
            );
        }

        this.authCleanup.clearAll();

        return of(undefined);
    }

    refresh(): Observable<PasswordRefreshResult> {
        const refreshToken = this.authSession.refreshToken()?.trim() ?? '';
        if (!refreshToken) {
            return throwError(() => new Error('Refresh failed: missing refresh token.'));
        }

        const tenantId = this.authSession.tenantId()?.trim() ?? '';
        if (!tenantId) {
            return throwError(() => new Error('Refresh failed: missing tenantId.'));
        }

        return this.apiClient
            .passwordRefresh({
                refreshToken,
                tenantId,
                environmentTag: null,
                scopes: null,
                audience: null,
            })
            .pipe(
                map((response) => {
                    const parsed = this.extract(response);
                    if (!parsed) {
                        throw new Error('Refresh failed: unsupported response shape (missing access token).');
                    }

                    this.authSession.storeSessionToken(parsed.accessToken, {
                        expiresAt: parsed.expiresAt,
                        passwordChangeRequired: parsed.passwordChangeRequired,
                    });

                    if (parsed.refreshToken) {
                        this.authSession.storeRefreshToken(parsed.refreshToken);
                    }

                    const refreshTokenPresent = Boolean(parsed.refreshToken ?? this.authSession.refreshToken());

                    return {
                        storedInSession: true,
                        token: parsed.accessToken,
                        expiresAt: parsed.expiresAt ?? null,
                        refreshTokenPresent,
                        passwordChangeRequired: parsed.passwordChangeRequired,
                        raw: parsed.raw,
                    };
                }),
            );
    }

    changePassword(params: PasswordChangePasswordParams): Observable<void> {
        const payload: PasswordChangePasswordRequest = {
            currentPassword: params.currentPassword,
            newPassword: params.newPassword,
        };

        return this.apiClient.passwordChangePassword(payload).pipe(
            map(() => {
                // Password changes revoke refresh tokens server-side; force a clean re-login.
                this.authSession.clearAuthState();
            }),
        );
    }

    private extract(response: unknown): PasswordLoginResponse | null {
        const parsed = passwordLoginResponseSchema.safeParse(response);
        return parsed.success ? parsed.data : null;
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

function extractTenantIdFromToken(token: string): string | null {
    const parts = token.split('.');
    if (parts.length !== 3) {
        return null;
    }
    const payloadJson = base64UrlDecodeToString(parts[1]);
    if (!payloadJson) {
        return null;
    }
    try {
        const payload = JSON.parse(payloadJson);
        return (payload.tenantId || payload.tenant || null) as string | null;
    } catch {
        return null;
    }
}
