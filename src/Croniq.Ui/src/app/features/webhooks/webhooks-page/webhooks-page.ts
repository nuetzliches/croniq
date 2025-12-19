import { Tab, TabContent, TabList, TabPanel, Tabs } from '@angular/aria/tabs';
import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { CreateWebhookIpRuleRequest, RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { WebhooksStore } from '@features/webhooks/webhooks.store';

type DetailTab = {
  id: 'controls' | 'endpoints' | 'ops';
  label: string;
};

@Component({
  selector: 'cq-webhooks-page',
  imports: [DatePipe, Tabs, TabList, Tab, TabPanel, TabContent],
  providers: [WebhooksStore],
  templateUrl: './webhooks-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhooksPage {
  private readonly store = inject(WebhooksStore);
  private readonly tenantContext = inject(TenantContextService);

  readonly detailTabs: ReadonlyArray<DetailTab> = [
    { id: 'controls', label: 'Controls' },
    { id: 'endpoints', label: 'Endpoints' },
    { id: 'ops', label: 'Ops log' },
  ];

  readonly selectedTab = signal<string>(this.detailTabs[0]?.id ?? '');

  setSelectedTab(nextTab: string | null | undefined): void {
    this.selectedTab.set(nextTab ?? this.detailTabs[0]?.id ?? '');
  }

  readonly endpoints = this.store.endpoints;
  readonly actionLog = this.store.actionLog;
  readonly loading = this.store.loading;
  readonly deadLetterCount = this.store.deadLetterCount;
  readonly lastError = this.store.lastError;
  readonly activeCount = this.store.activeCount;

  readonly tenantId = this.tenantContext.tenantId;
  readonly environment = this.tenantContext.environment;
  readonly hookKey = signal('billing-updates');
  readonly jobKey = signal('jobs.billing-webhook');
  readonly requestsPerMinute = signal('120');
  readonly allowUnsigned = signal(false);
  readonly requireSignature = signal(true);
  readonly ipRuleId = signal('rule-allow-vpn');
  readonly ipRuleCidr = signal('10.0.0.0/24');
  readonly ipRuleDescription = signal('Ops VPN');
  readonly deadLetterId = signal('dl-001');
  readonly secretActivateDelay = signal('60');
  readonly secretGracePeriod = signal('600');

  setHookKey(value: string): void {
    this.hookKey.set(value);
  }

  setJobKey(value: string): void {
    this.jobKey.set(value);
  }

  setRequestsPerMinute(value: string): void {
    this.requestsPerMinute.set(value);
  }

  toggleAllowUnsigned(): void {
    this.allowUnsigned.set(!this.allowUnsigned());
  }

  toggleRequireSignature(): void {
    this.requireSignature.set(!this.requireSignature());
  }

  setIpRuleId(value: string): void {
    this.ipRuleId.set(value);
  }

  setIpRuleCidr(value: string): void {
    this.ipRuleCidr.set(value);
  }

  setIpRuleDescription(value: string): void {
    this.ipRuleDescription.set(value);
  }

  setDeadLetterId(value: string): void {
    this.deadLetterId.set(value);
  }

  setSecretActivateDelay(value: string): void {
    this.secretActivateDelay.set(value);
  }

  setSecretGracePeriod(value: string): void {
    this.secretGracePeriod.set(value);
  }

  async refreshEndpoints(): Promise<void> {
    const tenantId = this.tenantId().trim();
    if (!tenantId) {
      return;
    }
    await this.store.refreshEndpoints({ tenantId, environment: this.environment() });
  }

  async upsertEndpoint(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const hookKey = this.hookKey().trim();
    const jobKey = this.jobKey().trim();
    if (!tenantId || !hookKey || !jobKey) {
      return;
    }

    const payload: UpsertWebhookEndpointRequest = {
      hookKey,
      jobKey,
      enabled: true,
      requireSignature: this.requireSignature(),
      requestsPerMinute: this.parseNumber(this.requestsPerMinute()),
      secret: null,
      metadata: {},
      signatureVersion: 1,
    };

    await this.store.upsertEndpoint(
      {
        tenantId,
        environment: this.environment(),
        hookKey,
        allowUnsigned: this.allowUnsigned(),
      },
      payload,
    );
  }

  async deleteEndpoint(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const hookKey = this.hookKey().trim();
    if (!tenantId || !hookKey) {
      return;
    }
    await this.store.deleteEndpoint({
      tenantId,
      environment: this.environment(),
      hookKey,
    });
  }

  async rotateSecret(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const hookKey = this.hookKey().trim();
    if (!tenantId || !hookKey) {
      return;
    }
    const payload: RotateWebhookSecretRequest = {
      activateInSeconds: this.parseNumber(this.secretActivateDelay()),
      gracePeriodSeconds: this.parseNumber(this.secretGracePeriod()),
      notes: 'UI-initiated rotation',
    };
    await this.store.rotateSecret(
      {
        tenantId,
        environment: this.environment(),
        hookKey,
      },
      payload,
    );
  }

  async createIpRule(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const hookKey = this.hookKey().trim();
    if (!tenantId || !hookKey) {
      return;
    }
    const payload: CreateWebhookIpRuleRequest = {
      cidr: this.ipRuleCidr().trim(),
      description: this.ipRuleDescription().trim() || undefined,
    };
    await this.store.createIpRule(
      {
        tenantId,
        environment: this.environment(),
        hookKey,
      },
      payload,
    );
  }

  async deleteIpRule(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const hookKey = this.hookKey().trim();
    const ruleId = this.ipRuleId().trim();
    if (!tenantId || !hookKey || !ruleId) {
      return;
    }
    await this.store.deleteIpRule({
      tenantId,
      environment: this.environment(),
      hookKey,
      ruleId,
    });
  }

  async replayDeadLetter(): Promise<void> {
    const tenantId = this.tenantId().trim();
    const deadLetterId = this.deadLetterId().trim();
    if (!tenantId || !deadLetterId) {
      return;
    }
    await this.store.replayDeadLetter({
      tenantId,
      environment: this.environment(),
      deadLetterId,
    });
  }

  async invokeWebhook(): Promise<void> {
    const hookKey = this.hookKey().trim();
    if (!hookKey) {
      return;
    }
    await this.store.invokeWebhook({ hookKey });
  }

  private parseNumber(value: string): number | undefined {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
}
