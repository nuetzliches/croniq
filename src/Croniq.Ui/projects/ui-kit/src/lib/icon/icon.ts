import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { DomSanitizer, type SafeHtml } from '@angular/platform-browser';
import { MDI_ICONS, type MdiIconName } from './mdi-icons';

@Component({
  selector: 'cq-icon',
  template: `
    @if (icon(); as resolved) {
      <svg
        [attr.viewBox]="resolved.viewBox"
        [attr.width]="size()"
        [attr.height]="size()"
        [attr.role]="ariaRole()"
        [attr.aria-label]="resolvedLabel()"
        [attr.aria-hidden]="ariaHidden()"
        [innerHTML]="svgBody()"
        focusable="false"
        class="block"
      >
      </svg>
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'inline-flex leading-none',
  },
})
export class CqIconComponent {
  readonly name = input.required<MdiIconName>();
  readonly size = input<string | number>('1em');
  readonly ariaLabel = input<string | null>(null);

  private readonly sanitizer = inject(DomSanitizer);

  readonly icon = computed(() => MDI_ICONS[this.name()] ?? null);
  readonly svgBody = computed<SafeHtml | null>(() => {
    const resolved = this.icon();
    if (!resolved) {
      return null;
    }
    return this.sanitizer.bypassSecurityTrustHtml(resolved.body);
  });
  readonly resolvedLabel = computed(() => {
    const value = this.ariaLabel();
    const trimmed = typeof value === 'string' ? value.trim() : '';
    return trimmed ? trimmed : null;
  });
  readonly ariaRole = computed(() => (this.resolvedLabel() ? 'img' : null));
  readonly ariaHidden = computed(() => (this.resolvedLabel() ? null : 'true'));
}

export type { MdiIconName };
