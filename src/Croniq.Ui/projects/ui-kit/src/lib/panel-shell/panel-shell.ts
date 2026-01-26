import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, TemplateRef, computed, input, output } from '@angular/core';
import { CqIconComponent, type MdiIconName } from '@ui-kit/icon/icon';

@Component({
    selector: 'cq-panel-shell',
    imports: [NgTemplateOutlet, CqIconComponent],
    templateUrl: './panel-shell.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CqPanelShellComponent {
    readonly panelTemplate = input<TemplateRef<unknown> | null>(null);
    readonly collapsedTemplate = input<TemplateRef<unknown> | null>(null);
    readonly title = input<string>('Search & filters');
    readonly subtitle = input<string>('');
    readonly open = input<boolean>(true);
    readonly toggleRequested = output<void>();
    readonly toggleIcon = computed<MdiIconName>(() => (this.open() ? 'chevron-left' : 'chevron-right'));

    handleToggle(): void {
        this.toggleRequested.emit();
    }
}
