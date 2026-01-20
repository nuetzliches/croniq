import { HttpClient, HttpHeaders } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { tryIsoFromUnknown } from '@core/time/clock';
import { catchError, map, of } from 'rxjs';
import type { Observable } from 'rxjs';
import { z } from 'zod';
import { AuthSessionService } from './auth-session.service';
import { AuthLogoutCleanupService } from './auth-logout-cleanup.service';
import type { PasswordRefreshResult } from './password-auth.service';
import { RuntimeConfigService } from '../runtime-config.service';

const CSRF_COOKIE = 'croniq.oidc.csrf';
const CSRF_HEADER = 'X-CSRF';

const oidcRefreshResponseSchema = z
    .object({
        accessToken: z.string().trim().min(1),
        tokenType: z.string().trim().optional(),
        expiresIn: z.number().int().positive().optional(),
        expiresAt: z.unknown().optional(),
        tenantId: z.string().trim().min(1).optional().nullable(),
    })
    .passthrough()
    .transform((data) => {
        const expiryFromField = tryIsoFromUnknown(data.expiresAt);
        const expiryFromSeconds =
            !expiryFromField && typeof data.expiresIn === 'number'
                ? new Date(Date.now() + data.expiresIn * 1000).toISOString()
                : null;

        return {
            accessToken: data.accessToken,
            tokenType: data.tokenType ?? 'Bearer',
            expiresAt: expiryFromField ?? expiryFromSeconds,
            tenantId: (data.tenantId ?? null) as string | null,
            raw: data as unknown,
        };
    });

type OidcRefreshResponse = z.infer<typeof oidcRefreshResponseSchema>;

@Injectable({ providedIn: 'root' })
export class OidcAuthService {
    private readonly http = inject(HttpClient);
    private readonly runtimeConfig = inject(RuntimeConfigService);
    private readonly authSession = inject(AuthSessionService);
    private readonly authCleanup = inject(AuthLogoutCleanupService);

    startLogin(returnUrl: string): void {
        const target = this.resolveApiUrl('/auth/oidc/start');
        const redirect = `${target}?returnUrl=${encodeURIComponent(returnUrl)}`;
        window.location.assign(redirect);
    }

    refresh(): Observable<PasswordRefreshResult> {
        const csrf = readCookie(CSRF_COOKIE);
        const headers = csrf ? new HttpHeaders({ [CSRF_HEADER]: csrf }) : new HttpHeaders();
        const url = this.resolveApiUrl('/auth/refresh');

        return this.http.post<unknown>(url, null, { withCredentials: true, headers }).pipe(
            map((response) => {
                const parsed = this.extract(response);
                if (!parsed) {
                    throw new Error('Refresh failed: unsupported response shape (missing access token).');
                }

                this.authSession.storeSessionToken(parsed.accessToken, { expiresAt: parsed.expiresAt });

                let tenantId = parsed.tenantId;
                if (!tenantId) {
                    tenantId = extractTenantIdFromToken(parsed.accessToken);
                }

                if (tenantId) {
                    this.authSession.storeTenantId(tenantId);
                }

                return {
                    storedInSession: true,
                    token: parsed.accessToken,
                    expiresAt: parsed.expiresAt ?? null,
                    refreshTokenPresent: true,
                    passwordChangeRequired: false,
                    raw: parsed.raw,
                };
            }),
        );
    }

    logout(): Observable<void> {
        const csrf = readCookie(CSRF_COOKIE);
        const headers = csrf ? new HttpHeaders({ [CSRF_HEADER]: csrf }) : new HttpHeaders();
        const url = this.resolveApiUrl('/auth/logout');

        return this.http.post(url, null, { withCredentials: true, headers, responseType: 'text' }).pipe(
            catchError(() => of('')),
            map(() => {
                this.authCleanup.clearAll();
            }),
        );
    }

    hasSession(): boolean {
        return Boolean(readCookie(CSRF_COOKIE));
    }

    private extract(response: unknown): OidcRefreshResponse | null {
        const parsed = oidcRefreshResponseSchema.safeParse(response);
        return parsed.success ? parsed.data : null;
    }

    private resolveApiUrl(path: string): string {
        const base = this.runtimeConfig.apiBaseUrl.trim();
        const normalizedPath = path.startsWith('/') ? path : `/${path}`;
        if (!base) {
            return normalizedPath;
        }
        if (base.startsWith('/')) {
            return `${base.replace(/\/+$/, '')}${normalizedPath}`;
        }
        return new URL(normalizedPath, base).toString();
    }
}

function readCookie(name: string): string | null {
    if (typeof document === 'undefined') {
        return null;
    }

    const cookies = document.cookie.split(';');
    for (const entry of cookies) {
        const [rawName, ...rest] = entry.trim().split('=');
        if (!rawName || rawName !== name) {
            continue;
        }
        const value = rest.join('=').trim();
        return value ? decodeURIComponent(value) : null;
    }

    return null;
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
