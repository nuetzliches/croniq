import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import type { ManualTriggerEntry } from '../jobs.store';
import { JobsStore } from '../jobs.store';

@Component({
  selector: 'cq-jobs-page',
  imports: [CommonModule],
  templateUrl: './jobs-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class JobsPage {
  private readonly store = inject(JobsStore);

  readonly manualTriggers = this.store.manualTriggers;
  readonly pendingCount = this.store.pendingCount;
  readonly lastError = this.store.lastError;

  readonly jobKey = signal('nightly-billing-sweep');
  readonly metadataSource = signal('tenant=cron-lab\nsource=ui');

  readonly metadataPreview = computed(() =>
    Object.entries(this.parseMetadata(this.metadataSource()))
      .map(([key, value]) => `${key}: ${value}`)
      .join(', ')
  );

  setJobKey(value: string): void {
    this.jobKey.set(value);
  }

  setMetadataSource(value: string): void {
    this.metadataSource.set(value);
  }

  async queueManualTrigger(): Promise<void> {
    const metadata = this.parseMetadata(this.metadataSource());
    await this.store.triggerJob(this.jobKey(), metadata);
  }

  hasMetadata(entry: ManualTriggerEntry): boolean {
    return Object.keys(entry.metadata).length > 0;
  }

  metadataEntries(entry: ManualTriggerEntry): ReadonlyArray<{ key: string; value: string }> {
    return Object.entries(entry.metadata).map(([key, value]) => ({ key, value }));
  }

  private parseMetadata(input: string): Record<string, string> {
    return input
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean)
      .reduce<Record<string, string>>((acc, line) => {
        const [rawKey, ...rawValue] = line.split('=');
        const key = rawKey?.trim();
        const value = rawValue.join('=').trim();
        if (key) {
          acc[key] = value || '';
        }
        return acc;
      }, {});
  }
}
