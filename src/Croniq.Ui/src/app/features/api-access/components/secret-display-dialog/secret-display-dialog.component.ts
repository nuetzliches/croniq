import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';

export interface SecretDisplayData {
    secret: string;
}

@Component({
    selector: 'cq-secret-display-dialog',
    templateUrl: './secret-display-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [],
})
export class SecretDisplayDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<SecretDisplayData>(DIALOG_DATA);

    readonly copied = signal(false);

    async copy() {
        try {
            await navigator.clipboard.writeText(this.data.secret);
            this.copied.set(true);
            setTimeout(() => this.copied.set(false), 2000);
        } catch (err) {
            console.error('Failed to copy text: ', err);
        }
    }

    close(): void {
        this.dialogRef.close();
    }
}
