import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'cq-data-access',
  imports: [],
  template: `
    <p>
      data-access works!
    </p>
  `,
  styles: ``,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DataAccess {

}
