import { inject } from '@angular/core';
import { type CanActivateFn, Router } from '@angular/router';
import { catchError, map, of } from 'rxjs';
import { AuthRefreshCoordinator } from './auth-refresh-coordinator.service';
import { AuthSessionService } from './auth-session.service';

export const redirectIfSessionTokenGuard: CanActivateFn = (route) => {
    const authSession = inject(AuthSessionService);
    const refreshCoordinator = inject(AuthRefreshCoordinator);
    const router = inject(Router);

    const returnUrl = (route.queryParamMap.get('returnUrl') ?? '').trim();

    const token = authSession.getSessionToken();
    if (!token) {
        return refreshCoordinator.ensureFreshAccessToken().pipe(
            map((refreshed) => (refreshed ? resolveRedirect(router, authSession, returnUrl) : true)),
            catchError(() => of(true)),
        );
    }

    return resolveRedirect(router, authSession, returnUrl);
};

function resolveRedirect(router: Router, authSession: AuthSessionService, returnUrl: string) {
    if (authSession.passwordChangeRequired()) {
        return router.parseUrl('/auth/change-password');
    }

    if (
        !returnUrl ||
        returnUrl === '/' ||
        returnUrl.startsWith('/login') ||
        returnUrl.startsWith('/auth')
    ) {
        return router.parseUrl('/');
    }

    return router.parseUrl(returnUrl);
}
