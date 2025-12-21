import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { AuthSessionService } from './auth-session.service';

export const requireSessionTokenGuard: CanActivateFn = (_route, state) => {
  const authSession = inject(AuthSessionService);
  const router = inject(Router);

  const token = authSession.getSessionToken();
  if (!token) {
    return router.createUrlTree(['/auth', 'login'], {
      queryParams: {
        returnUrl: state.url,
      },
    });
  }

  return true;
};
