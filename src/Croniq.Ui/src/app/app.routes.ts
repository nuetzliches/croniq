import { Routes } from '@angular/router';
import { redirectIfSessionTokenGuard } from './core/auth/redirect-if-session-token.guard';
import { requireSessionTokenGuard } from './core/auth/require-session-token.guard';

export const appRoutes: Routes = [
    {
        path: '',
        pathMatch: 'full',
        redirectTo: 'dashboard',
    },
    {
        path: 'login',
        canActivate: [redirectIfSessionTokenGuard],
        loadComponent: () =>
            import('./features/login/login-page/login-page').then((m) => m.LoginPage),
    },
    {
        path: '',
        canActivate: [requireSessionTokenGuard],
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
                path: 'change-password',
                loadComponent: () =>
                    import('./features/account/change-password-page/change-password-page').then(
                        (m) => m.ChangePasswordPage,
                    ),
            },
        ],
    },
    {
        path: '**',
        redirectTo: 'dashboard',
    },
];
