import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject, linkedSignal, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormField, form } from '@angular/forms/signals';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { CreateWebhookIpRuleRequest } from '@croniq/api-schema';
import { WebhookEndpointView, WebhookIpRuleView, WebhooksStore } from '@features/webhooks/webhooks.store';
import { CqDialogComponent, CqDialogHeaderDirective, CqFormFieldComponent, CqInputDirective, CqTextareaDirective } from 'ui-kit';

type IpRuleFormModel = {
    cidr: string;
    description: string;
};

type WebhookIpRulesDialogData = {
    endpoint: WebhookEndpointView;
};

@Component({
    selector: 'cq-webhook-ip-rules-dialog',
    imports: [
        FormField,
        CqDialogComponent,
        CqDialogHeaderDirective,
        CqFormFieldComponent,
        CqInputDirective,
        CqTextareaDirective,
    ],
    templateUrl: './webhook-ip-rules-dialog.component.html',
    providers: [WebhooksStore],
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhookIpRulesDialogComponent {
    private readonly dialogRef = inject(DialogRef);
    private readonly tenantContext = inject(TenantContextService);
    private readonly store = inject(WebhooksStore);
    private readonly destroyRef = inject(DestroyRef);

    readonly data = inject<WebhookIpRulesDialogData>(DIALOG_DATA);
    readonly endpoint = signal<WebhookEndpointView | null>(this.data?.endpoint ?? null);
    readonly rules = this.store.ipRules;

    private readonly formModel = linkedSignal<IpRuleFormModel>(() => ({
        cidr: '',
        description: '',
    }));

    readonly form = form(this.formModel, () => { });

    readonly cidrError = computed(() => {
        const cidr = this.formModel().cidr.trim();
        if (!cidr) {
            return 'CIDR is required.';
        }
        return null;
    });

    readonly canSubmit = computed(() => Boolean(this.endpoint()) && !this.cidrError());

    constructor() {
        const endpoint = this.endpoint();
        if (endpoint) {
            queueMicrotask(() => this.store.selectHook(endpoint.hookKey));
        }

        this.dialogRef.closed
            .pipe(takeUntilDestroyed(this.destroyRef))
            .subscribe(() => this.store.selectHook(''));
    }

    close(): void {
        this.dialogRef.close();
    }

    submit(): void {
        if (!this.canSubmit()) {
            return;
        }
        const endpoint = this.endpoint();
        if (!endpoint) {
            return;
        }
        const cidr = this.formModel().cidr.trim();
        if (!cidr) {
            return;
        }
        const description = this.formModel().description.trim();
        const payload: CreateWebhookIpRuleRequest = {
            cidr,
            description: description ? description : null,
        };

        const tenantId = this.tenantContext.tenantId();
        const environment = this.tenantContext.environment();
        if (!tenantId) {
            return;
        }

        this.store.createIpRule({
            tenantId,
            environment,
            hookKey: endpoint.hookKey,
        }, payload);

        this.formModel.set({
            cidr: '',
            description: '',
        });
    }

    removeRule(rule: WebhookIpRuleView): void {
        const endpoint = this.endpoint();
        if (!endpoint) {
            return;
        }
        const tenantId = this.tenantContext.tenantId();
        const environment = this.tenantContext.environment();
        if (!tenantId) {
            return;
        }
        this.store.deleteIpRule({
            tenantId,
            environment,
            hookKey: endpoint.hookKey,
            ruleId: rule.ruleId,
        });
    }
}
