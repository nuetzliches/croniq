import { CommonModule } from '@angular/common';
import { Component, ViewChild, signal } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';

import { CommandPalette } from '../../shared/command-palette/command-palette';
import { StatusBeacon } from '../../shared/status-beacon/status-beacon';

type NavItem = {
  path: string;
  label: string;
  description: string;
};

@Component({
  selector: 'app-shell',
  standalone: true,
  imports: [CommonModule, RouterLink, RouterLinkActive, RouterOutlet, CommandPalette, StatusBeacon],
  templateUrl: './shell.html',
  styleUrl: './shell.css',
})
export class Shell {
  readonly tenant = signal('Tenant Alpha');
  readonly environment = signal('dev');
  readonly navItems: NavItem[] = [
    { path: '/dashboard', label: 'Dashboard', description: 'Queue depth, misfires, hooks' },
    { path: '/schedules', label: 'Schedules', description: 'Cron + policy inventory' },
    { path: '/jobs', label: 'Jobs', description: 'Registry browser & triggers' },
    { path: '/webhooks', label: 'Webhooks', description: 'Ingress status & secrets' },
    { path: '/tenants', label: 'Tenants & Keys', description: 'Quota + key rotation' },
  ];

  readonly statusCards = [
    { label: 'Cluster', value: 'Healthy', intent: 'success' as const },
    { label: 'Queue Depth', value: '42', intent: 'warn' as const },
    { label: 'Clock Δ', value: '+120 ms', intent: 'neutral' as const },
  ];

  @ViewChild(CommandPalette) private commandPalette?: CommandPalette;

  openCommandPalette(): void {
    this.commandPalette?.open();
  }
}
