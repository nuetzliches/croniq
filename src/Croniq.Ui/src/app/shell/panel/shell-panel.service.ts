import { Injectable, TemplateRef, effect, signal } from '@angular/core';

const PANEL_OPEN_STORAGE_KEY = 'croniq.shell.panel.open';

@Injectable({ providedIn: 'root' })
export class ShellPanelService {
    readonly panelTemplate = signal<TemplateRef<unknown> | null>(null);
    readonly collapsedTemplate = signal<TemplateRef<unknown> | null>(null);
    readonly title = signal('Search & filters');
    readonly subtitle = signal('');
    readonly isOpen = signal(readPanelOpen());

    constructor() {
        effect(() => {
            const open = this.isOpen();
            try {
                window.localStorage.setItem(PANEL_OPEN_STORAGE_KEY, open ? '1' : '0');
            } catch {
                // ignore storage errors
            }
        });
    }

    setPanel(
        template: TemplateRef<unknown>,
        title: string,
        subtitle?: string,
        collapsedTemplate?: TemplateRef<unknown> | null,
    ): void {
        this.panelTemplate.set(template);
        this.collapsedTemplate.set(collapsedTemplate ?? null);
        this.title.set(title);
        this.subtitle.set(subtitle ?? '');
    }

    clearPanel(template?: TemplateRef<unknown>): void {
        const current = this.panelTemplate();
        if (!template || current === template) {
            this.panelTemplate.set(null);
            this.collapsedTemplate.set(null);
        }
    }

    toggle(): void {
        this.isOpen.update((value) => !value);
    }
}

function readPanelOpen(): boolean {
    try {
        const stored = window.localStorage.getItem(PANEL_OPEN_STORAGE_KEY);
        return stored ? stored === '1' : true;
    } catch {
        return true;
    }
}
