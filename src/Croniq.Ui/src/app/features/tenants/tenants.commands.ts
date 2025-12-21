import { CommandPaletteCommand, provideCommandPaletteCommands } from '@shared/command-palette/command-palette.controller';

const TENANTS_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'tenants-add',
        label: 'API keys · Add client',
        path: 'tenants',
        description: 'Launch the onboarding flow',
        keywords: ['client', 'add', 'onboard'],
    },
    {
        id: 'tenants-rotate-keys',
        label: 'API keys · Rotate API keys',
        path: 'tenants',
        description: 'Jump to API key rotation panel',
        keywords: ['keys', 'rotation', 'security'],
    },
];

export const TENANTS_COMMANDS_PROVIDER = provideCommandPaletteCommands(TENANTS_COMMANDS);
