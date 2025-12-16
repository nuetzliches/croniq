import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { RouterTestingModule } from '@angular/router/testing';

import {
    CommandPaletteController,
    provideCommandPaletteCommands,
} from './command-palette.controller';

describe('CommandPaletteController', () => {
    let controller: CommandPaletteController;
    let router: Router;

    beforeEach(() => {
        TestBed.configureTestingModule({
            imports: [RouterTestingModule],
            providers: [
                provideZonelessChangeDetection(),
                provideCommandPaletteCommands([
                    { id: 'alpha', label: 'Alpha', path: 'dashboard', description: 'Main overview' },
                    { id: 'beta', label: 'Beta', path: 'schedules', description: 'All schedules' },
                ]),
            ],
        });

        controller = TestBed.inject(CommandPaletteController);
        router = TestBed.inject(Router);
        vi.spyOn(router, 'navigate').mockResolvedValue(true);
    });

    it('exposes commands provided via injection token', () => {
        expect(controller.filteredCommands().map((command) => command.id)).toEqual(['alpha', 'beta']);
    });

    it('filters commands based on query text and announces counts', () => {
        controller.updateQuery('bet');
        expect(controller.filteredCommands().map((command) => command.id)).toEqual(['beta']);
        expect(controller.liveRegionMessage()).toBe('1 command available.');

        controller.updateQuery('missing');
        expect(controller.filteredCommands().length).toBe(0);
        expect(controller.liveRegionMessage()).toBe('No commands available.');
    });

    it('moves selection via keyboard helpers', () => {
        controller.open();
        controller.moveSelection(1);
        expect(controller.activeIndex()).toBe(1);

        const handled = controller.handleKey(new KeyboardEvent('keydown', { key: 'ArrowUp' }));
        expect(handled).toBe(true);
        expect(controller.activeIndex()).toBe(0);
    });

    it('navigates to the selected command and closes', async () => {
        controller.open();
        await controller.executeCommand(1);

        expect(router.navigate).toHaveBeenCalledWith(['/', 'schedules']);
        expect(controller.isOpen()).toBe(false);
    });
});
