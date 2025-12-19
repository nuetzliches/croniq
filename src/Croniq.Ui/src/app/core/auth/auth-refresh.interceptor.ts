import { HttpContextToken, HttpErrorResponse, type HttpInterceptorFn } from '@angular/common/http';
import { inject } from '@angular/core';
import { Router } from '@angular/router';
import { CRONIQ_API_BASE_URL } from 'data-access';
import { catchError, from, switchMap, throwError } from 'rxjs';
import { AuthRefreshCoordinator } from './auth-refresh-coordinator.service';

const DID_RETRY_WITH_REFRESH = new HttpContextToken<boolean>(() => false);

function isAuthEndpoint(url: string, baseUrl: string): boolean {
    const normalizedBase = baseUrl.endsWith('/') ? baseUrl.slice(0, -1) : baseUrl;
    const authPrefix = `${normalizedBase}/auth/`;
    return url.startsWith(authPrefix);
}

export const authRefreshInterceptor: HttpInterceptorFn = (req, next) => {
    const baseUrl = inject(CRONIQ_API_BASE_URL);
    const refreshCoordinator = inject(AuthRefreshCoordinator);
    const router = inject(Router);

    const isApiCall = typeof baseUrl === 'string' && baseUrl.length > 0 && req.url.startsWith(baseUrl);
    if (!isApiCall || isAuthEndpoint(req.url, baseUrl)) {
        return next(req);
    }

    return from(refreshCoordinator.ensureFreshAccessToken()).pipe(
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
                        return from(refreshCoordinator.forceRefresh()).pipe(
                            switchMap((refreshed) => {
                                if (!refreshed) {
                                    const returnUrl = router.url;
                                    void router.navigate(['/login'], {
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
