import { Directive } from '@angular/core';

@Directive({
  selector: 'input[type=checkbox][cqToggle]',
  host: {
    class:
      'h-4 w-4 rounded border-white/10 bg-surface-alt text-accent focus:ring-accent',
  },
})
export class CqToggleDirective {}
