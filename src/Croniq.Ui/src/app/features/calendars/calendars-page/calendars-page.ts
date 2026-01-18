import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, inject } from '@angular/core';
import type { CalendarMode, CalendarResponse, CroniqCalendarSeedDefinition } from '@croniq/api-schema';
import { CalendarDialogComponent } from '@features/calendars/components/calendar-dialog/calendar-dialog.component';
import { CalendarSummaryView, CalendarsStore } from '@features/calendars/calendars.store';
import { CqCellDefDirective, CqColumnComponent, CqDialogService, DataGrid } from 'ui-kit';
import { filter } from 'rxjs';

@Directive({
    selector: '[cqCalendarCell]',
    providers: [{ provide: CqCellDefDirective, useExisting: CqCalendarCellDirective }],
})
export class CqCalendarCellDirective extends CqCellDefDirective<CalendarSummaryView> {
    // Inherits ngTemplateContextGuard from base class
}

@Component({
    selector: 'cq-calendars-page',
    imports: [DatePipe, DataGrid, CqColumnComponent, CqCalendarCellDirective],
    templateUrl: './calendars-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    providers: [CalendarsStore],
})
export class CalendarsPage {
    private readonly store = inject(CalendarsStore);
    private readonly dialog = inject(CqDialogService);

    readonly calendars = this.store.calendars;
    readonly calendarDefinitions = this.store.calendarDefinitions;
    readonly loading = this.store.loading;
    readonly error = this.store.error;

    calendarRowKey = (row: CalendarSummaryView, index: number) =>
        row.calendarId || `calendar-${index}`;

    calendarRowClasses = (row: CalendarSummaryView) =>
        row.enabled ? undefined : ['opacity-80'];

    refresh(): void {
        this.store.refresh();
    }

    createCalendar(): void {
        this.openCalendarDialog(null);
    }

    editCalendar(calendar: CalendarSummaryView): void {
        const definition = this.findCalendarDefinition(calendar.calendarId);
        const payload = definition ? mapToSeedDefinition(definition) : mapSummaryToSeedDefinition(calendar);
        this.openCalendarDialog(payload);
    }

    deleteCalendar(calendarId: string): void {
        if (confirm('Are you sure you want to delete this calendar?')) {
            this.store.deleteCalendar(calendarId);
        }
    }

    private openCalendarDialog(payload: CroniqCalendarSeedDefinition | null): void {
        this.dialog
            .open<CroniqCalendarSeedDefinition>(CalendarDialogComponent, {
                data: payload,
                width: '720px',
                panelClass: 'bg-transparent',
            })
            .closed.pipe(filter((result): result is CroniqCalendarSeedDefinition => !!result))
            .subscribe((result) => {
                this.store.upsertCalendar(result);
            });
    }

    private findCalendarDefinition(calendarId: string): CalendarResponse | null {
        const trimmedId = calendarId.trim();
        if (!trimmedId) {
            return null;
        }
        return (
            this.calendarDefinitions().find((calendar) => calendar.calendarId?.trim() === trimmedId) ?? null
        );
    }
}

const DEFAULT_TIME_ZONE = 'UTC';
const DEFAULT_MODE: CalendarMode = 0;

function mapToSeedDefinition(definition: CalendarResponse): CroniqCalendarSeedDefinition {
    const calendarId =
        typeof definition.calendarId === 'string' ? definition.calendarId.trim() : '';
    const name = typeof definition.name === 'string' ? definition.name.trim() : '';
    const description =
        typeof definition.description === 'string' && definition.description.trim()
            ? definition.description.trim()
            : null;
    const timeZoneId =
        typeof definition.timeZoneId === 'string' && definition.timeZoneId.trim()
            ? definition.timeZoneId.trim()
            : DEFAULT_TIME_ZONE;

    return {
        calendarId,
        name,
        description,
        timeZoneId,
        mode: normalizeCalendarMode(definition.mode),
        enabled: typeof definition.enabled === 'boolean' ? definition.enabled : true,
        rules: Array.isArray(definition.rules) ? definition.rules : [],
    };
}

function mapSummaryToSeedDefinition(summary: CalendarSummaryView): CroniqCalendarSeedDefinition {
    const calendarId = summary.calendarId.trim();
    const name = summary.name.trim();
    const description = summary.description?.trim() ? summary.description.trim() : null;
    const timeZoneId = summary.timeZoneId?.trim() || DEFAULT_TIME_ZONE;

    return {
        calendarId,
        name,
        description,
        timeZoneId,
        mode: summary.mode,
        enabled: summary.enabled,
        rules: [],
    };
}

function normalizeCalendarMode(value: unknown): CalendarMode {
    if (value === 1 || value === '1') {
        return 1;
    }
    return DEFAULT_MODE;
}
