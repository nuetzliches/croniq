import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { CommonModule } from '@angular/common';

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

  readonly snapshot = this.tenantContext.snapshot;
  readonly operatorProfile = this.operatorSession.profile;
  readonly flags = this.tenantContext.featureFlags;
  readonly tenantLabel = this.tenantContext.tenantLabel;
  readonly tenantPresets = this.tenantContext.presets;
  readonly environments: ReadonlyArray<TenantEnvironment> = ['dev', 'staging', 'production'];
  readonly operatorSummary = computed(() => {
    const profile = this.operatorProfile();
    return profile.impersonating ? `${profile.displayName} (impersonating)` : profile.displayName;
  });

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
}
