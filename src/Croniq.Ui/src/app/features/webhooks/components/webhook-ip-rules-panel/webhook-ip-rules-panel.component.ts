import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { Field, form } from '@angular/forms/signals';
import { WebhookEndpointView, WebhookIpRuleView } from '@features/webhooks/webhooks.store';
import { CqFormFieldComponent, CqInputDirective, CqTextareaDirective } from 'ui-kit';

type IpRuleFormModel = {
    cidr: string;
    description: string;
};

@Component({
    selector: 'cq-webhook-ip-rules-panel',
    imports: [Field, CqFormFieldComponent, CqInputDirective, CqTextareaDirective],
    templateUrl: './webhook-ip-rules-panel.component.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WebhookIpRulesPanelComponent {
    readonly endpoint = input<WebhookEndpointView | null>(null);
    readonly rules = input<ReadonlyArray<WebhookIpRuleView>>([]);
    readonly busy = input(false);

    readonly createRule = output<{ cidr: string; description: string | null }>();
    readonly deleteRule = output<WebhookIpRuleView>();
    readonly closePanel = output<void>();

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

    readonly canSubmit = computed(() => Boolean(this.endpoint()) && !this.cidrError() && !this.busy());

    submit(): void {
        if (!this.canSubmit()) {
            return;
        }
        const cidr = this.formModel().cidr.trim();
        if (!cidr) {
            return;
        }
        const description = this.formModel().description.trim();
        this.createRule.emit({
            cidr,
            description: description ? description : null,
        });
        this.formModel.set({
            cidr: '',
            description: '',
        });
    }

    removeRule(rule: WebhookIpRuleView): void {
        this.deleteRule.emit(rule);
    }
}
