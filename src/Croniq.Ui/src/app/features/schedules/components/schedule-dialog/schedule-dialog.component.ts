import { JsonPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output, signal } from '@angular/core';
import { Field, form, required } from '@angular/forms/signals';
import { UpsertScheduleRequest } from '@croniq/api-schema';

interface ScheduleFormModel {
    triggerId: string;
    jobKey: string;
    cronExpression: string;
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
        description: '',
        enabled: true,
        startAtUtc: null,
        endAtUtc: null,
    };
}

@Component({
    selector: 'app-schedule-dialog',
    imports: [Field, JsonPipe],
    templateUrl: './schedule-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ScheduleDialogComponent {
    readonly schedule = input<UpsertScheduleRequest | null>(null);

    readonly save = output<UpsertScheduleRequest>();
    readonly closeDialog = output<void>();

    readonly activeTab = signal<'form' | 'json'>('form');

    readonly isEditMode = computed(() => !!this.schedule());

    readonly model = linkedSignal(() => getScheduleFormModel(this.schedule()));

    readonly myForm = form(this.model, (f) => {
        required(f.jobKey);
        required(f.cronExpression);
    });

    onSubmit() {
        if (this.myForm().valid()) {
            const formValue = this.model();
            const request: UpsertScheduleRequest = {
                ...formValue,
                triggerId: formValue.triggerId || null,
            };
            this.save.emit(request);
        }
    }

    onCancel() {
        this.closeDialog.emit();
    }
}
