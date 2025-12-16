import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'cq-telemetry',
  imports: [],
  template: `
    <p>
      telemetry works!
    </p>
  `,
  styles: ``,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Telemetry {

}
