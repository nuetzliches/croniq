import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { CqDialogComponent, CqDialogFooterDirective, CqDialogHeaderDirective } from '../dialog/dialog';

type ConfirmVariant = 'default' | 'danger';

export type CqConfirmDialogData = {
    title?: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    variant?: ConfirmVariant;
    ariaLabel?: string;
};

@Component({
    selector: 'cq-confirm-dialog',
    template: `
    <cq-dialog
      size="sm"
      [role]="dialogRole()"
      [ariaLabelledby]="data.title ? titleId : null"
      [ariaDescribedby]="messageId"
      [ariaLabel]="!data.title ? (data.ariaLabel || 'Confirm action') : null"
    >
      @if (data.title) {
        <h2 cqDialogHeader class="text-lg font-semibold text-primary" [id]="titleId">
          {{ data.title }}
        </h2>
      }

      <p class="text-sm text-muted" [id]="messageId">{{ data.message }}</p>

      <div cqDialogFooter class="flex justify-end gap-3">
        <button
          type="button"
          (click)="cancel()"
          class="rounded-lg border border-white/10 bg-white/5 px-4 py-2 text-sm font-semibold text-primary transition hover:border-accent hover:text-accent"
          autofocus
        >
          {{ data.cancelLabel || 'Cancel' }}
        </button>
        <button
          type="button"
          (click)="confirm()"
          [class]="data.variant === 'danger'
            ? 'rounded-lg bg-danger px-4 py-2 text-sm font-semibold text-white transition hover:bg-danger/90'
            : 'rounded-lg bg-accent px-4 py-2 text-sm font-semibold text-white transition hover:bg-accent-hover'"
        >
          {{ data.confirmLabel || 'Confirm' }}
        </button>
      </div>
    </cq-dialog>
  `,
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [CqDialogComponent, CqDialogHeaderDirective, CqDialogFooterDirective],
})
export class CqConfirmDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    readonly data = inject<CqConfirmDialogData>(DIALOG_DATA);
    readonly titleId = `cq-confirm-title-${createDialogId()}`;
    readonly messageId = `cq-confirm-message-${createDialogId()}`;

    dialogRole(): 'dialog' | 'alertdialog' {
        return this.data.variant === 'danger' ? 'alertdialog' : 'dialog';
    }

    confirm(): void {
        this.dialogRef.close(true);
    }

    cancel(): void {
        this.dialogRef.close(false);
    }
}

function createDialogId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.round(Math.random() * 1000)}`;
}
