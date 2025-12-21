import { inject } from '@angular/core';
import { type CanActivateFn, Router } from '@angular/router';
import { AuthSessionService } from './auth-session.service';

export const redirectIfSessionTokenGuard: CanActivateFn = (route) => {
    const authSession = inject(AuthSessionService);
    const router = inject(Router);

    if (!authSession.getSessionToken()) {
        return true;
    }

    if (authSession.passwordChangeRequired()) {
        return router.parseUrl('/auth/change-password');
    }

    const returnUrl = (route.queryParamMap.get('returnUrl') ?? '').trim();
    if (
        !returnUrl ||
        returnUrl === '/' ||
        returnUrl.startsWith('/login') ||
        returnUrl.startsWith('/auth')
    ) {
        return router.parseUrl('/');
    }

    return router.parseUrl(returnUrl);
};
