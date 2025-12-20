import { A11yModule } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, computed, effect, input, output, signal } from '@angular/core';

export type DetailSurfaceMode = 'drawer' | 'dialog';

let nextDetailSurfaceId = 0;

@Component({
    selector: 'cq-detail-surface',
    imports: [A11yModule],
    template: `
    @if (open()) {
      <section
        class="fixed inset-0 z-50 flex"
        role="dialog"
        aria-modal="true"
        [attr.aria-labelledby]="titleId()"
        (keydown)="handleKey($event)"
      >
        <div
          class="absolute inset-0 bg-slate-950/80 backdrop-blur-xl"
          aria-hidden="true"
          (click)="onBackdropClick()"
        ></div>

        <div class="relative z-10 flex w-full" [class.items-center]="mode() === 'dialog'">
          <div
            role="document"
            cdkTrapFocus
            cdkTrapFocusAutoCapture
            class="w-full"
            [class.max-w-2xl]="mode() === 'dialog'"
            [class.mx-auto]="mode() === 'dialog'"
            [class.my-10]="mode() === 'dialog'"
            [class.h-full]="mode() === 'drawer'"
            [class.max-w-xl]="mode() === 'drawer'"
            [class.ml-auto]="mode() === 'drawer'"
          >
            <div
              class="flex h-full flex-col rounded-2xl border border-white/10 bg-surface-alt/95 shadow-2xl ring-1 ring-black/50"
              [class.rounded-l-2xl]="mode() === 'drawer'"
              [class.rounded-r-none]="mode() === 'drawer'"
            >
              <header class="flex items-start justify-between gap-3 border-b border-white/10 px-5 py-4">
                <div class="space-y-1">
                  <h2 class="text-sm font-semibold text-text" [id]="titleId()">{{ title() }}</h2>
                  @if (subtitle()) {
                    <p class="text-xs text-text-muted">{{ subtitle() }}</p>
                  }
                </div>

                <div class="flex items-center gap-2">
                  <ng-content select="[cqDetailSurfaceHeader]"></ng-content>
                  <button
                    type="button"
                    class="rounded-md border border-white/10 px-3 py-1 text-xs font-semibold text-text"
                    (click)="requestClose()"
                    [attr.aria-label]="closeAriaLabel()"
                  >
                    Close
                  </button>
                </div>
              </header>

              <div class="flex-1 overflow-auto px-5 py-4">
                <ng-content></ng-content>
              </div>

              <footer class="border-t border-white/10 px-5 py-4">
                <ng-content select="[cqDetailSurfaceFooter]"></ng-content>
              </footer>
            </div>
          </div>
        </div>
      </section>
    }
  `,
    changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DetailSurface {
    readonly open = input(false);
    readonly title = input('');
    readonly subtitle = input<string | null>(null);
    readonly mode = input<DetailSurfaceMode>('drawer');
    readonly closeOnBackdrop = input(true);
    readonly closeAriaLabel = input('Close dialog');

    readonly closed = output<void>();

    private readonly titleIdSignal = signal(`cq-detail-surface-title-${++nextDetailSurfaceId}`);
    readonly titleId = computed(() => this.titleIdSignal());

    private readonly previouslyOpen = signal(false);
    private openerElement: HTMLElement | null = null;

    private readonly rememberAndRestoreFocus = effect(() => {
        const isOpen = this.open();
        const wasOpen = this.previouslyOpen();

        if (isOpen && !wasOpen) {
            const active = document.activeElement;
            this.openerElement = active instanceof HTMLElement ? active : null;
        }

        if (!isOpen && wasOpen) {
            const opener = this.openerElement;
            this.openerElement = null;
            if (opener) {
                queueMicrotask(() => opener.focus());
            }
        }

        this.previouslyOpen.set(isOpen);
    });

    requestClose(): void {
        this.closed.emit();
    }

    onBackdropClick(): void {
        if (!this.closeOnBackdrop()) {
            return;
        }
        this.requestClose();
    }

    handleKey(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            this.requestClose();
        }
    }
}
