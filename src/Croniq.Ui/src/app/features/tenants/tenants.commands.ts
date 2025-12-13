import { CommandPaletteCommand, provideCommandPaletteCommands } from '../../shared/command-palette/command-palette.controller';

const TENANTS_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'tenants-add',
        label: 'Tenants · Add tenant',
        path: 'tenants',
        description: 'Launch the tenant onboarding flow',
        keywords: ['tenant', 'add', 'onboard'],
    },
    {
        id: 'tenants-rotate-keys',
        label: 'Tenants · Rotate API keys',
        path: 'tenants',
        description: 'Jump to API key rotation panel',
        keywords: ['keys', 'rotation', 'security'],
    },
];

export const TENANTS_COMMANDS_PROVIDER = provideCommandPaletteCommands(TENANTS_COMMANDS);
