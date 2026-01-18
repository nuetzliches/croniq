import { Dialog, type DialogConfig, type DialogRef } from '@angular/cdk/dialog';
import { type ComponentType } from '@angular/cdk/portal';
import { ChangeDetectionStrategy, Component, Directive, Injectable, computed, contentChild, inject, input } from '@angular/core';

type CqDialogSize = 'sm' | 'md' | 'lg' | 'xl' | 'full';

type CqDialogRole = 'dialog' | 'alertdialog';

export type CqDialogConfig<D = unknown, R = unknown, C = unknown> = DialogConfig<D, DialogRef<R, C>> & {
    size?: CqDialogSize;
};

@Directive({
    selector: '[cqDialogHeader]',
})
export class CqDialogHeaderDirective { }

@Directive({
    selector: '[cqDialogFooter]',
})
export class CqDialogFooterDirective { }

@Component({
    selector: 'cq-dialog',
    template: `
        <div
            [class]="containerClasses()"
      [attr.role]="role()"
      aria-modal="true"
      [attr.aria-label]="ariaLabel()"
      [attr.aria-labelledby]="ariaLabelledby()"
    [attr.aria-describedby]="ariaDescribedby() ?? bodyId"
    >
            @if (hasHeader()) {
                <header [class]="headerClasses()">
                    <ng-content select="[cqDialogHeader]"></ng-content>
                </header>
            }

            <div [class]="bodyClasses()" [id]="bodyId">
        <ng-content></ng-content>
      </div>

            @if (hasFooter()) {
                <footer [class]="footerClasses()">
                    <ng-content select="[cqDialogFooter]"></ng-content>
                </footer>
            }
    </div>
  `,
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CqDialogComponent {
    readonly size = input<CqDialogSize>('md');
    readonly containerClass = input<string>('');
    readonly headerClass = input<string>('');
    readonly bodyClass = input<string>('');
    readonly footerClass = input<string>('');
    readonly role = input<CqDialogRole>('dialog');
    readonly ariaLabel = input<string | null>(null);
    readonly ariaLabelledby = input<string | null>(null);
    readonly ariaDescribedby = input<string | null>(null);

    readonly bodyId = `cq-dialog-body-${createDialogId()}`;

    private readonly headerSlot = contentChild(CqDialogHeaderDirective);
    private readonly footerSlot = contentChild(CqDialogFooterDirective);

    readonly hasHeader = computed(() => !!this.headerSlot());
    readonly hasFooter = computed(() => !!this.footerSlot());

    readonly containerClasses = computed(() => {
        const sizeClass = getSizeClass(this.size());
        const customClass = this.containerClass().trim();
        const baseClass =
            'flex h-full w-full flex-col overflow-hidden rounded-xl border border-white/10 bg-surface shadow-2xl';
        return `${baseClass} ${sizeClass}${customClass ? ` ${customClass}` : ''}`.trim();
    });

    readonly bodyClasses = computed(() => {
        const baseClass = 'flex-1 overflow-y-auto p-6';
        const customClass = this.bodyClass().trim();
        return `${baseClass}${customClass ? ` ${customClass}` : ''}`.trim();
    });

    readonly headerClasses = computed(() => {
        const baseClass =
            'flex items-center justify-between border-b border-white/10 bg-surface-alt px-6 py-4';
        const customClass = this.headerClass().trim();
        return `${baseClass}${customClass ? ` ${customClass}` : ''}`.trim();
    });

    readonly footerClasses = computed(() => {
        const baseClass =
            'flex items-center justify-end gap-3 border-t border-white/10 bg-surface-alt px-6 py-4';
        const customClass = this.footerClass().trim();
        return `${baseClass}${customClass ? ` ${customClass}` : ''}`.trim();
    });
}

@Injectable({ providedIn: 'root' })
export class CqDialogService {
    private readonly dialog = inject(Dialog);

    open<R = unknown, D = unknown, C = unknown>(
        component: ComponentType<C>,
        config?: CqDialogConfig<D, R, C>,
    ): DialogRef<R, C> {
        const { size: _size, ...dialogConfig } = config ?? {};
        return this.dialog.open<R, D, C>(component, {
            hasBackdrop: true,
            autoFocus: 'first-tabbable',
            restoreFocus: true,
            closeOnNavigation: true,
            ...dialogConfig,
        });
    }
}

function getSizeClass(size: CqDialogSize): string {
    switch (size) {
        case 'sm':
            return 'w-[420px] max-w-[90vw]';
        case 'md':
            return 'w-[520px] max-w-[90vw]';
        case 'lg':
            return 'w-[720px] max-w-[90vw]';
        case 'xl':
            return 'w-[880px] max-w-[92vw]';
        case 'full':
            return 'w-[96vw] max-w-[96vw]';
        default:
            return 'w-[520px] max-w-[90vw]';
    }
}

function createDialogId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.round(Math.random() * 1000)}`;
}
