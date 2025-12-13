import { HttpClient } from '@angular/common/http';
import { EnvironmentProviders, InjectionToken, Provider, inject, makeEnvironmentProviders } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { map } from 'rxjs/operators';

import { ScheduleListResponse, scheduleListResponseSchema } from '@croniq/api-schema';

export interface CroniqApiClient {
    getSchedules(): Promise<ScheduleListResponse>;
}

export const CRONIQ_API_BASE_URL = new InjectionToken<string>('CRONIQ_API_BASE_URL', {
    providedIn: 'root',
    factory: () => 'https://api.croniq.dev',
});

class HttpCroniqApiClient implements CroniqApiClient {
    constructor(private readonly http: HttpClient, private readonly baseUrl: string) { }

    async getSchedules(): Promise<ScheduleListResponse> {
        const response$ = this.http
            .get<unknown>(`${this.baseUrl}/schedules`)
            .pipe(map((payload) => scheduleListResponseSchema.parse(payload)));
        return firstValueFrom(response$);
    }
}

export const CRONIQ_API_CLIENT = new InjectionToken<CroniqApiClient>('CRONIQ_API_CLIENT', {
    providedIn: 'root',
    factory: () => new HttpCroniqApiClient(inject(HttpClient), inject(CRONIQ_API_BASE_URL)),
});

export function provideCroniqApiClient(config: { baseUrl?: string } = {}): EnvironmentProviders {
    const providers: Provider[] = [];
    if (config.baseUrl) {
        providers.push({ provide: CRONIQ_API_BASE_URL, useValue: config.baseUrl });
    }
    return makeEnvironmentProviders(providers);
}
