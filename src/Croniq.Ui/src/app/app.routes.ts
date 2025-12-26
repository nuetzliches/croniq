import { Routes } from '@angular/router';
import { blockAuthPopstateGuard } from './core/auth/block-auth-popstate.guard';
import { blockIfPasswordChangeRequiredGuard } from './core/auth/block-if-password-change-required.guard';
import { redirectIfSessionTokenGuard } from './core/auth/redirect-if-session-token.guard';
import { requirePasswordChangeGuard } from './core/auth/require-password-change.guard';
import { requireSessionTokenGuard } from './core/auth/require-session-token.guard';

export const appRoutes: Routes = [
    {
        path: '',
        pathMatch: 'full',
        redirectTo: 'dashboard',
    },
    {
        path: 'login',
        pathMatch: 'full',
        redirectTo: 'auth/login',
    },
    {
        path: 'change-password',
        pathMatch: 'full',
        redirectTo: 'account/change-password',
    },
    {
        path: 'auth',
        children: [
            {
                path: '',
                pathMatch: 'full',
                redirectTo: 'login',
            },
            {
                path: 'login',
                canActivate: [redirectIfSessionTokenGuard],
                canDeactivate: [blockAuthPopstateGuard],
                loadComponent: () =>
                    import('./features/login/login-page/login-page').then((m) => m.LoginPage),
            },
            {
                path: 'change-password',
                canActivate: [requireSessionTokenGuard, requirePasswordChangeGuard],
                canDeactivate: [blockAuthPopstateGuard],
                loadComponent: () =>
                    import('./features/account/change-password-page/change-password-page').then(
                        (m) => m.ChangePasswordPage,
                    ),
            },
        ],
    },
    {
        path: '',
        canActivate: [requireSessionTokenGuard, blockIfPasswordChangeRequiredGuard],
        loadComponent: () => import('./shell/shell/shell').then((m) => m.Shell),
        children: [
            {
                path: 'dashboard',
                loadComponent: () =>
                    import('./features/dashboard/dashboard-page/dashboard-page').then((m) => m.DashboardPage),
            },
            {
                path: 'schedules',
                loadComponent: () =>
                    import('./features/schedules/schedules-page/schedules-page').then((m) => m.SchedulesPage),
            },
            {
                path: 'jobs',
                loadComponent: () =>
                    import('./features/jobs/jobs-page/jobs-page').then((m) => m.JobsPage),
            },
            {
                path: 'webhooks',
                loadComponent: () =>
                    import('./features/webhooks/webhooks-page/webhooks-page').then((m) => m.WebhooksPage),
            },
            {
                path: 'executions',
                loadComponent: () =>
                    import('./features/executions/executions-page/executions-page').then(
                        (m) => m.ExecutionsPage,
                    ),
            },
            {
                path: 'runners',
                loadComponent: () =>
                    import('./features/runners/runners-page/runners-page').then((m) => m.RunnersPage),
            },
            {
                path: 'api-access',
                loadComponent: () =>
                    import('./features/api-access/api-access-page/api-access-page').then(
                        (m) => m.ApiAccessPage,
                    ),
            },
            {
                path: 'settings',
                loadComponent: () =>
                    import('./features/settings/settings-page/settings-page').then((m) => m.SettingsPage),
            },
            {
                path: 'account/change-password',
                loadComponent: () =>
                    import('./features/account/change-password-page/change-password-page').then(
                        (m) => m.ChangePasswordPage,
                    ),
            },
            {
                path: 'change-password',
                pathMatch: 'full',
                redirectTo: 'account/change-password',
            },
        ],
    },
    {
        path: '**',
        redirectTo: 'dashboard',
    },
];
