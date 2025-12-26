import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { JobsStore } from '@features/jobs/jobs.store';

@Component({
  selector: 'cq-jobs-page',
  imports: [],
  providers: [JobsStore],
  templateUrl: './jobs-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class JobsPage {
  private readonly store = inject(JobsStore);

  readonly jobs = this.store.jobRegistry;
  readonly loading = this.store.jobRegistryLoading;
  readonly error = this.store.jobRegistryError;

  readonly searchQuery = signal('');
  readonly namespaceFilter = signal<string | null>(null);

  setSearchQuery(query: string): void {
    this.searchQuery.set(query);
  }

  setNamespaceFilter(namespace: string | null): void {
    this.namespaceFilter.set(namespace);
  }

  refresh(): void {
    void this.store.refreshJobRegistry();
  }
}
