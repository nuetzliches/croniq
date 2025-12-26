import { CommandPaletteCommand, provideCommandPaletteCommands } from '@shared/command-palette/command-palette.controller';

export type NavItem = {
    path: string;
    label: string;
    description: string;
};

export type NavSection = {
    label: string;
    items: NavItem[];
};

export const NAV_SECTIONS: ReadonlyArray<NavSection> = [
    {
        label: 'CORE',
        items: [
            { path: 'dashboard', label: 'Dashboard', description: 'Overview' },
            { path: 'jobs', label: 'Jobs', description: 'Registry' },
            { path: 'schedules', label: 'Schedules', description: 'Triggers' },
            { path: 'executions', label: 'Executions', description: 'History' },
        ],
    },
    {
        label: 'INFRA',
        items: [
            { path: 'runners', label: 'Runners', description: 'Workers' },
            { path: 'webhooks', label: 'Webhooks', description: 'Ingress' },
            { path: 'api-access', label: 'API Access', description: 'Clients & Keys' },
        ],
    },
    {
        label: 'SYS',
        items: [{ path: 'settings', label: 'Settings', description: 'Tenant config' }],
    },
];

export const PRIMARY_NAV_COMMANDS: ReadonlyArray<CommandPaletteCommand> = NAV_SECTIONS.flatMap(
    (section) =>
        section.items.map((item) => ({
            id: item.path,
            label: item.label,
            path: item.path,
            description: item.description,
            category: section.label,
        })),
);

export const PRIMARY_NAV_COMMANDS_PROVIDER = provideCommandPaletteCommands(PRIMARY_NAV_COMMANDS);


