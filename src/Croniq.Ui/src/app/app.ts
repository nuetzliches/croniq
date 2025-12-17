import { ChangeDetectionStrategy, Component } from '@angular/core';import { Shell } from './shell/shell/shell';

@Component({
  selector: 'cq-root',
  imports: [Shell],
  templateUrl: './app.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
}
