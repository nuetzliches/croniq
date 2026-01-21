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

function extractApiPath(url: string, baseUrl: string): string | null {
    const normalizedBase = normalizeBaseUrl(baseUrl);
    if (!url.startsWith(normalizedBase)) {
        return null;
    }
    const pathWithQuery = url.slice(normalizedBase.length);
    if (!pathWithQuery) {
        return '/';
    }
    const queryIndex = pathWithQuery.indexOf('?');
    return queryIndex >= 0 ? pathWithQuery.slice(0, queryIndex) : pathWithQuery;
}

function shouldBypassAuthRefresh(url: string, baseUrl: string): boolean {
    const path = extractApiPath(url, baseUrl);
    if (!path) {
        return false;
    }
    if (path.startsWith('/auth/oidc')) {
        return true;
    }
    return AUTH_BYPASS_PATHS.has(path);
}

export const authRefreshInterceptor: HttpInterceptorFn = (req, next) => {
    const baseUrl = resolveBaseUrl(inject(CRONIQ_API_BASE_URL));
    const refreshCoordinator = inject(AuthRefreshCoordinator);
    const router = inject(Router);

    const isApiCall = typeof baseUrl === 'string' && baseUrl.length > 0 && req.url.startsWith(baseUrl);
    if (!isApiCall || shouldBypassAuthRefresh(req.url, baseUrl)) {
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
