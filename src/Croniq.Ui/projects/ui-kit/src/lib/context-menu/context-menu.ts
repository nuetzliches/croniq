import { CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, Directive, TemplateRef, input } from '@angular/core';

@Directive({
  selector: '[cqContextMenuItem]',
  hostDirectives: [
    {
      directive: CdkMenuItem,
      inputs: ['cdkMenuItemDisabled:disabled'],
    },
  ],
  host: {
    class:
      'flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-white/5 hover:text-white focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent',
    '[class.opacity-50]': 'disabled()',
    '[class.pointer-events-none]': 'disabled()',
    '[attr.aria-disabled]': 'disabled() ? "true" : null',
    '[attr.disabled]': 'disabled() ? "" : null',
  },
})
export class CqContextMenuItemDirective {
  readonly disabled = input(false);
}

@Component({
  selector: 'cq-context-menu',
  imports: [CdkMenuTrigger],
  template: `
    <button
      type="button"
      class="rounded p-1 text-muted hover:bg-white/10 hover:text-white"
      [attr.aria-label]="ariaLabel()"
      [cdkMenuTriggerFor]="menu()"
      [cdkMenuTriggerData]="menuData()"
      [disabled]="disabled()"
    >
      <span class="sr-only">{{ ariaLabel() }}</span>
      <svg class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
        <path
          d="M10 4.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM10 11.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM10 18.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z"
        />
      </svg>
    </button>
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CqContextMenuComponent {
  readonly ariaLabel = input('Open actions menu');
  readonly disabled = input(false);
  readonly menu = input.required<TemplateRef<unknown>>();
  readonly menuData = input<unknown | null>(null);
}
