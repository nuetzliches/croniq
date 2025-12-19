import { DatePipe, JsonPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { TenantsStore } from '@features/tenants/tenants.store';

@Component({
  selector: 'cq-tenants-page',
  imports: [DatePipe, JsonPipe],
  templateUrl: './tenants-page.html',
  providers: [TenantsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TenantsPage {
  private readonly store = inject(TenantsStore);
  private readonly tenantContext = inject(TenantContextService);

  readonly activity = this.store.activity;
  readonly lastLookup = this.store.lastLookup;
  readonly busy = this.store.busy;
  readonly lastError = this.store.lastError;

  readonly tenantId = this.tenantContext.tenantId;
  readonly environment = this.tenantContext.environment;
  readonly clientId = signal('payments-service');
  readonly scopesInput = signal('schedules:read, webhooks:write');
  readonly ttlHoursInput = signal('24');
  readonly keyId = signal('key-prod-primary');

  readonly scopesPreview = signal('schedules:read, webhooks:write');

  setClientId(value: string): void {
    this.clientId.set(value);
  }

  setScopes(value: string): void {
    this.scopesInput.set(value);
    this.scopesPreview.set(
      this.parseScopes(value)
        .join(', ')
        .trim()
    );
  }

  setTtlHours(value: string): void {
    this.ttlHoursInput.set(value);
  }

  setKeyId(value: string): void {
    this.keyId.set(value);
  }

  async issueApiKey(): Promise<void> {
    const tenantId = this.tenantId().trim();
    if (!tenantId || !this.clientId().trim()) {
      return;
    }

    await this.store.issueApiKey(
      { tenantId },
      {
        clientId: this.clientId().trim(),
        environmentTag: this.environment(),
        scopes: this.parseScopes(this.scopesInput()),
        ttlHours: this.parseNumber(this.ttlHoursInput()),
      }
    );
  }

  async rotateApiKey(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const keyId = this.keyId().trim();
    if (!tenantId || !keyId) {
      return;
    }
    await this.store.rotateApiKey({
      tenantId,
      keyId,
      environment: this.environment(),
    });
  }

  async deleteApiKey(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const keyId = this.keyId().trim();
    if (!tenantId || !keyId) {
      return;
    }
    await this.store.deleteApiKey({
      tenantId,
      keyId,
      environment: this.environment(),
    });
  }

  async lookupApiClient(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const clientId = this.clientId().trim();
    if (!tenantId || !clientId) {
      return;
    }

    await this.store.lookupApiClient({
      tenantId,
      clientId,
      environment: this.environment(),
    });
  }

  private parseScopes(value: string): string[] {
    return value
      .split(',')
      .map((scope) => scope.trim())
      .filter((scope): scope is string => Boolean(scope));
  }

  private parseNumber(value: string): number | undefined {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
}
