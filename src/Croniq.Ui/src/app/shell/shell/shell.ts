import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { NavItem, PRIMARY_NAV_ITEMS } from '@core/navigation/nav-items';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { AppBrand } from '@shared/app-brand/app-brand';
import { CommandPalette } from '@shared/command-palette/command-palette';
import { CommandPaletteController } from '@shared/command-palette/command-palette.controller';
import { StatusBeacon } from '@shared/status-beacon/status-beacon';

type StatusIntent = 'success' | 'warn' | 'neutral';

type StatusCard = {
  label: string;
  value: string;
  intent: StatusIntent;
};

@Component({
  selector: 'cq-shell',
  imports: [RouterLink, RouterLinkActive, RouterOutlet, AppBrand, CommandPalette, StatusBeacon],
  templateUrl: './shell.html',
  host: {
    '(window:keydown)': 'handleGlobalPaletteShortcut($event)',
  },
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Shell {
  private readonly commandPalette = inject(CommandPaletteController);
  private readonly tenantContext = inject(TenantContextService);
  private readonly passwordAuth = inject(PasswordAuthService);
  private readonly router = inject(Router);

  readonly tenantDisplay = this.tenantContext.tenantLabel;
  readonly navItems = signal<ReadonlyArray<NavItem>>(PRIMARY_NAV_ITEMS);

  readonly statusCards = signal<ReadonlyArray<StatusCard>>([
    { label: 'Cluster', value: 'Healthy', intent: 'success' },
    { label: 'Queue Depth', value: '42', intent: 'warn' },
    { label: 'Clock Δ', value: '+120 ms', intent: 'neutral' },
  ]);
  readonly commandPaletteOpen = this.commandPalette.isOpen;

  openCommandPalette(): void {
    this.commandPalette.open();
  }

  closeCommandPalette(): void {
    this.commandPalette.close();
  }

  async logout(): Promise<void> {
    await this.passwordAuth.logout();
    await this.router.navigate(['/login']);
  }

  handleGlobalPaletteShortcut(event: KeyboardEvent): void {
    if (!isPaletteShortcut(event) || isEditableTarget(event.target)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (this.commandPaletteOpen()) {
      this.closeCommandPalette();
    } else {
      this.openCommandPalette();
    }
  }
}

const EDITABLE_TAGS = new Set(['INPUT', 'TEXTAREA', 'SELECT']);

function isPaletteShortcut(event: KeyboardEvent): boolean {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
    return !event.repeat;
  }
  return false;
}

function isEditableTarget(target: EventTarget | null): target is HTMLElement {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return target.isContentEditable || EDITABLE_TAGS.has(target.tagName);
}
