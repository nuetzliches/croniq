import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { WorkersStore } from './workers.store';

@Component({
  selector: 'cq-workers-page',
  imports: [DatePipe],
  templateUrl: './workers-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [WorkersStore],
})
export class WorkersPage {
  private readonly store = inject(WorkersStore);

  // Data
  readonly workers = this.store.workers;
  readonly loading = this.store.loading;

  // Metrics
  readonly activeWorkersCount = this.store.activeWorkersCount;
  readonly totalCapacity = this.store.totalCapacity;
  readonly busyThreads = this.store.busyThreads;

  refresh() {
    this.store.refresh();
  }

  // Actions
  drainWorker(id: string) {
    console.log('Drain worker', id);
  }

  deregisterWorker(id: string) {
    console.log('Deregister worker', id);
  }
}
