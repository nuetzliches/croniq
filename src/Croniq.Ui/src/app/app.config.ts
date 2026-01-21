import { provideHttpClient, withFetch, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, inject, provideAppInitializer, provideZonelessChangeDetection } from '@angular/core';
import { provideRouter, withEnabledBlockingInitialNavigation } from '@angular/router';
import { CRONIQ_API_BASE_URL } from 'data-access';
import { firstValueFrom } from 'rxjs';
import { appRoutes } from './app.routes';
import { authRefreshInterceptor } from './core/auth/auth-refresh.interceptor';
import { RuntimeConfigService } from './core/runtime-config.service';
import { FEATURE_COMMAND_PROVIDERS } from './features/feature-command-providers';

export const appConfig: ApplicationConfig = {
    providers: [
        provideZonelessChangeDetection(),
        provideRouter(appRoutes, withEnabledBlockingInitialNavigation()),
        provideHttpClient(withFetch(), withInterceptors([authRefreshInterceptor])),
        provideAppInitializer(() => {
            const config = inject(RuntimeConfigService);
            return firstValueFrom(config.load());
        }),
        {
            provide: CRONIQ_API_BASE_URL,
            useFactory: (config: RuntimeConfigService) => config.apiBaseUrl,
            deps: [RuntimeConfigService],
        },
        ...FEATURE_COMMAND_PROVIDERS,
    ],
};
