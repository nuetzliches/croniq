import { CommandPaletteCommand, provideCommandPaletteCommands } from '@shared/command-palette/command-palette.controller';

const CALENDARS_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'calendars-create',
        label: 'Calendars - Create calendar',
        path: 'calendars',
        description: 'Create a new schedule calendar',
        keywords: ['calendar', 'create', 'rules'],
    },
    {
        id: 'calendars-review-rules',
        label: 'Calendars - Review rules',
        path: 'calendars',
        description: 'Review calendar definitions and rule coverage',
        keywords: ['calendar', 'include', 'exclude'],
    },
];

export const CALENDARS_COMMANDS_PROVIDER = provideCommandPaletteCommands(CALENDARS_COMMANDS);
