import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { UpsertJobRequest } from '@croniq/api-schema';
import { JobDialogComponent } from '@features/jobs/components/job-dialog/job-dialog.component';
import { JobRegistryEntry, JobsStore } from '@features/jobs/jobs.store';
import { CqDialogService } from 'ui-kit';
import { filter } from 'rxjs';

@Component({
  selector: 'cq-jobs-page',
  imports: [DatePipe],
  providers: [JobsStore],
  templateUrl: './jobs-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class JobsPage {
  private readonly store = inject(JobsStore);
  private readonly dialog = inject(CqDialogService);

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

  triggerJob(jobKey: string): void {
    this.store.triggerJob(jobKey, {});
  }

  openJobDialog(job?: JobRegistryEntry): void {
    const data: UpsertJobRequest | undefined = job
      ? {
        jobKey: job.jobKey,
        namespace: job.namespace || 'default',
        name: job.name || job.jobKey,
        variant: job.variant || undefined,
        description: job.description || undefined,
        metadata: job.metadata || undefined,
      }
      : undefined;

    this.dialog
      .open<UpsertJobRequest>(JobDialogComponent, {
        data,
        panelClass: 'dialog-panel', // Ensure this class is defined in global styles or component styles
      })
      .closed.pipe(filter((result): result is UpsertJobRequest => !!result))
      .subscribe((payload) => {
        this.store.upsertJob(payload);
      });
  }
}
