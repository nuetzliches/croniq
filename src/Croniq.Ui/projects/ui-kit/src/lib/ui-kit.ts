import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'cq-ui-kit',
  imports: [],
  template: `
    <p>
      ui-kit works!
    </p>
  `,
  styles: ``,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class UiKit {

}
