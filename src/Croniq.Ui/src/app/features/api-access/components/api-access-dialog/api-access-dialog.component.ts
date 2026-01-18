import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Field, form, required, submit } from '@angular/forms/signals';
import { UpsertApiClientRequest } from '@croniq/api-schema';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective } from 'ui-kit';

interface ApiClientFormModel {
    clientId: string;
    name: string;
    environmentTag: string;
    // scopes handled separately or via internal helper to split string
}

function mapToFormModel(data: UpsertApiClientRequest | null): ApiClientFormModel {
    return {
        clientId: data?.clientId ?? '',
        name: data?.name ?? '',
        environmentTag: data?.environmentTag ?? 'Live',
    };
}

@Component({
    selector: 'cq-api-access-dialog',
    imports: [Field, CqDialogComponent, CqDialogHeaderDirective, CqDialogFooterDirective],
    templateUrl: './api-access-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ApiAccessDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<UpsertApiClientRequest | null>(DIALOG_DATA);

    readonly isEdit = !!this.data;
    readonly submitAttempted = signal(false);

    readonly model = signal(mapToFormModel(this.data));
    readonly scopesInput = signal((this.data?.scopes || []).join(', '));

    readonly form = form(this.model, (f) => {
        required(f.clientId, { message: 'Client ID is required.' });
    });

    readonly clientIdInvalid = computed(() => !this.model().clientId);

    setScopes(value: string) {
        this.scopesInput.set(value);
    }

    close(): void {
        this.dialogRef.close();
    }

    async onSubmit(event: SubmitEvent) {
        event.preventDefault();
        this.submitAttempted.set(true);

        await submit(this.form, async () => {
            const model = this.model();
            const scopes = this.scopesInput()
                .split(',')
                .map(s => s.trim())
                .filter(s => !!s);

            const payload: UpsertApiClientRequest = {
                clientId: model.clientId,
                name: model.name || null,
                environmentTag: model.environmentTag || null,
                scopes: scopes.length ? scopes : null,
                isActive: true
            };

            this.dialogRef.close(payload);
        });
    }
}
