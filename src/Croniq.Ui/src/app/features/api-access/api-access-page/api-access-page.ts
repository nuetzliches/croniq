import { Dialog } from '@angular/cdk/dialog';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ApiClientResponse, UpsertApiClientRequest } from '@croniq/api-schema';
import { ApiAccessStore } from '@features/api-access/api-access.store';
import { ApiAccessDialogComponent } from '@features/api-access/components/api-access-dialog/api-access-dialog.component';
import { SecretDisplayDialogComponent } from '@features/api-access/components/secret-display-dialog/secret-display-dialog.component';
import { ConfirmDialogComponent, ConfirmDialogData } from '@shared/components/confirm-dialog/confirm-dialog.component';
import { filter, of, switchMap } from 'rxjs';

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

    ref.closed.pipe(
      filter((result): result is UpsertApiClientRequest => !!result),
      switchMap(req => this.store.upsertClient(req).pipe(
        switchMap(client => {
          if (!client || !client.clientId) return of(null);
          // Automatically issue the first key
          return this.store.issueApiKey({
            clientId: client.clientId,
            environmentTag: client.environmentTag,
            scopes: client.scopes
          });
        })
      ))
    ).subscribe(keyResponse => {
      this.showSecret(keyResponse?.plaintextSecret);
    });
  }

  issueKey(client: ApiClientResponse) {
    if (!client.clientId) return;

    this.dialog.open<boolean>(ConfirmDialogComponent, {
      data: {
        title: 'Generate New Key',
        message: 'Are you sure you want to generate a new API key for this client?',
        confirmLabel: 'Generate',
      } as ConfirmDialogData,
      width: '400px',
      panelClass: 'bg-transparent'
    }).closed.pipe(
      filter(result => !!result)
    ).subscribe(() => {
      this.store.issueApiKey({
        clientId: client.clientId!,
        environmentTag: client.environmentTag,
        scopes: client.scopes
      }).subscribe(keyResponse => {
        this.showSecret(keyResponse?.plaintextSecret);
      });
    });
  }

  revokeKey(clientId: string) {
    this.dialog.open<boolean>(ConfirmDialogComponent, {
      data: {
        title: 'Revoke API Client',
        message: 'Are you sure you want to revoke this API Client? This action cannot be undone.',
        confirmLabel: 'Revoke',
        variant: 'danger'
      } as ConfirmDialogData,
      width: '400px',
      panelClass: 'bg-transparent'
    }).closed.pipe(
      filter(result => !!result)
    ).subscribe(() => {
      this.store.deleteClient(clientId);
    });
  }

  private showSecret(secret: string | null | undefined) {
    if (secret) {
      this.dialog.open(SecretDisplayDialogComponent, {
        data: { secret },
        width: '500px',
        panelClass: 'bg-transparent'
      });
    }
  }
}
