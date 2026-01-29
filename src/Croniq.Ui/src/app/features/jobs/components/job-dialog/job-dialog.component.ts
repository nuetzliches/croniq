import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, effect, inject, signal } from '@angular/core';
import { disabled, form, FormField, required, submit } from '@angular/forms/signals';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { UpsertJobRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { firstValueFrom } from 'rxjs';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective, CqIconComponent } from 'ui-kit';

interface JobFormModel {
    jobKey: string;
    namespace: string;
    name: string;
    variant: string;
    description: string;
    assignedRunnerId: string;
    assignmentNotes: string;
}

function mapToFormModel(data: UpsertJobRequest | null): JobFormModel {
    return {
        jobKey: data?.jobKey ?? '',
        namespace: data?.namespace ?? 'default',
        name: data?.name ?? '',
        variant: data?.variant ?? '',
        description: data?.description ?? '',
        assignedRunnerId: data?.assignedRunnerId ?? '',
        assignmentNotes: data?.assignmentNotes ?? '',
    };
}

@Component({
    selector: 'cq-job-dialog',
    imports: [FormField, CqDialogComponent, CqDialogHeaderDirective, CqDialogFooterDirective, CqIconComponent],
    templateUrl: './job-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class JobDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);
    readonly data = inject<UpsertJobRequest | null>(DIALOG_DATA);

    readonly isEdit = !!this.data;
    readonly submitAttempted = signal(false);
    readonly submitError = signal<string | null>(null);
    readonly submitting = signal(false);
    readonly isActive = signal<boolean | null>(this.data?.isActive ?? null);

    readonly jobModel = signal(mapToFormModel(this.data));

    readonly jobForm = form(this.jobModel, (f) => {
        required(f.jobKey, { message: 'Job Key is required.' });
        required(f.namespace, { message: 'Namespace is required.' });
        required(f.name, { message: 'Name is required.' });
        disabled(f.jobKey);
        if (this.isEdit) {
            disabled(f.namespace);
            disabled(f.name);
            disabled(f.variant);
        }
        if (this.isEdit && this.isActive()) {
            disabled(f.assignedRunnerId);
        }
    });

    readonly jobKeyInvalid = computed(() => !this.jobModel().jobKey);
    readonly namespaceInvalid = computed(() => !this.jobModel().namespace);
    readonly nameInvalid = computed(() => !this.jobModel().name);
    readonly jobKeyError = computed(() => this.getJobKeyError());

    constructor() {
        effect(() => {
            if (this.isEdit) {
                return;
            }

            const model = this.jobModel();
            const expected = this.buildExpectedJobKey(model);
            if (!expected || expected === model.jobKey) {
                return;
            }

            this.jobModel.set({ ...model, jobKey: expected });
        });
    }

    close(): void {
        this.dialogRef.close();
    }

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();
        this.submitAttempted.set(true);
        this.submitError.set(null);

        const keyError = this.getJobKeyError();
        if (keyError) {
            return;
        }

        await submit(this.jobForm, async () => {
            if (this.submitting()) {
                return;
            }

            const model = this.jobModel();
            const { tenantId, environment } = this.tenantContext.snapshot();
            if (!tenantId.trim()) {
                this.submitError.set('Required context is missing — unable to upsert job.');
                return;
            }

            const payload: UpsertJobRequest = {
                jobKey: this.buildExpectedJobKey(model),
                namespace: model.namespace,
                name: model.name,
                variant: model.variant || undefined,
                description: model.description || undefined,
                metadata: this.data?.metadata,
                assignedRunnerId: model.assignedRunnerId.trim() || undefined,
                assignmentNotes: model.assignmentNotes.trim() || undefined,
            };

            this.submitting.set(true);
            try {
                await firstValueFrom(this.api.upsertJob(
                    { tenantId, environment },
                    payload,
                    this.tenantContext.createRequestOptions('jobs.upsert', {
                        tenantId,
                        environment,
                    }),
                ));
                this.dialogRef.close(true);
            } catch (error) {
                console.error('Failed to upsert job', error);
                this.submitError.set('Unable to upsert job via API.');
            } finally {
                this.submitting.set(false);
            }
        });
    }

    private getJobKeyError(): string | null {
        const model = this.jobModel();
        const expected = this.buildExpectedJobKey(model);
        if (!expected) {
            return this.submitAttempted() ? 'Job Key is required.' : null;
        }

        if (!this.isKeySegmentValid(model.namespace) || !this.isKeySegmentValid(model.name)) {
            return 'Namespace and Name must not contain colons.';
        }

        if (model.variant && !this.isKeySegmentValid(model.variant)) {
            return 'Variant must not contain colons.';
        }

        return null;
    }

    private buildExpectedJobKey(model: JobFormModel): string {
        const namespace = model.namespace.trim();
        const name = model.name.trim();
        const variant = model.variant.trim();
        return variant ? `${namespace}:${name}:${variant}` : `${namespace}:${name}`;
    }

    private isKeySegmentValid(value: string): boolean {
        return !value.includes(':');
    }

    private equalsIgnoreCase(left: string, right: string): boolean {
        return left.localeCompare(right, undefined, { sensitivity: 'accent' }) === 0;
    }
}
