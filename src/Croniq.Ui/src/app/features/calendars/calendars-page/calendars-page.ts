import { DatePipe } from '@angular/common';
import { CdkMenu } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import type { CalendarMode, CalendarResponse, CroniqCalendarSeedDefinition } from '@croniq/api-schema';
import { CalendarDialogComponent } from '@features/calendars/components/calendar-dialog/calendar-dialog.component';
import { CalendarSummaryView, CalendarsStore } from '@features/calendars/calendars.store';
import { bindQueryParam } from '@shared/routing/selection-sync';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { CqCellDefDirective, CqColumnComponent, CqContextMenuItemDirective, CqDialogService, CqIconComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';
import { filter } from 'rxjs';

type CalendarStatusFilter = 'all' | 'enabled' | 'paused';
type CalendarModeFilter = 'all' | 'include' | 'exclude';

const STATUS_OPTIONS: ReadonlyArray<{ value: CalendarStatusFilter; label: string }> = [
    { value: 'all', label: 'All statuses' },
    { value: 'enabled', label: 'Enabled' },
    { value: 'paused', label: 'Paused' },
];

const MODE_OPTIONS: ReadonlyArray<{ value: CalendarModeFilter; label: string }> = [
    { value: 'all', label: 'All modes' },
    { value: 'include', label: 'Include' },
    { value: 'exclude', label: 'Exclude' },
];

@Directive({
    selector: '[cqCalendarCell]',
    providers: [{ provide: CqCellDefDirective, useExisting: CqCalendarCellDirective }],
})
export class CqCalendarCellDirective extends CqCellDefDirective<CalendarSummaryView> {
    // Inherits ngTemplateContextGuard from base class
}

@Component({
    selector: 'cq-calendars-page',
    imports: [
        CdkMenu,
        DatePipe,
        DataGrid,
        CqColumnComponent,
        CqCalendarCellDirective,
        CqContextMenuItemDirective,
        CqIconComponent,
        CqInputDirective,
        CqSelectDirective,
    ],
    templateUrl: './calendars-page.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    providers: [CalendarsStore],
})
export class CalendarsPage {
    private readonly store = inject(CalendarsStore);
    private readonly dialog = inject(CqDialogService);
    private readonly shellPanel = inject(ShellPanelService);
    private readonly panelTemplate = viewChild<TemplateRef<unknown>>('calendarsFilterPanel');
    private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('calendarsFilterCollapsed');

    readonly calendars = this.store.calendars;
    readonly calendarDefinitions = this.store.calendarDefinitions;
    readonly loading = this.store.loading;
    readonly error = this.store.error;

    readonly calendarSearch = signal('');
    readonly statusFilter = signal<CalendarStatusFilter>('all');
    readonly modeFilter = signal<CalendarModeFilter>('all');
    readonly selectedCalendarId = bindQueryParam({ paramKey: 'calendarId' });
    readonly statusOptions = STATUS_OPTIONS;
    readonly modeOptions = MODE_OPTIONS;

    readonly filteredCalendars = computed(() => {
        const query = this.calendarSearch().trim().toLowerCase();
        const status = this.statusFilter();
        const mode = this.modeFilter();

        return this.calendars().filter((calendar) => {
            if (status !== 'all') {
                if (status === 'enabled' && !calendar.enabled) {
                    return false;
                }
                if (status === 'paused' && calendar.enabled) {
                    return false;
                }
            }

            if (mode !== 'all') {
                const isInclude = calendar.mode === 0;
                if (mode === 'include' && !isInclude) {
                    return false;
                }
                if (mode === 'exclude' && isInclude) {
                    return false;
                }
            }

            if (!query) {
                return true;
            }

            return (
                calendar.calendarId.toLowerCase().includes(query) ||
                calendar.name.toLowerCase().includes(query) ||
                (calendar.description ?? '').toLowerCase().includes(query) ||
                calendar.timeZoneId.toLowerCase().includes(query)
            );
        });
    });

    readonly selectedCalendar = computed(() => {
        const raw = this.selectedCalendarId();
        if (raw === null || raw === undefined) {
            return null;
        }
        const id = typeof raw === 'string' ? raw : String(raw);
        return this.calendars().find((calendar) => calendar.calendarId === id) ?? null;
    });

    calendarRowKey = (row: CalendarSummaryView, index: number) =>
        row.calendarId || `calendar-${index}`;

    calendarRowClasses = (row: CalendarSummaryView) =>
        row.enabled ? undefined : ['opacity-80'];

    constructor() {
        effect((onCleanup) => {
            const template = this.panelTemplate();
            const collapsedTemplate = this.collapsedTemplate();
            if (!template) {
                return;
            }
            this.shellPanel.setPanel(
                template,
                'Filters & settings',
                'Refine the calendars list.',
                collapsedTemplate ?? null,
            );
            onCleanup(() => this.shellPanel.clearPanel(template));
        });
    }

    refresh(): void {
        this.store.refresh();
    }

    setCalendarSearch(query: string): void {
        this.calendarSearch.set(query);
    }

    setStatusFilter(status: CalendarStatusFilter): void {
        this.statusFilter.set(status);
    }

    setModeFilter(mode: CalendarModeFilter): void {
        this.modeFilter.set(mode);
    }

    resetFilters(): void {
        this.calendarSearch.set('');
        this.statusFilter.set('all');
        this.modeFilter.set('all');
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
