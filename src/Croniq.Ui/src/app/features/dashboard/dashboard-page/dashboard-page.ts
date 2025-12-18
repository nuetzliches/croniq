import { Tab, TabContent, TabList, TabPanel, Tabs } from '@angular/aria/tabs';
import { ChangeDetectionStrategy, Component, signal } from '@angular/core';

type DetailTab = {
  id: 'metrics';
  label: string;
};

type SummaryCard = {
  label: string;
  value: string;
  description: string;
};

@Component({
  selector: 'cq-dashboard-page',
  imports: [Tabs, TabList, Tab, TabPanel, TabContent],
  templateUrl: './dashboard-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DashboardPage {
  readonly detailTabs: ReadonlyArray<DetailTab> = [{ id: 'metrics', label: 'Metrics' }];
  readonly selectedTab = signal<string>(this.detailTabs[0]?.id ?? '');

  setSelectedTab(nextTab: string | null | undefined): void {
    this.selectedTab.set(nextTab ?? this.detailTabs[0]?.id ?? '');
  }

  readonly summaryCards = signal<ReadonlyArray<SummaryCard>>([
    { label: 'Active schedules', value: '128', description: 'Enabled policies across tenants' },
    { label: 'Queue depth', value: '42', description: 'Waiting jobs in the last minute' },
    { label: 'Misfires today', value: '3', description: 'Automatically retried triggers' },
    { label: 'Avg. webhook latency', value: '210 ms', description: 'p95 delivery round trip' },
  ]);
}
