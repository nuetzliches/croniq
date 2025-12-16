import { ChangeDetectionStrategy, Component, input } from '@angular/core';

export type StatusIntent = 'success' | 'warn' | 'neutral';

@Component({
  selector: 'cq-status-beacon',
  imports: [],
  templateUrl: './status-beacon.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class:
      'inline-flex min-w-[170px] items-center gap-3 rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm backdrop-blur',
  },
})
export class StatusBeacon {
  readonly label = input.required<string>();
  readonly value = input.required<string>();
  readonly intent = input<StatusIntent>('neutral');
}
