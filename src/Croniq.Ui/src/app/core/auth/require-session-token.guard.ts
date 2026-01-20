import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { catchError, map, of } from 'rxjs';
import { AuthRefreshCoordinator } from './auth-refresh-coordinator.service';
import { AuthSessionService } from './auth-session.service';

export const requireSessionTokenGuard: CanActivateFn = (_route, state) => {
  const authSession = inject(AuthSessionService);
  const refreshCoordinator = inject(AuthRefreshCoordinator);
  const router = inject(Router);

  const token = authSession.getSessionToken();
  if (!token) {
    return refreshCoordinator.ensureFreshAccessToken().pipe(
      map((refreshed) => {
        if (refreshed) {
          return true;
        }

        return router.createUrlTree(['/auth', 'login'], {
          queryParams: {
            returnUrl: state.url,
          },
        });
      }),
      catchError(() =>
        of(router.createUrlTree(['/auth', 'login'], {
          queryParams: {
            returnUrl: state.url,
          },
        })),
      ),
    );
  }

  return true;
};
