import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { disabled, Field, form, required } from '@angular/forms/signals';
import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { RuntimeConfigService } from '@core/runtime-config.service';

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
    imports: [Field],
    templateUrl: './webhook-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhookDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<UpsertWebhookEndpointRequest | null>(DIALOG_DATA);
    private readonly runtimeConfig = inject(RuntimeConfigService);

    readonly isEdit = !!this.data;
    readonly submitAttempted = signal(false);
    readonly signatureToggleDisabled = !this.runtimeConfig.webhooksAllowUnsignedHooks;

    readonly webhookModel = signal(mapToFormModel(this.data, this.signatureToggleDisabled));

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

    save(): void {
        this.submitAttempted.set(true);

        if (this.webhookForm().invalid()) {
            return;
        }

        const model = this.webhookModel();

        // Coerce requestsPerMinute to number or null, as HTML input might return a string
        const rawRpm = model.requestsPerMinute as unknown;
        const requestsPerMinute = rawRpm === null || rawRpm === '' ? null : Number(rawRpm);

        const payload: UpsertWebhookEndpointRequest = {
            hookKey: model.hookKey,
            jobKey: model.jobKey,
            enabled: model.enabled,
            requireSignature: this.signatureToggleDisabled ? true : model.requireSignature,
            requestsPerMinute,
            metadata: model.description ? { description: model.description } : undefined,
            secret: model.secret || undefined,
        };

        this.dialogRef.close(payload);
    }
}
