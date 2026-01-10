import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ExecutionsStore } from '@features/executions/executions.store';
import { finalize } from 'rxjs';

export interface LogViewerData {
    executionId: string;
}

@Component({
    selector: 'cq-log-viewer-dialog',
    templateUrl: './log-viewer-dialog.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [],
    providers: [ExecutionsStore] // Note: In this case, we might want to use the parent store or a fresh one. 
    // Since fetchLogs is stateless in the store, a fresh provider is fine, 
    // BUT usually we want to inject the token.
    // Actually, `ExecutionsStore` is just a service wrapper around API. 
    // Let's use `inject(ExecutionsStore)` but provided here or in parent.
    // If we provide it here, we get a fresh instance.
})
export class LogViewerDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    private readonly store = inject(ExecutionsStore);
    private readonly destroyRef = inject(DestroyRef);
    readonly data = inject<LogViewerData>(DIALOG_DATA);

    readonly logs = signal<string>('');
    readonly isLoading = signal(false);
    readonly error = signal(false);
    readonly copied = signal(false);

    // Side effect: trigger the log fetch on dialog open.
    private readonly _loadLogs = effect(() => {
        const executionId = this.data.executionId?.trim();
        if (!executionId) {
            return;
        }
        this.loadLogs(executionId);
    });

    loadLogs(executionId: string) {
        this.isLoading.set(true);
        this.error.set(false);
        this.store.fetchLogs(executionId)
            .pipe(
                finalize(() => this.isLoading.set(false)),
                takeUntilDestroyed(this.destroyRef),
            )
            .subscribe({
                next: (content) => this.logs.set(content),
                error: () => this.error.set(true)
            });
    }

    async copyLogs() {
        const text = this.logs();
        if (!text) return;

        try {
            await navigator.clipboard.writeText(text);
            this.copied.set(true);
            setTimeout(() => this.copied.set(false), 2000);
        } catch (err) {
            console.error('Failed to copy', err);
        }
    }

    close(): void {
        this.dialogRef.close();
    }
}
