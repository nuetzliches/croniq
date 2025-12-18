import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
    selector: 'cq-app-brand',
    templateUrl: './app-brand.html',
    changeDetection: ChangeDetectionStrategy.OnPush,
    host: {
        class: 'text-lg font-semibold uppercase tracking-[0.35em] text-text',
    },
})
export class AppBrand { }
