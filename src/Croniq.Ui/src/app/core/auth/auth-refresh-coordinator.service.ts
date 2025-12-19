import { Injectable, inject } from '@angular/core';
import { Router } from '@angular/router';
import { epochMsFromIso, nowMs } from '@core/time/clock';
import { AuthSessionService } from './auth-session.service';
import { PasswordAuthService } from './password-auth.service';

const REFRESH_SKEW_MS = 60_000;

@Injectable({ providedIn: 'root' })
export class AuthRefreshCoordinator {
    private readonly authSession = inject(AuthSessionService);
    private readonly passwordAuth = inject(PasswordAuthService);
    private readonly router = inject(Router);

    private inFlight: Promise<string | null> | null = null;

    async ensureFreshAccessToken(): Promise<string | null> {
        const refreshToken = this.authSession.refreshToken()?.trim() ?? '';
        const currentToken = this.authSession.getSessionToken();

        if (!refreshToken) {
            return currentToken;
        }

        const expiresAt = this.authSession.sessionToken()?.expiresAt ?? null;
        const expiryMs = expiresAt ? epochMsFromIso(expiresAt) : null;
        const shouldRefresh =
            !currentToken ||
            (expiryMs != null && expiryMs - nowMs() <= REFRESH_SKEW_MS);

        if (!shouldRefresh) {
            return currentToken;
        }

        return this.forceRefresh();
    }

    async forceRefresh(): Promise<string | null> {
        if (this.inFlight) {
            return this.inFlight;
        }

        this.inFlight = (async () => {
            try {
                const result = await this.passwordAuth.refresh();
                if (result.passwordChangeRequired) {
                    await this.router.navigate(['/change-password']);
                }
                return result.token;
            } catch {
                this.authSession.clearAuthState();
                await this.router.navigate(['/login']);
                return null;
            } finally {
                this.inFlight = null;
            }
        })();

        return this.inFlight;
    }
}
