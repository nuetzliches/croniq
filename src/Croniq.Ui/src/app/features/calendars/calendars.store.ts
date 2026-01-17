import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { CalendarMode, CalendarResponse, CroniqCalendarSeedDefinition } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { EMPTY, catchError, finalize, map, of, tap } from 'rxjs';

export type CalendarSummaryView = {
    calendarId: string;
    name: string;
    description?: string;
    timeZoneId: string;
    mode: CalendarMode;
    modeLabel: string;
    enabled: boolean;
    ruleCount: number;
    updatedAtUtc?: string;
};

@Injectable()
export class CalendarsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly calendarDefinitionsSignal = signal<ReadonlyArray<CalendarResponse>>([]);
    private readonly listErrorSignal = signal<string | null>(null);

    private readonly calendarsResource = tenantRxResource<CalendarResponse[], { tenantId: string; environment: string }>({
        command: 'calendars.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            this.listErrorSignal.set(null);

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId) {
                this.listErrorSignal.set('Required context is missing - unable to load calendars.');
                this.calendarDefinitionsSignal.set([]);
                return of([]);
            }

            const request$ = this.api.listCalendars({ tenantId, environment }, requestOptions);

            return request$.pipe(
                map((response) => (Array.isArray(response) ? response : [])),
                tap((response) => {
                    this.calendarDefinitionsSignal.set(response);
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load calendars', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing calendars permissions.',
                    });
                    if (authFailure) {
                        this.listErrorSignal.set(authFailure.message);
                    } else {
                        this.listErrorSignal.set('Unable to load calendars from API.');
                    }
                    this.calendarDefinitionsSignal.set([]);
                    return of([]);
                }),
            );
        },
    });

    private readonly upsertLoadingSignal = signal(false);
    private readonly upsertErrorSignal = signal<string | null>(null);
    private readonly deleteLoadingSignal = signal(false);
    private readonly deleteErrorSignal = signal<string | null>(null);

    readonly loading = computed(() => this.calendarsResource.isLoading());
    readonly error = this.listErrorSignal.asReadonly();

    readonly calendarDefinitions = this.calendarDefinitionsSignal.asReadonly();
    readonly calendars = computed<ReadonlyArray<CalendarSummaryView>>(() =>
        this.calendarDefinitionsSignal().map(mapToSummary),
    );

    readonly upsertLoading = this.upsertLoadingSignal.asReadonly();
    readonly upsertError = this.upsertErrorSignal.asReadonly();
    readonly deleteLoading = this.deleteLoadingSignal.asReadonly();
    readonly deleteError = this.deleteErrorSignal.asReadonly();

    refresh(): void {
        this.calendarsResource.reload();
    }

    upsertCalendar(payload: CroniqCalendarSeedDefinition): void {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.upsertErrorSignal.set('Required context is missing - unable to save calendars.');
            return;
        }

        this.upsertLoadingSignal.set(true);
        this.upsertErrorSignal.set(null);

        this.api
            .upsertCalendar(
                { tenantId, environment },
                payload,
                this.tenantContext.createRequestOptions('calendars.upsert', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.refresh();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to upsert calendar', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing calendars permissions.',
                    });
                    if (authFailure) {
                        this.upsertErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    this.upsertErrorSignal.set('Unable to save calendar via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.upsertLoadingSignal.set(false);
                }),
            )
            .subscribe();
    }

    deleteCalendar(calendarId: string): void {
        const trimmedId = calendarId.trim();
        if (!trimmedId) {
            this.deleteErrorSignal.set('Calendar id is required before deleting.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.deleteErrorSignal.set('Required context is missing - unable to delete calendars.');
            return;
        }

        this.deleteLoadingSignal.set(true);
        this.deleteErrorSignal.set(null);

        this.api
            .deleteCalendar(
                { tenantId, environment, calendarId: trimmedId },
                this.tenantContext.createRequestOptions('calendars.delete', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.calendarDefinitionsSignal.set(
                        this.calendarDefinitionsSignal().filter((calendar) => calendar.calendarId !== trimmedId),
                    );
                    this.refresh();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to delete calendar', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing calendars permissions.',
                    });
                    if (authFailure) {
                        this.deleteErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.deleteErrorSignal.set('Calendar not found (404) - it may have already been deleted.');
                        return EMPTY;
                    }
                    this.deleteErrorSignal.set('Unable to delete calendar via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.deleteLoadingSignal.set(false);
                }),
            )
            .subscribe();
    }
}

const DEFAULT_TIME_ZONE = 'UTC';
const CALENDAR_MODE_LABELS: Record<CalendarMode, string> = {
    0: 'Include',
    1: 'Exclude',
};

function mapToSummary(response: CalendarResponse, index: number): CalendarSummaryView {
    const calendarId =
        typeof response.calendarId === 'string' && response.calendarId.trim()
            ? response.calendarId.trim()
            : `calendar-${index}`;
    const name =
        typeof response.name === 'string' && response.name.trim()
            ? response.name.trim()
            : calendarId;
    const description =
        typeof response.description === 'string' && response.description.trim()
            ? response.description.trim()
            : undefined;
    const timeZoneId =
        typeof response.timeZoneId === 'string' && response.timeZoneId.trim()
            ? response.timeZoneId.trim()
            : DEFAULT_TIME_ZONE;
    const mode = normalizeCalendarMode(response.mode);
    const enabled = typeof response.enabled === 'boolean' ? response.enabled : true;
    const rules = Array.isArray(response.rules) ? response.rules : [];
    const updatedAtUtc =
        typeof response.updatedAtUtc === 'string' && response.updatedAtUtc.trim()
            ? response.updatedAtUtc.trim()
            : undefined;

    return {
        calendarId,
        name,
        description,
        timeZoneId,
        mode,
        modeLabel: CALENDAR_MODE_LABELS[mode],
        enabled,
        ruleCount: rules.length,
        updatedAtUtc,
    };
}

function normalizeCalendarMode(value: unknown): CalendarMode {
    if (value === 1 || value === '1') {
        return 1;
    }
    return 0;
}
