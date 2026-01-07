import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { Field, form, required, submit } from '@angular/forms/signals';
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
    imports: [Field],
    templateUrl: './schedule-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ScheduleDialogComponent {
    readonly schedule = input<UpsertScheduleRequest | null>(null);

    readonly save = output<UpsertScheduleRequest>();
    readonly closeDialog = output<void>();

    readonly isEditMode = computed(() => !!this.schedule());

    readonly model = linkedSignal(() => getScheduleFormModel(this.schedule()));

    readonly scheduleForm = form(this.model, (f) => {
        required(f.jobKey);
        required(f.cronExpression);
    });

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();

        await submit(this.scheduleForm, async () => {
            const request = this.model();
            this.save.emit(request);
        });
    }

    onCancel() {
        this.closeDialog.emit();
    }
}
