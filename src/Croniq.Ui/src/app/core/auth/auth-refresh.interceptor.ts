import { HttpContextToken, HttpErrorResponse, type HttpInterceptorFn } from '@angular/common/http';
import { inject } from '@angular/core';
import { Router } from '@angular/router';
import { CRONIQ_API_BASE_URL } from 'data-access';
import { catchError, switchMap, throwError } from 'rxjs';
import { AuthRefreshCoordinator } from './auth-refresh-coordinator.service';

const DID_RETRY_WITH_REFRESH = new HttpContextToken<boolean>(() => false);
const AUTH_BYPASS_PATHS = new Set(['/auth/login', '/auth/refresh', '/auth/logout']);

function normalizeBaseUrl(baseUrl: string): string {
    return baseUrl.endsWith('/') ? baseUrl.slice(0, -1) : baseUrl;
}

function resolveBaseUrl(value: string | (() => string)): string {
    const resolved = typeof value === 'function' ? value() : value;
    return resolved?.trim() ?? '';
}

function stripQuery(value: string): string {
    const queryIndex = value.indexOf('?');
    return queryIndex >= 0 ? value.slice(0, queryIndex) : value;
}

function extractApiPath(url: string, baseUrl: string): string | null {
    const normalizedBase = normalizeBaseUrl(baseUrl);
    if (!url.startsWith(normalizedBase)) {
        return null;
    }
    const pathWithQuery = url.slice(normalizedBase.length);
    if (!pathWithQuery) {
        return '/';
    }
    return stripQuery(pathWithQuery);
}

function extractSameOriginPath(url: string): string | null {
    if (url.startsWith('/')) {
        return stripQuery(url);
    }

    const origin = globalThis.location?.origin;
    if (!origin) {
        return null;
    }

    try {
        const parsed = new URL(url, origin);
        if (parsed.origin !== origin) {
            return null;
        }
        return parsed.pathname;
    } catch {
        return null;
    }
}

function resolveApiPath(url: string, baseUrl: string): string | null {
    const normalizedBase = baseUrl.trim();
    if (normalizedBase) {
        return extractApiPath(url, normalizedBase);
    }
    return extractSameOriginPath(url);
}

function shouldBypassAuthRefresh(path: string): boolean {
    if (path.startsWith('/auth/oidc')) {
        return true;
    }
    return AUTH_BYPASS_PATHS.has(path);
}

export const authRefreshInterceptor: HttpInterceptorFn = (req, next) => {
    const baseUrl = resolveBaseUrl(inject(CRONIQ_API_BASE_URL));
    const refreshCoordinator = inject(AuthRefreshCoordinator);
    const router = inject(Router);

    const apiPath = resolveApiPath(req.url, baseUrl);
    if (!apiPath || shouldBypassAuthRefresh(apiPath)) {
        return next(req);
    }

    return refreshCoordinator.ensureFreshAccessToken().pipe(
        switchMap((token) => {
            const withAuth = token
                ? req.clone({ setHeaders: { Authorization: `Bearer ${token}` } })
                : req;

            return next(withAuth).pipe(
                catchError((error: unknown) => {
                    if (
                        error instanceof HttpErrorResponse &&
                        error.status === 401 &&
                        !withAuth.context.get(DID_RETRY_WITH_REFRESH)
                    ) {
                        return refreshCoordinator.forceRefresh().pipe(
                            switchMap((refreshed) => {
                                if (!refreshed) {
                                    const returnUrl = router.url;
                                    void router.navigate(['/auth', 'login'], {
                                        queryParams: returnUrl ? { returnUrl } : undefined,
                                    });
                                    return throwError(() => error);
                                }

                                const retryReq = withAuth.clone({
                                    setHeaders: { Authorization: `Bearer ${refreshed}` },
                                    context: withAuth.context.set(DID_RETRY_WITH_REFRESH, true),
                                });
                                return next(retryReq);
                            }),
                        );
                    }

                    return throwError(() => error);
                }),
            );
        }),
    );
};
