import { provideHttpClient, withFetch } from '@angular/common/http';
import { ApplicationConfig } from '@angular/core';
import { provideRouter, withEnabledBlockingInitialNavigation } from '@angular/router';

import { provideCroniqApiClient } from 'data-access';
import { appRoutes } from './app.routes';
import { FEATURE_COMMAND_PROVIDERS } from './features/feature-command-providers';
import { API_CONFIG } from './core/api-config';

export const appConfig: ApplicationConfig = {
    providers: [
        provideRouter(appRoutes, withEnabledBlockingInitialNavigation()),
        provideHttpClient(withFetch()),
        provideCroniqApiClient({ baseUrl: API_CONFIG.baseUrl }),
        ...FEATURE_COMMAND_PROVIDERS,
    ],
};