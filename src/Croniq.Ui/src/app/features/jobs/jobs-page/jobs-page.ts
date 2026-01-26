import { CdkMenu } from '@angular/cdk/menu';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { UpsertJobRequest } from '@croniq/api-schema';
import { JobDialogComponent } from '@features/jobs/components/job-dialog/job-dialog.component';
import { JobRegistryEntry, JobsStore } from '@features/jobs/jobs.store';
import { bindQueryParam } from '@shared/routing/selection-sync';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { filter } from 'rxjs';
import { CqCellDefDirective, CqColumnComponent, CqContextMenuItemDirective, CqDialogService, CqIconComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';

const DEFAULT_NAMESPACE = 'default';

type JobFilterEntry = {
  jobKey: string;
  status: 'success' | 'failure' | 'canceled' | 'unknown';
  scheduleCount: number;
};

@Directive({
  selector: '[cqJobCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqJobCellDirective }],
})
export class CqJobCellDirective extends CqCellDefDirective<JobRegistryEntry> {
  // Inherits ngTemplateContextGuard from base class.
}

@Component({
  selector: 'cq-jobs-page',
  imports: [CdkMenu, DatePipe, RouterLink, DataGrid, CqColumnComponent, CqJobCellDirective, CqInputDirective, CqSelectDirective, CqContextMenuItemDirective, CqIconComponent],
  providers: [JobsStore],
  templateUrl: './jobs-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class JobsPage {
  private readonly store = inject(JobsStore);
  private readonly dialog = inject(CqDialogService);
  private readonly router = inject(Router);
  private readonly shellPanel = inject(ShellPanelService);
  private readonly panelTemplate = viewChild<TemplateRef<unknown>>('jobsFilterPanel');
  private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('jobsFilterCollapsed');

  readonly jobs = this.store.jobRegistry;
  readonly loading = this.store.jobRegistryLoading;
  readonly error = this.store.jobRegistryError;
  readonly triggerError = this.store.lastError;

  readonly jobSearch = signal('');
  readonly namespaceFilter = signal<string | null>(null);
  readonly selectedJobKey = bindQueryParam({ paramKey: 'jobKey' });
  readonly selectedJobFilterKeys = signal<ReadonlyArray<string>>([]);

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

  readonly jobEntries = computed<ReadonlyArray<JobFilterEntry>>(() =>
    this.jobs().map((job) => ({
      jobKey: job.jobKey,
      status: normalizeJobStatus(job.lastExecution?.status),
      scheduleCount: job.scheduleCount,
    })),
  );

  readonly visibleJobEntries = computed<ReadonlyArray<JobFilterEntry>>(() => {
    const query = this.jobSearch().trim().toLowerCase();
    if (!query) {
      return this.jobEntries();
    }
    return this.jobEntries().filter((entry) => entry.jobKey.toLowerCase().includes(query));
  });

  readonly filteredJobs = computed(() => {
    const namespaceFilter = this.namespaceFilter();
    const selectedJobs = new Set(this.selectedJobFilterKeys());

    return this.jobs().filter((job) => {
      if (namespaceFilter) {
        const jobNamespace = job.namespace?.trim() || DEFAULT_NAMESPACE;
        if (jobNamespace !== namespaceFilter) {
          return false;
        }
      }

      if (selectedJobs.size > 0 && !selectedJobs.has(job.jobKey)) {
        return false;
      }

      return true;
    });
  });

  readonly selectedJob = computed(() => {
    const raw = this.selectedJobKey();
    if (raw === null || raw === undefined) {
      return null;
    }
    const key = typeof raw === 'string' ? raw : String(raw);
    return this.jobs().find((job) => job.jobKey === key) ?? null;
  });

  readonly jobDetail = this.store.jobDetail;
  readonly jobDetailLoading = this.store.jobDetailLoading;
  readonly jobDetailError = this.store.jobDetailError;
  readonly deleteJobLoading = this.store.deleteJobLoading;
  readonly deleteJobError = this.store.deleteJobError;
  readonly toggleSchedulesLoading = this.store.toggleSchedulesLoading;
  readonly toggleSchedulesError = this.store.toggleSchedulesError;
  readonly executions = this.store.executions;
  readonly executionsLoading = this.store.executionsLoading;
  readonly executionsError = this.store.executionsError;

  jobRowKey = (row: JobRegistryEntry, index: number) => row.jobKey ?? `job-${index}`;

  jobRowClasses = (row: JobRegistryEntry) =>
    row.lastExecution?.status === 'Failure' ? ['opacity-90'] : undefined;

  constructor() {
    effect((onCleanup) => {
      const template = this.panelTemplate();
      const collapsedTemplate = this.collapsedTemplate();
      if (!template) {
        return;
      }
      this.shellPanel.setPanel(
        template,
        'Search & filters',
        'Refine the job registry view.',
        collapsedTemplate ?? null,
      );
      onCleanup(() => this.shellPanel.clearPanel(template));
    });

    effect(() => {
      const raw = this.selectedJobKey();
      if (raw === null || raw === undefined) {
        return;
      }
      const key = typeof raw === 'string' ? raw : String(raw);
      this.store.refreshJobDetail(key);
      this.store.refreshExecutions({ jobKey: key });
    });
  }

  setJobSearch(query: string): void {
    this.jobSearch.set(query);
  }

  setNamespaceFilter(namespace: string | null): void {
    this.namespaceFilter.set(namespace);
  }

  refresh(): void {
    void this.store.refreshJobRegistry();
    const raw = this.selectedJobKey();
    if (raw !== null && raw !== undefined) {
      const current = typeof raw === 'string' ? raw : String(raw);
      this.store.refreshJobDetail(current);
      this.store.refreshExecutions({ jobKey: current });
    }
  }

  triggerJob(jobKey: string): void {
    this.store.triggerJob(jobKey, {});
  }

  openSchedulesForJob(jobKey: string): void {
    void this.router.navigate(['/schedules'], { queryParams: { jobKey } });
  }

  openWebhooksForJob(jobKey: string): void {
    void this.router.navigate(['/webhooks'], { queryParams: { jobKey } });
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
      .open<boolean>(JobDialogComponent, {
        data,
        panelClass: 'dialog-panel', // Ensure this class is defined in global styles or component styles
      })
      .closed.pipe(filter((result): result is boolean => !!result))
      .subscribe(() => {
        this.store.refreshJobRegistry();
      });
  }

  deleteJob(job: JobRegistryEntry): void {
    if (job.isSeeded) {
      return;
    }
    if (confirm(`Delete ${job.jobKey}? This removes the job and all schedules.`)) {
      this.store.deleteJob(job.jobKey);
    }
  }

  disableSchedules(job: JobRegistryEntry): void {
    if (job.isSeeded) {
      return;
    }
    this.store.setJobSchedulesEnabled(job.jobKey, false);
  }

  enableSchedules(job: JobRegistryEntry): void {
    if (job.isSeeded) {
      return;
    }
    this.store.setJobSchedulesEnabled(job.jobKey, true);
  }

  hasActiveSchedules(job: JobRegistryEntry): boolean {
    return job.activeScheduleCount > 0;
  }

  hasDisabledSchedules(job: JobRegistryEntry): boolean {
    return job.activeScheduleCount < job.scheduleCount;
  }

  isSeedLocked(job: JobRegistryEntry): boolean {
    return job.isSeeded;
  }

  resetFilters(): void {
    this.jobSearch.set('');
    this.namespaceFilter.set(null);
    this.selectedJobFilterKeys.set([]);
  }

  isJobSelected(jobKey: string): boolean {
    return this.selectedJobFilterKeys().includes(jobKey);
  }

  toggleJobSelection(jobKey: string, checked: boolean): void {
    this.selectedJobFilterKeys.update((current) =>
      checked
        ? Array.from(new Set([...current, jobKey]))
        : current.filter((entry) => entry !== jobKey),
    );
  }
}

function normalizeJobStatus(status: string | null | undefined): JobFilterEntry['status'] {
  const normalized = (status ?? '').toLowerCase();
  if (normalized === 'success' || normalized === 'succeeded') {
    return 'success';
  }
  if (normalized === 'failure' || normalized === 'failed') {
    return 'failure';
  }
  if (normalized === 'canceled' || normalized === 'cancelled') {
    return 'canceled';
  }
  return 'unknown';
}
