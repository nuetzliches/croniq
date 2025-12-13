import { CommandPaletteCommand, provideCommandPaletteCommands } from '../../shared/command-palette/command-palette.controller';

const WEBHOOKS_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'webhooks-create',
        label: 'Webhooks · Add endpoint',
        path: 'webhooks',
        description: 'Start the guided webhook creation flow',
        keywords: ['webhook', 'create', 'endpoint'],
    },
    {
        id: 'webhooks-failures',
        label: 'Webhooks · Investigate failures',
        path: 'webhooks',
        description: 'Jump to webhooks filtered by delivery errors',
        keywords: ['failures', 'delivery', 'errors'],
    },
];

export const WEBHOOKS_COMMANDS_PROVIDER = provideCommandPaletteCommands(WEBHOOKS_COMMANDS);
