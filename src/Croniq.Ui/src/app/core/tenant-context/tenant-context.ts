import { DatePipe, UpperCasePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { TenantContextService } from './tenant-context.service';
import { TenantEnvironment } from './tenant-context.types';

@Component({
  selector: 'cq-tenant-context',
  imports: [DatePipe, UpperCasePipe],
  templateUrl: './tenant-context.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TenantContext {
  private readonly tenantContext = inject(TenantContextService);

  readonly snapshot = this.tenantContext.snapshot;
  readonly flags = this.tenantContext.featureFlags;

  // Environment selection is token-bound; without a discovery endpoint we only show the current environment.
  readonly environments = computed<ReadonlyArray<TenantEnvironment>>(() => [this.snapshot().environment]);

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
