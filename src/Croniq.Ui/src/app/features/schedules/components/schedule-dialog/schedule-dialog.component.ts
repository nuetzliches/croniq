import { CdkTrapFocus } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { disabled, FormField, form, required, submit } from '@angular/forms/signals';
import { UpsertScheduleRequest } from '@croniq/api-schema';
import type { CalendarOption } from '@features/schedules/schedules-page/schedules.store';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective } from 'ui-kit';

interface ScheduleFormModel {
    triggerId: string;
    jobKey: string;
    cronExpression: string;
    calendarId: string;
    description: string;
    enabled: boolean;
    startAtUtc: string | null;
    endAtUtc: string | null;
}

const getScheduleFormModel = (schedule?: UpsertScheduleRequest | null): ScheduleFormModel => {
    if (schedule) {
        return {
            triggerId: schedule.triggerId ?? '',
            jobKey: schedule.jobKey ?? '',
            cronExpression: schedule.cronExpression ?? '',
            calendarId: schedule.calendarId ?? '',
            description: schedule.description ?? '',
            enabled: schedule.enabled ?? true,
            startAtUtc: schedule.startAtUtc ?? null,
            endAtUtc: schedule.endAtUtc ?? null,
        };
    }
    return {
        triggerId: '',
        jobKey: '',
        cronExpression: '',
        calendarId: '',
        description: '',
        enabled: true,
        startAtUtc: null,
        endAtUtc: null,
    };
}

@Component({
    selector: 'app-schedule-dialog',
    imports: [FormField, CdkTrapFocus, CqDialogComponent, CqDialogHeaderDirective, CqDialogFooterDirective],
    templateUrl: './schedule-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ScheduleDialogComponent {
    readonly schedule = input<UpsertScheduleRequest | null>(null);
    readonly calendarOptions = input<ReadonlyArray<CalendarOption>>([]);
    readonly calendarOptionsLoading = input(false);
    readonly calendarOptionsError = input<string | null>(null);
    readonly calendarOptionsPermissionDenied = input(false);

    readonly save = output<UpsertScheduleRequest>();
    readonly closeDialog = output<void>();

    readonly isEditMode = computed(() => !!this.schedule());

    readonly model = linkedSignal(() => getScheduleFormModel(this.schedule()));

    readonly scheduleForm = form(this.model, (f) => {
        required(f.jobKey);
        required(f.cronExpression);
        disabled(f.calendarId, () => this.calendarOptionsLoading() || this.calendarOptionsPermissionDenied());
    });

    readonly calendarOptionsEmpty = computed(() => this.calendarOptions().length === 0);

    readonly calendarSelectionMissing = computed(() => {
        if (this.calendarOptionsLoading()) {
            return false;
        }
        if (this.calendarOptionsError()) {
            return false;
        }
        if (this.calendarOptionsPermissionDenied()) {
            return false;
        }
        const selected = this.model().calendarId.trim();
        if (!selected) {
            return false;
        }
        return !this.calendarOptions().some((option) => option.calendarId === selected);
    });

    readonly calendarSelectOptions = computed<ReadonlyArray<CalendarOption>>(() => {
        const options = this.calendarOptions();
        const selected = this.model().calendarId.trim();
        const entries: CalendarOption[] = [{ calendarId: '', label: 'No calendar' }];

        if (selected && !options.some((option) => option.calendarId === selected)) {
            entries.push({
                calendarId: selected,
                label: `Missing calendar (${selected})`,
            });
        }

        return entries.concat(options);
    });

    readonly showCalendarEmptyState = computed(() => {
        if (this.calendarOptionsLoading()) {
            return false;
        }
        if (this.calendarOptionsError()) {
            return false;
        }
        if (this.calendarOptionsPermissionDenied()) {
            return false;
        }
        return this.calendarOptionsEmpty();
    });

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();

        await submit(this.scheduleForm, async () => {
            const request = this.model();
            const calendarId = request.calendarId.trim();
            const payload: UpsertScheduleRequest = {
                ...request,
                calendarId: calendarId ? calendarId : null,
            };
            this.save.emit(payload);
        });
    }

    onCancel() {
        this.closeDialog.emit();
    }

    onBackdropClick(event: MouseEvent) {
        if (event.target === event.currentTarget) {
            this.onCancel();
        }
    }
}
