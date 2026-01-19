import { Injectable, inject } from '@angular/core';
import { Router } from '@angular/router';
import { epochMsFromIso, nowMs } from '@core/time/clock';
import { catchError, defer, finalize, map, of, shareReplay } from 'rxjs';
import type { Observable } from 'rxjs';
import { AuthSessionService } from './auth-session.service';
import { AuthLogoutCleanupService } from './auth-logout-cleanup.service';
import { PasswordAuthService } from './password-auth.service';

const REFRESH_SKEW_MS = 60_000;

@Injectable({ providedIn: 'root' })
export class AuthRefreshCoordinator {
    private readonly authSession = inject(AuthSessionService);
    private readonly passwordAuth = inject(PasswordAuthService);
    private readonly router = inject(Router);
    private readonly authCleanup = inject(AuthLogoutCleanupService);

    private inFlight$: Observable<string | null> | null = null;

    ensureFreshAccessToken(): Observable<string | null> {
        const refreshToken = this.authSession.refreshToken()?.trim() ?? '';
        const currentToken = this.authSession.getSessionToken();

        if (!refreshToken) {
            return of(currentToken);
        }

        const expiresAt = this.authSession.sessionToken()?.expiresAt ?? null;
        const expiryMs = expiresAt ? epochMsFromIso(expiresAt) : null;
        const shouldRefresh =
            !currentToken ||
            (expiryMs != null && expiryMs - nowMs() <= REFRESH_SKEW_MS);

        if (!shouldRefresh) {
            return of(currentToken);
        }

        return this.forceRefresh();
    }

    forceRefresh(): Observable<string | null> {
        if (this.inFlight$) {
            return this.inFlight$;
        }

        this.inFlight$ = defer(() => this.passwordAuth.refresh()).pipe(
            map((result) => {
                if (result.passwordChangeRequired) {
                    void this.router.navigate(['/auth', 'change-password']);
                }
                return result.token;
            }),
            catchError(() => {
                this.authCleanup.clearAll();
                void this.router.navigate(['/auth', 'login']);
                return of(null);
            }),
            shareReplay({ bufferSize: 1, refCount: false }),
            finalize(() => {
                this.inFlight$ = null;
            }),
        );

        return this.inFlight$;
    }
}
