import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { AuthSessionService } from './auth-session.service';

/**
 * Allows /auth/change-password only when a session token exists AND passwordChangeRequired is true.
 * Fully authenticated users should not access /auth/* pages.
 */
export const requirePasswordChangeGuard: CanActivateFn = (_route, _state) => {
    const authSession = inject(AuthSessionService);
    const router = inject(Router);

    if (!authSession.getSessionToken()) {
        return router.createUrlTree(['/auth', 'login']);
    }

    if (!authSession.passwordChangeRequired()) {
        return router.parseUrl('/');
    }

    return true;
};
