import { CommandPaletteCommand, provideCommandPaletteCommands } from '../../shared/command-palette/command-palette.controller';

const SCHEDULES_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'schedules-create',
        label: 'Schedules · New schedule',
        path: 'schedules',
        description: 'Create a new cron workflow',
        keywords: ['create', 'cron', 'policy'],
    },
    {
        id: 'schedules-review-failures',
        label: 'Schedules · Review failures',
        path: 'schedules',
        description: 'Open schedules filtered by recent failures',
        keywords: ['errors', 'failures'],
    },
];

export const SCHEDULES_COMMANDS_PROVIDER = provideCommandPaletteCommands(SCHEDULES_COMMANDS);
