import { inject } from '@angular/core';
import { CanDeactivateFn, Router } from '@angular/router';

/**
 * Blocks browser back/forward navigation (popstate) while the user is inside /auth/*.
 * This is configured at the route level via canDeactivate.
 */
export const blockAuthPopstateGuard: CanDeactivateFn<unknown> = (
    _component,
    _currentRoute,
    currentState,
    _nextState,
) => {
    const router = inject(Router);

    if (!currentState.url.startsWith('/auth')) {
        return true;
    }

    const nav = router.currentNavigation();
    if (nav?.trigger !== 'popstate') {
        return true;
    }

    // Cancel the navigation. For popstate navigations, this keeps the user on the current /auth/* URL.
    return false;
};
