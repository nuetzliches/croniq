import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { disabled, Field, form, required, submit } from '@angular/forms/signals';
import { UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective } from 'ui-kit';
type WebhookDialogData = {
    endpoint: UpsertWebhookEndpointRequest | null;
    capabilities: {
        allowUnsignedHooks: boolean;
        defaultRequestsPerMinute: number;
    } | null;
};

interface WebhookFormModel {
    hookKey: string;
    jobKey: string;
    enabled: boolean;
    requireSignature: boolean;
    requestsPerMinute: number | null;
    description: string;
    secret: string;
}

function mapToFormModel(data: UpsertWebhookEndpointRequest | null, forceSignature: boolean): WebhookFormModel {
    const requireSignature = data?.requireSignature ?? true;
    return {
        hookKey: data?.hookKey ?? '',
        jobKey: data?.jobKey ?? '',
        enabled: data?.enabled ?? true,
        requireSignature: forceSignature ? true : requireSignature,
        requestsPerMinute: data?.requestsPerMinute ?? null,
        description: data?.metadata?.['description'] ?? '',
        secret: '', // Secret is never returned in the view model, so we start empty
    };
}

@Component({
    selector: 'cq-webhook-dialog',
    imports: [Field, CqDialogComponent, CqDialogHeaderDirective, CqDialogFooterDirective],
    templateUrl: './webhook-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhookDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<WebhookDialogData>(DIALOG_DATA);

    readonly isEdit = !!this.data.endpoint;
    readonly submitAttempted = signal(false);
    readonly signatureToggleDisabled = !(this.data.capabilities?.allowUnsignedHooks ?? false);

    readonly webhookModel = signal(mapToFormModel(this.data.endpoint, this.signatureToggleDisabled));

    readonly webhookForm = form(this.webhookModel, (f) => {
        required(f.hookKey, { message: 'Hook Key is required.' });
        required(f.jobKey, { message: 'Target Job is required.' });
        if (!this.isEdit) {
            required(f.secret, { message: 'Secret is required for new endpoints.' });
        }
        if (this.signatureToggleDisabled) {
            disabled(f.requireSignature);
        }
    });

    readonly hookKeyInvalid = computed(() => !this.webhookModel().hookKey);
    readonly jobKeyInvalid = computed(() => !this.webhookModel().jobKey);
    readonly secretInvalid = computed(() => !this.isEdit && !this.webhookModel().secret);

    close(): void {
        this.dialogRef.close();
    }

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();
        this.submitAttempted.set(true);

        await submit(this.webhookForm, async () => {
            const model = this.webhookModel();

            // Coerce requestsPerMinute to number or null, as HTML input might return a string
            const rawRpm = model.requestsPerMinute as unknown;
            const requestsPerMinute = rawRpm === null || rawRpm === '' ? null : Number(rawRpm);
            const requireSignature = this.signatureToggleDisabled ? true : model.requireSignature;

            const payload: UpsertWebhookEndpointRequest = {
                hookKey: model.hookKey,
                jobKey: model.jobKey,
                enabled: model.enabled,
                requireSignature,
                allowUnsigned: !requireSignature,
                requestsPerMinute,
                metadata: model.description ? { description: model.description } : undefined,
                secret: model.secret || undefined,
            };

            this.dialogRef.close(payload);
        });
    }
}
