import { Routes } from '@angular/router';
import { requireSessionTokenGuard } from './core/auth/require-session-token.guard';

export const appRoutes: Routes = [
    {
        path: '',
        pathMatch: 'full',
        redirectTo: 'dashboard',
    },
    {
        path: 'login',
        loadComponent: () =>
            import('./features/login/login-page/login-page').then((m) => m.LoginPage),
    },
    {
        path: 'dashboard',
        canActivate: [requireSessionTokenGuard],
        loadComponent: () =>
            import('./features/dashboard/dashboard-page/dashboard-page').then((m) => m.DashboardPage),
    },
    {
        path: 'schedules',
        canActivate: [requireSessionTokenGuard],
        loadComponent: () =>
            import('./features/schedules/schedules-page/schedules-page').then((m) => m.SchedulesPage),
    },
    {
        path: 'jobs',
        canActivate: [requireSessionTokenGuard],
        loadComponent: () =>
            import('./features/jobs/jobs-page/jobs-page').then((m) => m.JobsPage),
    },
    {
        path: 'webhooks',
        canActivate: [requireSessionTokenGuard],
        loadComponent: () =>
            import('./features/webhooks/webhooks-page/webhooks-page').then((m) => m.WebhooksPage),
    },
    {
        path: '**',
        redirectTo: 'dashboard',
    },
];
