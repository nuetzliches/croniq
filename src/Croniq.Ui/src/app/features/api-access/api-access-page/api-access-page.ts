import { DatePipe } from '@angular/common';
import { CdkMenu } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { ApiClientResponse, UpsertApiClientRequest } from '@croniq/api-schema';
import { ApiAccessStore } from '@features/api-access/api-access.store';
import { ApiAccessDialogComponent } from '@features/api-access/components/api-access-dialog/api-access-dialog.component';
import { SecretDisplayDialogComponent } from '@features/api-access/components/secret-display-dialog/secret-display-dialog.component';
import { bindQueryParam } from '@shared/routing/selection-sync';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { CqConfirmDialogComponent, CqConfirmDialogData, CqDialogService } from 'ui-kit';
import { CqCellDefDirective, CqColumnComponent, CqContextMenuItemDirective, CqIconComponent, CqInputDirective, CqSelectDirective, DataGrid } from 'ui-kit';
import { filter, of, switchMap } from 'rxjs';

type ApiClientStatusFilter = 'all' | 'active' | 'inactive';

const STATUS_OPTIONS: ReadonlyArray<{ value: ApiClientStatusFilter; label: string }> = [
  { value: 'all', label: 'All statuses' },
  { value: 'active', label: 'Active' },
  { value: 'inactive', label: 'Inactive' },
];

@Directive({
  selector: '[cqApiClientCell]',
  providers: [{ provide: CqCellDefDirective, useExisting: CqApiClientCellDirective }],
})
export class CqApiClientCellDirective extends CqCellDefDirective<ApiClientResponse> {
  // Inherits ngTemplateContextGuard from base class.
}

@Component({
  selector: 'cq-api-access-page',
  imports: [
    CdkMenu,
    DatePipe,
    DataGrid,
    CqColumnComponent,
    CqApiClientCellDirective,
    CqInputDirective,
    CqSelectDirective,
    CqContextMenuItemDirective,
    CqIconComponent,
  ],
  templateUrl: './api-access-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [ApiAccessStore],
})
export class ApiAccessPage {
  private readonly store = inject(ApiAccessStore);
  private readonly dialog = inject(CqDialogService);
  private readonly shellPanel = inject(ShellPanelService);
  private readonly panelTemplate = viewChild<TemplateRef<unknown>>('apiAccessFilterPanel');
  private readonly collapsedTemplate = viewChild<TemplateRef<unknown>>('apiAccessFilterCollapsed');

  // Data
  readonly clients = this.store.clients;
  readonly isLoading = this.store.isLoading;
  readonly error = this.store.error;

  readonly clientSearch = signal('');
  readonly statusFilter = signal<ApiClientStatusFilter>('all');
  readonly environmentFilter = signal('');
  readonly selectedScopes = signal<ReadonlyArray<string>>([]);
  readonly selectedClientId = bindQueryParam({ paramKey: 'clientId' });
  readonly statusOptions = STATUS_OPTIONS;

  readonly environmentOptions = computed(() => {
    const entries = new Set<string>();
    this.clients().forEach((client) => {
      if (client.environmentTag) {
        entries.add(client.environmentTag);
      }
    });
    return Array.from(entries).sort((a, b) => a.localeCompare(b));
  });

  readonly scopeOptions = computed(() => {
    const entries = new Set<string>();
    this.clients().forEach((client) => client.scopes?.forEach((scope) => entries.add(scope)));
    return Array.from(entries).sort((a, b) => a.localeCompare(b));
  });

  readonly filteredClients = computed(() => {
    const query = this.clientSearch().trim().toLowerCase();
    const statusFilter = this.statusFilter();
    const environment = this.environmentFilter();
    const selectedScopes = new Set(this.selectedScopes());

    return this.clients().filter((client) => {
      if (statusFilter !== 'all') {
        const isActive = !!client.isActive;
        if (statusFilter === 'active' && !isActive) {
          return false;
        }
        if (statusFilter === 'inactive' && isActive) {
          return false;
        }
      }

      if (environment && client.environmentTag !== environment) {
        return false;
      }

      if (selectedScopes.size > 0) {
        const scopes = client.scopes ?? [];
        if (!scopes.some((scope) => selectedScopes.has(scope))) {
          return false;
        }
      }

      if (!query) {
        return true;
      }

      return (
        (client.name ?? '').toLowerCase().includes(query) ||
        (client.clientId ?? '').toLowerCase().includes(query) ||
        (client.environmentTag ?? '').toLowerCase().includes(query) ||
        (client.scopes ?? []).some((scope) => scope.toLowerCase().includes(query))
      );
    });
  });

  readonly selectedClient = computed(() => {
    const raw = this.selectedClientId();
    if (raw === null || raw === undefined) {
      return null;
    }
    const id = typeof raw === 'string' ? raw : String(raw);
    return this.clients().find((client) => client.clientId === id) ?? null;
  });

  clientRowKey = (row: ApiClientResponse, index: number) => row.clientId ?? `client-${index}`;

  clientRowClasses = (row: ApiClientResponse) =>
    row.isActive ? undefined : ['opacity-70'];

  constructor() {
    effect((onCleanup) => {
      const template = this.panelTemplate();
      const collapsedTemplate = this.collapsedTemplate();
      if (!template) {
        return;
      }
      this.shellPanel.setPanel(
        template,
        'Filters & settings',
        'Refine the API client list.',
        collapsedTemplate ?? null,
      );
      onCleanup(() => this.shellPanel.clearPanel(template));
    });
  }

  // Actions
  refresh() {
    this.store.refresh();
  }

  setClientSearch(query: string): void {
    this.clientSearch.set(query);
  }

  setStatusFilter(status: ApiClientStatusFilter): void {
    this.statusFilter.set(status);
  }

  setEnvironmentFilter(environment: string): void {
    this.environmentFilter.set(environment);
  }

  resetFilters(): void {
    this.clientSearch.set('');
    this.statusFilter.set('all');
    this.environmentFilter.set('');
    this.selectedScopes.set([]);
  }

  isScopeSelected(scope: string): boolean {
    return this.selectedScopes().includes(scope);
  }

  toggleScopeSelection(scope: string, checked: boolean): void {
    this.selectedScopes.update((current) =>
      checked ? Array.from(new Set([...current, scope])) : current.filter((entry) => entry !== scope),
    );
  }

  createClient(): void {
    this.openClientDialog(null, true);
  }

  editClient(client: ApiClientResponse): void {
    this.openClientDialog(client, false);
  }

  issueKey(client: ApiClientResponse) {
    if (!client.clientId) return;

    this.dialog.open<boolean>(CqConfirmDialogComponent, {
      data: {
        title: 'Generate New Key',
        message: 'Are you sure you want to generate a new API key for this client?',
        confirmLabel: 'Generate',
      } as CqConfirmDialogData,
      width: '400px',
      panelClass: 'bg-transparent',
      restoreFocus: true,
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
    this.dialog.open<boolean>(CqConfirmDialogComponent, {
      data: {
        title: 'Revoke API Client',
        message: 'Are you sure you want to revoke this API Client? This action cannot be undone.',
        confirmLabel: 'Revoke',
        variant: 'danger'
      } as CqConfirmDialogData,
      width: '400px',
      panelClass: 'bg-transparent',
      restoreFocus: true,
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
        panelClass: 'bg-transparent',
        restoreFocus: true,
      });
    }
  }

  private openClientDialog(client: ApiClientResponse | null, issueKeyOnSave: boolean): void {
    const data = client ? this.mapClientToRequest(client) : null;
    const ref = this.dialog.open<UpsertApiClientRequest>(ApiAccessDialogComponent, {
      data,
      width: '500px',
      panelClass: 'bg-transparent',
    });

    ref.closed
      .pipe(
        filter((result): result is UpsertApiClientRequest => !!result),
        switchMap((req) =>
          this.store.upsertClient(req).pipe(
            switchMap((saved) => {
              if (!issueKeyOnSave) {
                return of(null);
              }
              const clientId = saved?.clientId ?? req.clientId;
              if (!clientId) {
                return of(null);
              }
              return this.store.issueApiKey({
                clientId,
                environmentTag: req.environmentTag,
                scopes: req.scopes,
              });
            }),
          ),
        ),
      )
      .subscribe((keyResponse) => {
        if (issueKeyOnSave) {
          this.showSecret(keyResponse?.plaintextSecret);
        }
      });
  }

  private mapClientToRequest(client: ApiClientResponse): UpsertApiClientRequest {
    return {
      clientId: client.clientId ?? '',
      name: client.name ?? null,
      environmentTag: client.environmentTag ?? null,
      scopes: client.scopes ?? null,
      isActive: client.isActive ?? true,
    };
  }
}
