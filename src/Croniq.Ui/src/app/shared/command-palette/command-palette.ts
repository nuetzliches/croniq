import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, output, viewChild } from '@angular/core';

import { CommandPaletteCommand, CommandPaletteController } from './command-palette.controller';

@Component({
  selector: 'app-command-palette',
  imports: [CommonModule],
  templateUrl: './command-palette.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CommandPalette {
  readonly closed = output<void>();
  private readonly controller = inject(CommandPaletteController);
  private readonly searchField = viewChild<ElementRef<HTMLInputElement>>('commandPaletteInput');

  readonly isOpen = this.controller.isOpen;
  readonly query = this.controller.query;
  readonly results = this.controller.filteredCommands;
  readonly activeIndex = this.controller.activeIndex;
  readonly liveRegionMessage = this.controller.liveRegionMessage;
  readonly activeDescendantId = computed(() => {
    const commands = this.results();
    const index = this.activeIndex();
    if (index < 0 || index >= commands.length) {
      return null;
    }
    return this.controller.optionId(commands[index], index);
  });

  private readonly focusInput = effect(() => {
    if (!this.isOpen()) {
      return;
    }
    const input = this.searchField();
    if (!input) {
      return;
    }
    queueMicrotask(() => input.nativeElement.focus());
  });

  close(): void {
    this.controller.close();
    this.closed.emit();
  }

  onSearch(value: string): void {
    this.controller.updateQuery(value);
  }

  handleKey(event: KeyboardEvent): void {
    if (this.controller.handleKey(event)) {
      event.preventDefault();
      event.stopPropagation();
      if (!this.controller.isOpen()) {
        this.closed.emit();
      }
    }
  }

  async execute(command: CommandPaletteCommand, index: number): Promise<void> {
    await this.controller.executeCommand(index);
    this.closed.emit();
  }

  optionId(command: CommandPaletteCommand, index: number): string {
    return this.controller.optionId(command, index);
  }
}
