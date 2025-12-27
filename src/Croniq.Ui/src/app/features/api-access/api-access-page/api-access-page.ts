import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, signal } from '@angular/core';

interface ApiKey {
  id: string;
  name: string;
  prefix: string;
  createdAt: Date;
  lastUsed: Date | null;
  scopes: string[];
}

@Component({
  selector: 'cq-api-access-page',
  imports: [DatePipe],
  templateUrl: './api-access-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ApiAccessPage {
  // Data
  apiKeys = signal<ApiKey[]>([
    { id: 'key-1', name: 'CI/CD Pipeline', prefix: 'cq_live_8f...', createdAt: new Date('2023-01-15'), lastUsed: new Date(), scopes: ['jobs:write', 'deployments:read'] },
    { id: 'key-2', name: 'Developer Local', prefix: 'cq_test_9a...', createdAt: new Date('2023-03-10'), lastUsed: new Date(Date.now() - 86400000), scopes: ['*'] },
    { id: 'key-3', name: 'Monitoring Service', prefix: 'cq_live_7b...', createdAt: new Date('2023-02-20'), lastUsed: null, scopes: ['metrics:read'] },
  ]);

  // Actions
  generateKey() {
    console.log('Generate new key');
  }

  revokeKey(id: string) {
    console.log('Revoke key', id);
  }
}
