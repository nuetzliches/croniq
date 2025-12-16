import { provideHttpClient, withFetch } from '@angular/common/http';
import { ApplicationConfig, inject, provideAppInitializer } from '@angular/core';
import { provideRouter, withEnabledBlockingInitialNavigation } from '@angular/router';

import { CRONIQ_API_BASE_URL, CRONIQ_CREDENTIAL_SUPPLIER } from 'data-access';
import { appRoutes } from './app.routes';
import { AuthSessionService } from './core/auth/auth-session.service';
import { RuntimeConfigService } from './core/runtime-config.service';
import { FEATURE_COMMAND_PROVIDERS } from './features/feature-command-providers';

export const appConfig: ApplicationConfig = {
    providers: [
        provideRouter(appRoutes, withEnabledBlockingInitialNavigation()),
        provideHttpClient(withFetch()),
        provideAppInitializer(() => {
            const config = inject(RuntimeConfigService);
            return config.load();
        }),
        {
            provide: CRONIQ_API_BASE_URL,
            useFactory: (config: RuntimeConfigService) => config.apiBaseUrl,
            deps: [RuntimeConfigService],
        },
        { provide: CRONIQ_CREDENTIAL_SUPPLIER, useExisting: AuthSessionService },
        ...FEATURE_COMMAND_PROVIDERS,
    ],
};