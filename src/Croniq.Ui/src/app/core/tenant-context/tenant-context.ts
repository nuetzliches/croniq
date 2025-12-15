import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { AuthSessionService } from '../auth/auth-session.service';
import { OperatorSession } from '../auth/operator-session';
import { TenantContextService } from './tenant-context.service';
import { TenantEnvironment } from './tenant-context.types';

@Component({
  selector: 'app-tenant-context',
  imports: [CommonModule],
  templateUrl: './tenant-context.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TenantContext {
  private readonly tenantContext = inject(TenantContextService);
  private readonly operatorSession = inject(OperatorSession);
  private readonly authSession = inject(AuthSessionService);
  readonly oidcBootstrapBusy = signal(false);

  readonly snapshot = this.tenantContext.snapshot;
  readonly operatorProfile = this.operatorSession.profile;
  readonly flags = this.tenantContext.featureFlags;
  readonly tenantLabel = this.tenantContext.tenantLabel;
  readonly tenantPresets = this.tenantContext.presets;
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

  selectTenant(tenantId: string): void {
    this.tenantContext.applyPreset(tenantId);
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
}
