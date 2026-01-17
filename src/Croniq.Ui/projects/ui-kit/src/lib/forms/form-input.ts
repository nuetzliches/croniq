import { Directive } from '@angular/core';

@Directive({
  selector: 'input[cqInput]',
  host: {
    class:
      'w-full rounded-lg border border-white/10 bg-surface-alt px-3 py-2 text-sm text-white placeholder-muted focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent',
  },
})
export class CqInputDirective {}
