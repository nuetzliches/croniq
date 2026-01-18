import { ChangeDetectionStrategy, Component, TemplateRef, input, output } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';

@Component({
    selector: 'cq-panel-shell',
    imports: [NgTemplateOutlet],
    templateUrl: './panel-shell.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CqPanelShellComponent {
    readonly panelTemplate = input<TemplateRef<unknown> | null>(null);
    readonly title = input<string>('Filters & settings');
    readonly subtitle = input<string>('');
    readonly open = input<boolean>(true);
    readonly toggle = output<void>();

    handleToggle(): void {
        this.toggle.emit();
    }
}
