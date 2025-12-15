import { provideHttpClient, withFetch } from '@angular/common/http';
import { ApplicationConfig } from '@angular/core';
import { provideRouter, withEnabledBlockingInitialNavigation } from '@angular/router';

import { CRONIQ_CREDENTIAL_SUPPLIER, provideCroniqApiClient } from 'data-access';
import { appRoutes } from './app.routes';
import { API_CONFIG } from './core/api-config';
import { AuthSessionService } from './core/auth/auth-session.service';
import { FEATURE_COMMAND_PROVIDERS } from './features/feature-command-providers';

export const appConfig: ApplicationConfig = {
    providers: [
        provideRouter(appRoutes, withEnabledBlockingInitialNavigation()),
        provideHttpClient(withFetch()),
        provideCroniqApiClient({ baseUrl: API_CONFIG.baseUrl }),
        { provide: CRONIQ_CREDENTIAL_SUPPLIER, useExisting: AuthSessionService },
        ...FEATURE_COMMAND_PROVIDERS,
    ],
};