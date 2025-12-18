import { DOCUMENT, NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, computed, contentChildren, inject, input, linkedSignal } from '@angular/core';

export type SplitPaneTab = {
    id: string;
    label: string;
};

@Directive({
    selector: 'ng-template[cqSplitPaneTab]',
    standalone: true,
})
export class SplitPaneTabTemplateDirective {
    private readonly templateRef = inject<TemplateRef<unknown>>(TemplateRef);

    readonly tabId = input.required<string>({ alias: 'cqSplitPaneTab' });

    template(): TemplateRef<unknown> {
        return this.templateRef;
    }
}

@Component({
    selector: 'cq-split-pane',
    standalone: true,
    imports: [NgTemplateOutlet],
    templateUrl: './split-pane.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SplitPane {
    readonly tabs = input.required<ReadonlyArray<SplitPaneTab>>();
    readonly tabListLabel = input('Detail tabs');
    readonly defaultTabId = input<string | null>(null);

    readonly activeTabId = linkedSignal({
        source: () => ({ tabs: this.tabs(), defaultTabId: this.defaultTabId() }),
        computation: ({ tabs, defaultTabId }, previous) => {
            if (tabs.length === 0) {
                return '';
            }

            const current = previous?.value ?? '';
            const hasCurrent = current ? tabs.some((tab) => tab.id === current) : false;
            if (hasCurrent) {
                return current;
            }

            if (defaultTabId && tabs.some((tab) => tab.id === defaultTabId)) {
                return defaultTabId;
            }

            return tabs[0].id;
        },
    });

    private readonly tabTemplates = contentChildren(SplitPaneTabTemplateDirective, { descendants: true });
    private readonly document = inject(DOCUMENT);

    readonly templatesById = computed(() => {
        const map = new Map<string, TemplateRef<unknown>>();
        for (const entry of this.tabTemplates()) {
            map.set(entry.tabId(), entry.template());
        }
        return map;
    });

    readonly activeTabIndex = computed(() => {
        const tabs = this.tabs();
        const active = this.activeTabId();
        const idx = tabs.findIndex((tab) => tab.id === active);
        return idx >= 0 ? idx : 0;
    });

    readonly activeTemplate = computed(() => {
        const tabId = this.tabs()[this.activeTabIndex()]?.id;
        if (!tabId) {
            return null;
        }
        return this.templatesById().get(tabId) ?? null;
    });

    selectTab(id: string): void {
        const exists = this.tabs().some((tab) => tab.id === id);
        if (!exists) {
            return;
        }

        this.activeTabId.set(id);
    }

    tabButtonId(tabId: string): string {
        return `cq-split-pane-tab-${tabId}`;
    }

    tabPanelId(tabId: string): string {
        return `cq-split-pane-panel-${tabId}`;
    }

    onTabKeydown(event: KeyboardEvent, tabIndex: number): void {
        const tabs = this.tabs();
        if (tabs.length === 0) {
            return;
        }

        let nextIndex: number | null = null;
        switch (event.key) {
            case 'ArrowRight':
            case 'ArrowDown':
                nextIndex = (tabIndex + 1) % tabs.length;
                break;
            case 'ArrowLeft':
            case 'ArrowUp':
                nextIndex = (tabIndex - 1 + tabs.length) % tabs.length;
                break;
            case 'Home':
                nextIndex = 0;
                break;
            case 'End':
                nextIndex = tabs.length - 1;
                break;
            default:
                break;
        }

        if (nextIndex === null) {
            return;
        }

        event.preventDefault();
        const nextTab = tabs[nextIndex];
        this.selectTab(nextTab.id);

        const button = this.document.getElementById(this.tabButtonId(nextTab.id));
        if (button instanceof HTMLElement) {
            button.focus();
        }
    }
}
