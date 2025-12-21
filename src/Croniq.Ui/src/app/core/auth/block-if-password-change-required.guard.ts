import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { AuthSessionService } from './auth-session.service';

/**
 * Prevents access to non-auth application routes while the backend requires a password change.
 */
export const blockIfPasswordChangeRequiredGuard: CanActivateFn = (_route, _state) => {
    const authSession = inject(AuthSessionService);
    const router = inject(Router);

    if (!authSession.passwordChangeRequired()) {
        return true;
    }

    return router.createUrlTree(['/auth', 'change-password']);
};
