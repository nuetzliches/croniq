import { CommandPaletteCommand, provideCommandPaletteCommands } from '@shared/command-palette/command-palette.controller';

const JOBS_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'jobs-register',
        label: 'Jobs · Register new job',
        path: 'jobs',
        description: 'Open the job registry to add a new handler',
        keywords: ['register', 'job', 'add'],
    },
    {
        id: 'jobs-retry-latest',
        label: 'Jobs · Retry last failure',
        path: 'jobs',
        description: 'Jump to failed jobs filtered by newest first',
        keywords: ['retry', 'failed'],
    },
];

export const JOBS_COMMANDS_PROVIDER = provideCommandPaletteCommands(JOBS_COMMANDS);
