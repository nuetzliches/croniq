import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
    selector: 'cq-app-brand',
    template: `Croniq`,
    host: {
        class: 'text-lg font-bold uppercase tracking-[0.35em] text-primary',
    },
    changeDetection: ChangeDetectionStrategy.OnPush
})
export class AppBrand { }
