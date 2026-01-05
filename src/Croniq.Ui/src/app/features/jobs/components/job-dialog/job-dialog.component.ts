import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required } from '@angular/forms/signals';
import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { UpsertJobRequest } from '@croniq/api-schema';

interface JobFormModel {
    jobKey: string;
    namespace: string;
    name: string;
    variant: string;
    description: string;
}

function mapToFormModel(data: UpsertJobRequest | null): JobFormModel {
    return {
        jobKey: data?.jobKey ?? '',
        namespace: data?.namespace ?? 'default',
        name: data?.name ?? '',
        variant: data?.variant ?? '',
        description: data?.description ?? '',
    };
}

@Component({
    selector: 'cq-job-dialog',
    imports: [Field],
    templateUrl: './job-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class JobDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<UpsertJobRequest | null>(DIALOG_DATA);

    readonly isEdit = !!this.data;
    readonly submitAttempted = signal(false);

    readonly jobModel = signal(mapToFormModel(this.data));

    readonly jobForm = form(this.jobModel, (f) => {
        required(f.jobKey, { message: 'Job Key is required.' });
        required(f.namespace, { message: 'Namespace is required.' });
        required(f.name, { message: 'Name is required.' });
    });

    readonly jobKeyInvalid = computed(() => !this.jobModel().jobKey);
    readonly namespaceInvalid = computed(() => !this.jobModel().namespace);
    readonly nameInvalid = computed(() => !this.jobModel().name);

    close(): void {
        this.dialogRef.close();
    }

    save(): void {
        this.submitAttempted.set(true);

        if (this.jobForm().invalid()) {
            return;
        }

        const model = this.jobModel();
        const payload: UpsertJobRequest = {
            jobKey: model.jobKey,
            namespace: model.namespace,
            name: model.name,
            variant: model.variant || undefined,
            description: model.description || undefined,
            metadata: this.data?.metadata,
        };

        this.dialogRef.close(payload);
    }
}
