import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormField, form, min, submit } from '@angular/forms/signals';
import { RotateWebhookSecretRequest } from '@croniq/api-schema';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective, CqFormFieldComponent, CqInputDirective, CqTextareaDirective } from 'ui-kit';

type WebhookRotateSecretDialogData = {
    hookKey: string;
};

type RotateSecretFormModel = {
    activateInSeconds: number | null | '';
    gracePeriodSeconds: number | null | '';
    notes: string;
};

const DEFAULT_MODEL: RotateSecretFormModel = {
    activateInSeconds: null,
    gracePeriodSeconds: null,
    notes: '',
};

@Component({
    selector: 'cq-webhook-rotate-secret-dialog',
    templateUrl: './webhook-rotate-secret-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [
        FormField,
        CqDialogComponent,
        CqDialogHeaderDirective,
        CqDialogFooterDirective,
        CqFormFieldComponent,
        CqInputDirective,
        CqTextareaDirective,
    ],
})
export class WebhookRotateSecretDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<WebhookRotateSecretDialogData>(DIALOG_DATA);

    readonly submitAttempted = signal(false);
    readonly model = signal<RotateSecretFormModel>({ ...DEFAULT_MODEL });

    readonly form = form(this.model, (f) => {
        min(f.activateInSeconds, 0, { message: 'Must be non-negative.' });
        min(f.gracePeriodSeconds, 0, { message: 'Must be non-negative.' });
    });

    readonly activateError = computed(() => this.validateNonNegative(this.model().activateInSeconds));
    readonly graceError = computed(() => this.validateNonNegative(this.model().gracePeriodSeconds));

    close(): void {
        this.dialogRef.close();
    }

    async onSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        this.submitAttempted.set(true);

        if (this.activateError() || this.graceError()) {
            return;
        }

        await submit(this.form, async () => {
            const payload = this.buildRequest();
            this.dialogRef.close(payload);
        });
    }

    private buildRequest(): RotateWebhookSecretRequest {
        const model = this.model();
        const activateInSeconds = this.coerceNumber(model.activateInSeconds);
        const gracePeriodSeconds = this.coerceNumber(model.gracePeriodSeconds);
        const notes = model.notes?.trim() ? model.notes.trim() : null;

        return {
            activateInSeconds,
            gracePeriodSeconds,
            notes,
        } satisfies RotateWebhookSecretRequest;
    }

    private validateNonNegative(value: number | null | ''): string | null {
        if (value === null || value === undefined || value === '') {
            return null;
        }
        const numeric = Number(value);
        if (!Number.isFinite(numeric) || numeric < 0) {
            return 'Value must be a non-negative number.';
        }
        return null;
    }

    private coerceNumber(value: number | null | ''): number | null {
        if (value === null || value === undefined || value === '') {
            return null;
        }
        const numeric = Number(value);
        return Number.isFinite(numeric) ? numeric : null;
    }
}
