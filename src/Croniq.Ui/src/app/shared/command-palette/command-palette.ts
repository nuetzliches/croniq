import { CommonModule } from '@angular/common';
import { Component, EventEmitter, Output, signal } from '@angular/core';
import { Router } from '@angular/router';

interface CommandItem {
  label: string;
  path: string;
}

@Component({
  selector: 'app-command-palette',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './command-palette.html',
  styleUrl: './command-palette.css',
})
export class CommandPalette {
  @Output() closed = new EventEmitter<void>();

  readonly isOpen = signal(false);
  readonly commands: CommandItem[] = [
    { label: 'Dashboard', path: '/dashboard' },
    { label: 'Schedules', path: '/schedules' },
    { label: 'Jobs', path: '/jobs' },
    { label: 'Webhooks', path: '/webhooks' },
    { label: 'Tenants & Keys', path: '/tenants' },
  ];

  constructor(private readonly router: Router) { }

  open(): void {
    this.isOpen.set(true);
  }

  close(): void {
    this.isOpen.set(false);
    this.closed.emit();
  }

  async execute(command: CommandItem): Promise<void> {
    await this.router.navigateByUrl(command.path);
    this.close();
  }
}
