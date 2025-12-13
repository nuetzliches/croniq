import { CommandPaletteCommand, provideCommandPaletteCommands } from '../../shared/command-palette/command-palette.controller';

type NavItem = {
    path: string;
    label: string;
    description: string;
};

export const PRIMARY_NAV_ITEMS: ReadonlyArray<NavItem> = [
    { path: 'dashboard', label: 'Dashboard', description: 'Queue depth, misfires, hooks' },
    { path: 'schedules', label: 'Schedules', description: 'Cron + policy inventory' },
    { path: 'jobs', label: 'Jobs', description: 'Registry browser & triggers' },
    { path: 'webhooks', label: 'Webhooks', description: 'Ingress status & secrets' },
    { path: 'tenants', label: 'Tenants & Keys', description: 'Quota + key rotation' },
];

export const PRIMARY_NAV_COMMANDS: ReadonlyArray<CommandPaletteCommand> = PRIMARY_NAV_ITEMS.map(
    (item) => ({
        id: item.path,
        label: item.label,
        path: item.path,
        description: item.description,
    })
);

export const PRIMARY_NAV_COMMANDS_PROVIDER = provideCommandPaletteCommands(PRIMARY_NAV_COMMANDS);

export type { NavItem };
