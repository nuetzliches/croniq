import { ChangeDetectionStrategy, Component, signal, computed } from '@angular/core';
import { CommonModule } from '@angular/common';

interface Runner {
  id: string;
  hostname: string;
  status: 'Online' | 'Offline' | 'Draining';
  lastHeartbeat: Date;
  activeJobs: number;
  capacity: number;
  tags: string[];
}

@Component({
  selector: 'cq-runners-page',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './runners-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RunnersPage {
  // Data
  runners = signal<Runner[]>([
    { id: 'run-01', hostname: 'worker-prod-01', status: 'Online', lastHeartbeat: new Date(), activeJobs: 3, capacity: 10, tags: ['linux', 'gpu'] },
    { id: 'run-02', hostname: 'worker-prod-02', status: 'Online', lastHeartbeat: new Date(), activeJobs: 8, capacity: 10, tags: ['linux'] },
    { id: 'run-03', hostname: 'worker-prod-03', status: 'Draining', lastHeartbeat: new Date(Date.now() - 5000), activeJobs: 1, capacity: 10, tags: ['windows'] },
    { id: 'run-04', hostname: 'worker-prod-04', status: 'Offline', lastHeartbeat: new Date(Date.now() - 3600000), activeJobs: 0, capacity: 10, tags: ['linux'] },
  ]);

  // Metrics
  activeRunnersCount = computed(() => this.runners().filter(r => r.status === 'Online').length);
  totalCapacity = computed(() => this.runners().reduce((acc, r) => acc + r.capacity, 0));
  busyThreads = computed(() => this.runners().reduce((acc, r) => acc + r.activeJobs, 0));

  // Actions
  drainRunner(id: string) {
    console.log('Drain runner', id);
  }

  deregisterRunner(id: string) {
    console.log('Deregister runner', id);
  }
}
