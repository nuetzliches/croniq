import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { UpsertJobRequest } from '@croniq/api-schema';
import { JobDialogComponent } from '@features/jobs/components/job-dialog/job-dialog.component';
import { JobRegistryEntry, JobsStore } from '@features/jobs/jobs.store';
import { CqCellDefDirective, CqColumnComponent, CqDialogService, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';
import { filter } from 'rxjs';

const DEFAULT_NAMESPACE = 'default';

@Directive({
  selector: '[cqJobCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqJobCellDirective }],
})
export class CqJobCellDirective extends CqCellDefDirective<JobRegistryEntry> {
  // Inherits ngTemplateContextGuard from base class.
}

@Component({
  selector: 'cq-jobs-page',
  imports: [DatePipe, RouterLink, DataGrid, CqColumnComponent, CqJobCellDirective, CqInputDirective, CqSelectDirective],
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
  readonly selectedRowKey = signal<string | null>(null);

  readonly namespaceOptions = computed(() => {
    const namespaces = new Set<string>();
    this.jobs().forEach((job) => {
      const value = job.namespace?.trim() || DEFAULT_NAMESPACE;
      if (value) {
        namespaces.add(value);
      }
    });
    return Array.from(namespaces).sort((a, b) => a.localeCompare(b));
  });

  readonly filteredJobs = computed(() => {
    const query = this.searchQuery().trim().toLowerCase();
    const namespaceFilter = this.namespaceFilter();
    return this.jobs().filter((job) => {
      if (namespaceFilter) {
        const jobNamespace = job.namespace?.trim() || DEFAULT_NAMESPACE;
        if (jobNamespace !== namespaceFilter) {
          return false;
        }
      }

      if (!query) {
        return true;
      }

      const haystack = [
        job.jobKey,
        job.name,
        job.namespace,
        job.description,
        job.variant,
      ]
        .filter((value): value is string => typeof value === 'string')
        .join(' ')
        .toLowerCase();
      return haystack.includes(query);
    });
  });

  readonly selectedJob = computed(() => {
    const key = this.selectedRowKey();
    if (!key) {
      return null;
    }
    return this.jobs().find((job) => this.jobRowKey(job, 0) === key) ?? null;
  });

  readonly jobDetail = this.store.jobDetail;
  readonly jobDetailLoading = this.store.jobDetailLoading;
  readonly jobDetailError = this.store.jobDetailError;
  readonly executions = this.store.executions;
  readonly executionsLoading = this.store.executionsLoading;
  readonly executionsError = this.store.executionsError;

  jobRowKey = (row: JobRegistryEntry, index: number) => row.jobKey ?? `job-${index}`;

  jobRowClasses = (row: JobRegistryEntry) =>
    row.lastExecution?.status === 'Failure' ? ['opacity-90'] : undefined;

  setSearchQuery(query: string): void {
    this.searchQuery.set(query);
  }

  setNamespaceFilter(namespace: string | null): void {
    this.namespaceFilter.set(namespace);
  }

  refresh(): void {
    void this.store.refreshJobRegistry();
    const current = this.selectedJob();
    if (current) {
      this.store.refreshJobDetail(current.jobKey);
      this.store.refreshExecutions({ jobKey: current.jobKey });
    }
  }

  triggerJob(jobKey: string): void {
    this.store.triggerJob(jobKey, {});
  }

  selectRow(event: { row: JobRegistryEntry }): void {
    const row = event.row;
    if (!row) {
      return;
    }
    const nextKey = this.jobRowKey(row, 0);
    if (this.selectedRowKey() === nextKey) {
      return;
    }
    this.selectedRowKey.set(nextKey);
    this.store.refreshJobDetail(row.jobKey);
    this.store.refreshExecutions({ jobKey: row.jobKey });
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

  clearFilters(): void {
    this.searchQuery.set('');
    this.namespaceFilter.set(null);
  }
}
