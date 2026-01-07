import { Routes } from '@angular/router';

export const METRICS_ROUTES: Routes = [
    {
        path: '',
        loadComponent: () => import('./metrics-page/metrics-page').then(m => m.MetricsPage),
    },
];
