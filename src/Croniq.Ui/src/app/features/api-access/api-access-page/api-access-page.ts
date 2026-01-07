import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ApiAccessStore } from '@features/api-access/api-access.store';
import { Dialog } from '@angular/cdk/dialog';
import { ApiAccessDialogComponent } from '@features/api-access/components/api-access-dialog/api-access-dialog.component';
import { UpsertApiClientRequest } from '@croniq/api-schema';

@Component({
  selector: 'cq-api-access-page',
  imports: [DatePipe],
  templateUrl: './api-access-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [ApiAccessStore],
})
export class ApiAccessPage {
  private readonly store = inject(ApiAccessStore);
  private readonly dialog = inject(Dialog);

  // Data
  readonly clients = this.store.clients;
  readonly isLoading = this.store.isLoading;

  // Actions
  generateKey() {
    const ref = this.dialog.open<UpsertApiClientRequest>(ApiAccessDialogComponent, {
      data: null,
      width: '500px',
      panelClass: 'bg-transparent' // Tailwind classes on component
    });

    ref.closed.subscribe(result => {
      if (result) {
        this.store.upsertClient(result);
      }
    });
  }

  revokeKey(clientId: string) {
    if (confirm('Are you sure you want to revoke this API Client? This action cannot be undone.')) {
      this.store.deleteClient(clientId);
    }
  }
}
