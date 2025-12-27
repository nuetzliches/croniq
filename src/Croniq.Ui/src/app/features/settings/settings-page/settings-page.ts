import { ChangeDetectionStrategy, Component, signal } from '@angular/core';

@Component({
  selector: 'cq-settings-page',
  imports: [],
  templateUrl: './settings-page.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsPage {
  // Data
  tenant = signal({
    id: 'ten-123456789',
    name: 'Acme Corp',
    createdAt: new Date('2022-11-01'),
    plan: 'Enterprise',
    ownerEmail: 'admin@acme.com'
  });

  // Actions
  updateTenant() {
    console.log('Update tenant');
  }

  deactivateTenant() {
    console.log('Deactivate tenant');
  }
}
