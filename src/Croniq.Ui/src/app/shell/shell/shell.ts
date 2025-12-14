import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, HostListener, inject, signal } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';

import { CommandPalette } from '../../shared/command-palette/command-palette';
import { CommandPaletteController } from '../../shared/command-palette/command-palette.controller';
import { StatusBeacon } from '../../shared/status-beacon/status-beacon';
import { NavItem, PRIMARY_NAV_ITEMS } from '../../core/navigation/nav-items';
import { TenantContextService } from '../../core/tenant-context/tenant-context.service';

type StatusIntent = 'success' | 'warn' | 'neutral';

type StatusCard = {
  label: string;
  value: string;
  intent: StatusIntent;
};

@Component({
  selector: 'app-shell',
  imports: [CommonModule, RouterLink, RouterLinkActive, RouterOutlet, CommandPalette, StatusBeacon],
  templateUrl: './shell.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Shell {
  private readonly commandPalette = inject(CommandPaletteController);
  private readonly tenantContext = inject(TenantContextService);

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

  @HostListener('window:keydown', ['$event'])
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
