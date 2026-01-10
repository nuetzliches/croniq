import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { UiPreferencesService } from '@core/preferences/ui-preferences.service';

@Component({
  selector: 'cq-root',
  imports: [RouterOutlet],
  templateUrl: './app.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  private readonly _preferences = inject(UiPreferencesService);
}
