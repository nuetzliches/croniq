import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

export interface ConfirmDialogData {
    title?: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    variant?: 'default' | 'danger';
}

@Component({
    selector: 'cq-confirm-dialog',
    templateUrl: './confirm-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [],
})
export class ConfirmDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<ConfirmDialogData>(DIALOG_DATA);

    confirm(): void {
        this.dialogRef.close(true);
    }

    cancel(): void {
        this.dialogRef.close(false);
    }
}
