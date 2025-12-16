import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { AuthSessionService } from '../auth/auth-session.service';
import { OperatorSession } from '../auth/operator-session';
import { TenantTokenEndpointService } from '../auth/token-endpoint.service';
import { TenantContextService } from './tenant-context.service';
import { TenantEnvironment } from './tenant-context.types';

@Component({
  selector: 'cq-tenant-context',
  imports: [CommonModule],
  templateUrl: './tenant-context.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TenantContext {
  private readonly tenantContext = inject(TenantContextService);
  private readonly operatorSession = inject(OperatorSession);
  private readonly authSession = inject(AuthSessionService);
  private readonly tenantTokenEndpoint = inject(TenantTokenEndpointService);
  readonly oidcBootstrapBusy = signal(false);
  readonly tokenIssuanceBusy = signal(false);
  readonly tokenIssuanceStatus = signal<string | null>(null);
  readonly tokenIssuanceError = signal<string | null>(null);

  readonly snapshot = this.tenantContext.snapshot;
  readonly operatorProfile = this.operatorSession.profile;
  readonly flags = this.tenantContext.featureFlags;
  readonly tenantLabel = this.tenantContext.tenantLabel;
  readonly sessionToken = this.authSession.sessionToken;
  readonly sessionTokenExpired = this.authSession.sessionTokenExpired;
  readonly apiKey = this.authSession.apiKey;
  readonly apiKeyExpired = this.authSession.apiKeyExpired;
  readonly environments: ReadonlyArray<TenantEnvironment> = ['dev', 'staging', 'production'];
  readonly operatorSummary = computed(() => {
    const profile = this.operatorProfile();
    return profile.impersonating ? `${profile.displayName} (impersonating)` : profile.displayName;
  });
  readonly maskedSessionToken = computed(() => this.maskSecret(this.sessionToken()));
  readonly maskedApiKey = computed(() => this.maskSecret(this.apiKey()));

  updateTenantIdentity(tenantIdInput: HTMLInputElement, tenantNameInput: HTMLInputElement): void {
    this.tenantContext.setTenantIdentity(tenantIdInput.value, tenantNameInput.value);
  }

  selectEnvironment(environment: TenantEnvironment): void {
    this.tenantContext.setEnvironment(environment);
  }

  addFeatureFlag(input: HTMLInputElement): void {
    const value = input.value.trim();
    if (!value) {
      return;
    }
    this.tenantContext.addFeatureFlag(value);
    input.value = '';
  }

  removeFeatureFlag(flag: string): void {
    this.tenantContext.removeFeatureFlag(flag);
  }

  updateOperatorName(value: string): void {
    this.operatorSession.updateProfile({ displayName: value });
  }

  updateOperatorEmail(value: string): void {
    this.operatorSession.updateProfile({ email: value });
  }

  toggleImpersonation(): void {
    const current = this.operatorProfile();
    this.operatorSession.updateProfile({ impersonating: !current.impersonating });
  }

  storeSessionToken(tokenInput: HTMLInputElement, expiryInput?: HTMLInputElement): void {
    const expiresAt = this.resolveExpiry(expiryInput?.value);
    this.authSession.storeSessionToken(tokenInput.value, { expiresAt });
    tokenInput.value = '';
    if (expiryInput) {
      expiryInput.value = '';
    }
  }

  clearSessionToken(): void {
    this.authSession.clearSessionToken();
  }

  storeApiKey(keyInput: HTMLInputElement, labelInput?: HTMLInputElement, expiryInput?: HTMLInputElement): void {
    const expiresAt = this.resolveExpiry(expiryInput?.value);
    const label = labelInput?.value?.trim() || null;
    this.authSession.storeApiKey(keyInput.value, { expiresAt, label });
    keyInput.value = '';
    if (labelInput) {
      labelInput.value = '';
    }
    if (expiryInput) {
      expiryInput.value = '';
    }
  }

  clearApiKey(): void {
    this.authSession.clearApiKey();
  }

  async issueTenantToken(
    clientIdInput: HTMLInputElement,
    ttlInput: HTMLInputElement,
    scopesInput: HTMLInputElement,
    labelInput: HTMLInputElement,
    storeResultInput: HTMLInputElement,
  ): Promise<void> {
    if (this.tokenIssuanceBusy()) {
      return;
    }

    const clientId = clientIdInput.value.trim();
    if (!clientId) {
      this.tokenIssuanceError.set('Client ID ist erforderlich, um einen Token anzufordern.');
      return;
    }

    const ttlHours = this.parseNumber(ttlInput.value);
    const scopes = this.parseScopes(scopesInput.value);
    const label = labelInput.value?.trim() || null;
    const persist = storeResultInput.checked;
    const snapshot = this.snapshot();

    this.tokenIssuanceBusy.set(true);
    this.tokenIssuanceError.set(null);
    this.tokenIssuanceStatus.set(null);

    try {
      const fallbackExpiry = this.estimateExpiry(ttlHours);
      const result = await this.tenantTokenEndpoint.issueTenantToken({
        tenantId: snapshot.tenantId,
        clientId,
        environmentTag: snapshot.environment,
        scopes,
        ttlHours,
        label,
        persistInSession: persist,
        fallbackExpiry,
      });

      if (result.storedInSession) {
        this.tokenIssuanceStatus.set('Token ausgegeben und sicher in der Sitzung gespeichert.');
      } else if (result.token) {
        this.tokenIssuanceStatus.set('Token ausgegeben – bitte sofort sicher notieren.');
      } else {
        this.tokenIssuanceStatus.set('Anfrage gesendet. Token-Ausgabe folgt durch den Backend-Service.');
      }

      clientIdInput.value = '';
      ttlInput.value = '';
      scopesInput.value = '';
      labelInput.value = '';
      storeResultInput.checked = true;
    } catch (error) {
      this.tokenIssuanceError.set(error instanceof Error ? error.message : 'Token-Anfrage fehlgeschlagen.');
    } finally {
      this.tokenIssuanceBusy.set(false);
    }
  }

  async startOidcBootstrap(): Promise<void> {
    if (this.oidcBootstrapBusy()) {
      return;
    }
    this.oidcBootstrapBusy.set(true);
    try {
      await this.authSession.startOidcBootstrap();
    } finally {
      this.oidcBootstrapBusy.set(false);
    }
  }

  private resolveExpiry(value?: string): string | null {
    if (!value) {
      return null;
    }
    const timestamp = Date.parse(value);
    return Number.isFinite(timestamp) ? new Date(timestamp).toISOString() : null;
  }

  private maskSecret(secret: { value: string | undefined | null } | null): string {
    const raw = secret?.value?.trim();
    if (!raw) {
      return '—';
    }
    if (raw.length <= 4) {
      return raw;
    }
    return `•••• ${raw.slice(-4)}`;
  }

  private parseScopes(raw: string | null | undefined): string[] | undefined {
    if (!raw) {
      return undefined;
    }
    const scopes = raw
      .split(/[,\s]+/)
      .map((scope) => scope.trim())
      .filter(Boolean);
    return scopes.length ? scopes : undefined;
  }

  private parseNumber(value: string | null | undefined): number | null {
    if (!value) {
      return null;
    }
    const parsed = Number(value);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }

  private estimateExpiry(ttlHours: number | null): string | null {
    if (!ttlHours) {
      return null;
    }
    const expiresAt = Date.now() + ttlHours * 60 * 60 * 1000;
    return new Date(expiresAt).toISOString();
  }
}
