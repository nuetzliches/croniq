import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

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

  readonly snapshot = this.tenantContext.snapshot;
  readonly flags = this.tenantContext.featureFlags;
  readonly tenantLabel = this.tenantContext.tenantLabel;
  readonly environments: ReadonlyArray<TenantEnvironment> = ['dev', 'staging', 'production'];

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

}
