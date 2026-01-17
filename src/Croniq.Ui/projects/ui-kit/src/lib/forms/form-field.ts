import { ChangeDetectionStrategy, Component, input } from '@angular/core';

@Component({
  selector: 'cq-form-field',
  imports: [],
  template: `
    <div class="flex flex-col gap-1">
      @if (label()) {
        <label class="text-xs font-medium text-muted" [attr.for]="forId() ?? null">
          {{ label() }}
        </label>
      }
      <ng-content />
      @if (hint()) {
        <p class="text-xs text-muted">{{ hint() }}</p>
      }
      @if (error()) {
        <p class="text-xs text-danger">{{ error() }}</p>
      }
    </div>
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CqFormFieldComponent {
  readonly label = input<string | null>(null);
  readonly hint = input<string | null>(null);
  readonly error = input<string | null>(null);
  readonly forId = input<string | null>(null);
}
