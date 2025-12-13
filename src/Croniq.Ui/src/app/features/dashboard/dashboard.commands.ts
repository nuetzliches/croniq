import { CommandPaletteCommand, provideCommandPaletteCommands } from '../../shared/command-palette/command-palette.controller';

const DASHBOARD_COMMANDS: ReadonlyArray<CommandPaletteCommand> = [
    {
        id: 'dashboard-latency',
        label: 'Dashboard · Inspect latency chart',
        path: 'dashboard',
        description: 'Focus the latency widgets for quick review',
        keywords: ['metrics', 'latency', 'charts'],
    },
    {
        id: 'dashboard-queue-depth',
        label: 'Dashboard · Queue depth heatmap',
        path: 'dashboard',
        description: 'Jump to queue depth panels in the dashboard',
        keywords: ['queue', 'depth', 'heatmap'],
    },
];

export const DASHBOARD_COMMANDS_PROVIDER = provideCommandPaletteCommands(DASHBOARD_COMMANDS);
