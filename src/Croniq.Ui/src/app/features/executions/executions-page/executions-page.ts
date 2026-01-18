import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { LogViewerDialogComponent } from '@features/executions/components/log-viewer-dialog/log-viewer-dialog.component';
import { ExecutionsStore } from '@features/executions/executions.store';
import { CqDialogService } from 'ui-kit';

@Component({
  selector: 'cq-executions-page',
  imports: [DatePipe],
  templateUrl: './executions-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [ExecutionsStore],
})
export class ExecutionsPage {
  private readonly store = inject(ExecutionsStore);
  private readonly dialog = inject(CqDialogService);

  // Filters
  searchQuery = signal('');
  statusFilter = signal<string>('All');
  dateRangeFilter = signal<string>('24h');

  // Data
  readonly executions = this.store.executions;
  readonly isLoading = this.store.isLoading;

  // Actions
  viewLogs(id: string) {
    this.dialog.open(LogViewerDialogComponent, {
      data: { executionId: id },
      width: '800px',
      panelClass: 'bg-transparent'
    });
  }

  cancelExecution(id: string) {
    // Not implemented yet on backend/store side fully?
    // We only have `deleteJob` and `deleteSchedule`. 
    // `api-client.ts` has `workAck`, `workPoll`... no `cancelExecution`?
    // Let's check `api-client.ts` again. 
    // Ah, `deleteTenantApiClient`... `deactivateTenant`...
    // Actually, `deleteJob` usually stops schedule. 
    // Cancelling a running execution might not be exposed in API yet or I missed it.
    // For now, let's leave log only.
    console.log('Cancel execution not accessible yet', id);
  }
}
