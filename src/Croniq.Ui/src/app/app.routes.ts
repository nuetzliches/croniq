import { Routes } from '@angular/router';

export const appRoutes: Routes = [
    {
        path: '',
        pathMatch: 'full',
        redirectTo: 'dashboard',
    },
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
        path: 'tenants',
        loadComponent: () =>
            import('./features/tenants/tenants-page/tenants-page').then((m) => m.TenantsPage),
    },
    {
        path: '**',
        redirectTo: 'dashboard',
    },
];
