import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { PasswordAuthService } from '@core/auth/password-auth.service';
import { NAV_SECTIONS, NavSection } from '@core/navigation/nav-items';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { AppBrand } from '@shared/app-brand/app-brand';
import { CommandPalette } from '@shared/command-palette/command-palette';
import { CommandPaletteController } from '@shared/command-palette/command-palette.controller';
import { StatusBeacon } from '@shared/status-beacon/status-beacon';
import { ShellPanelService } from '@shell/panel/shell-panel.service';
import { CqPanelShellComponent } from 'ui-kit';
import { finalize } from 'rxjs';

type StatusIntent = 'success' | 'warn' | 'neutral';

type StatusCard = {
  label: string;
  value: string;
  intent: StatusIntent;
};

@Component({
  selector: 'cq-shell',
  imports: [RouterLink, RouterLinkActive, RouterOutlet, AppBrand, CommandPalette, StatusBeacon, CqPanelShellComponent],
  templateUrl: './shell.html',
  host: {
    '(window:keydown)': 'handleGlobalPaletteShortcut($event)',
  },
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Shell {
  private readonly commandPalette = inject(CommandPaletteController);
  private readonly tenantContext = inject(TenantContextService);
  private readonly runtimeConfig = inject(RuntimeConfigService);
  private readonly passwordAuth = inject(PasswordAuthService);
  private readonly router = inject(Router);
  private readonly panel = inject(ShellPanelService);

  readonly tenantDisplay = this.tenantContext.tenantLabel;
  readonly environmentDisplay = this.tenantContext.environment;
  readonly showTenantBadge = computed(() => this.runtimeConfig.defaultTenantId.trim().length === 0);
  readonly hasEnvironment = computed(() => (this.tenantContext.environment() ?? '').trim().length > 0);
  readonly navSections = signal<ReadonlyArray<NavSection>>(NAV_SECTIONS);

  readonly statusCards = signal<ReadonlyArray<StatusCard>>([
    { label: 'Cluster', value: 'Healthy', intent: 'success' },
    { label: 'Queue Depth', value: '42', intent: 'warn' },
    { label: 'Clock Δ', value: '+120 ms', intent: 'neutral' },
  ]);
  readonly commandPaletteOpen = this.commandPalette.isOpen;
  readonly panelTemplate = this.panel.panelTemplate;
  readonly panelTitle = this.panel.title;
  readonly panelSubtitle = this.panel.subtitle;
  readonly panelOpen = this.panel.isOpen;
  readonly panelCollapsedTemplate = this.panel.collapsedTemplate;

  openCommandPalette(): void {
    this.commandPalette.open();
  }


  closeCommandPalette(): void {
    this.commandPalette.close();
  }

  logout(): void {
    this.passwordAuth
      .logout()
      .pipe(finalize(() => void this.router.navigate(['/auth', 'login'])))
      .subscribe();
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

  togglePanel(): void {
    this.panel.toggle();
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
