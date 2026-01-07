import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RunnersStore } from './runners.store';

@Component({
  selector: 'cq-runners-page',
  imports: [DatePipe],
  templateUrl: './runners-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [RunnersStore],
})
export class RunnersPage {
  private readonly store = inject(RunnersStore);

  // Data
  readonly runners = this.store.runners;
  readonly loading = this.store.loading;

  // Metrics
  readonly activeRunnersCount = this.store.activeRunnersCount;
  readonly totalCapacity = this.store.totalCapacity;
  readonly busyThreads = this.store.busyThreads;

  refresh() {
    this.store.refresh();
  }

  // Actions
  drainRunner(id: string) {
    console.log('Drain runner', id);
  }

  deregisterRunner(id: string) {
    console.log('Deregister runner', id);
  }
}
