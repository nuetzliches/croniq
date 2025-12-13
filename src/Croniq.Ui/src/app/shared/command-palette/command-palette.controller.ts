import { Injectable, Provider, computed, inject, signal, InjectionToken } from '@angular/core';
import { Router } from '@angular/router';

type CommandKeyword = string;

export type CommandPaletteCommand = {
    id: string;
    label: string;
    path: string;
    description?: string;
    keywords?: ReadonlyArray<CommandKeyword>;
};

export const COMMAND_PALETTE_COMMANDS = new InjectionToken<ReadonlyArray<CommandPaletteCommand>>(
    'COMMAND_PALETTE_COMMANDS'
);

export function provideCommandPaletteCommands(commands: ReadonlyArray<CommandPaletteCommand>): Provider {
    return {
        provide: COMMAND_PALETTE_COMMANDS,
        useValue: commands,
        multi: true,
    };
}

const DEFAULT_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    { id: 'dashboard', label: 'Dashboard', path: 'dashboard', description: 'Queue depth, misfires, hooks' },
    { id: 'schedules', label: 'Schedules', path: 'schedules', description: 'Cron + policy inventory' },
    { id: 'jobs', label: 'Jobs', path: 'jobs', description: 'Registry browser & triggers' },
    { id: 'webhooks', label: 'Webhooks', path: 'webhooks', description: 'Ingress status & secrets' },
    { id: 'tenants', label: 'Tenants & Keys', path: 'tenants', description: 'Quota + key rotation' },
];

@Injectable({ providedIn: 'root' })
export class CommandPaletteController {
    private readonly router = inject(Router);
    private readonly providedCommandSets = inject(COMMAND_PALETTE_COMMANDS, { optional: true }) ?? [];

    readonly isOpen = signal(false);
    readonly query = signal('');
    readonly activeIndex = signal(0);
    readonly commands = signal<ReadonlyArray<CommandPaletteCommand>>(DEFAULT_COMMANDS);

    constructor() {
        const flattened = this.providedCommandSets.flat();
        if (flattened.length) {
            this.commands.set(flattened);
        }
    }

    readonly filteredCommands = computed(() => {
        const q = this.query().trim().toLowerCase();
        if (!q) {
            return this.commands();
        }

        return this.commands().filter((command) => {
            const haystack = [command.label, command.path, command.description ?? '', ...(command.keywords ?? [])]
                .join(' ')
                .toLowerCase();
            return haystack.includes(q);
        });
    });

    readonly liveRegionMessage = computed(() => {
        const count = this.filteredCommands().length;
        if (count === 0) {
            return 'No commands available.';
        }
        if (count === 1) {
            return '1 command available.';
        }
        return `${count} commands available.`;
    });

    registerCommands(commands: ReadonlyArray<CommandPaletteCommand>): void {
        if (commands.length === 0) {
            return;
        }
        this.commands.set(commands);
        this.resetNavigation();
    }

    open(): void {
        if (this.isOpen()) {
            return;
        }
        this.isOpen.set(true);
        this.resetNavigation();
    }

    close(): void {
        if (!this.isOpen()) {
            return;
        }
        this.isOpen.set(false);
        this.query.set('');
        this.resetNavigation();
    }

    updateQuery(value: string): void {
        this.query.set(value);
        this.resetNavigation();
    }

    moveSelection(delta: number): void {
        const results = this.filteredCommands();
        if (results.length === 0) {
            this.activeIndex.set(-1);
            return;
        }
        const length = results.length;
        const next = (this.activeIndex() + delta + length) % length;
        this.activeIndex.set(next);
    }

    async executeCommand(index = this.activeIndex()): Promise<void> {
        const command = this.filteredCommands()[index];
        if (!command) {
            return;
        }
        await this.router.navigate(['/', command.path]);
        this.close();
    }

    optionId(command: CommandPaletteCommand, index: number): string {
        return `command-option-${command.id}-${index}`;
    }

    handleKey(event: KeyboardEvent): boolean {
        if (event.key === 'Escape') {
            this.close();
            return true;
        }
        if (event.key === 'ArrowDown') {
            this.moveSelection(1);
            return true;
        }
        if (event.key === 'ArrowUp') {
            this.moveSelection(-1);
            return true;
        }
        if (event.key === 'Enter') {
            void this.executeCommand();
            return true;
        }
        return false;
    }

    private resetNavigation(): void {
        this.activeIndex.set(0);
    }
}
