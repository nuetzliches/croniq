import { CdkTrapFocus } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, output, viewChild } from '@angular/core';
import { CqDialogComponent, CqDialogHeaderDirective } from 'ui-kit';
import { CommandPaletteCommand, CommandPaletteController } from './command-palette.controller';

@Component({
  selector: 'cq-command-palette',
  templateUrl: './command-palette.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkTrapFocus, CqDialogComponent, CqDialogHeaderDirective],
})
export class CommandPalette {
  readonly closed = output<void>();
  private readonly controller = inject(CommandPaletteController);
  private readonly searchField = viewChild<ElementRef<HTMLInputElement>>('commandPaletteInput');
  private lastFocus: HTMLElement | null = null;

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
    if (!this.lastFocus) {
      const active = document.activeElement;
      this.lastFocus = active instanceof HTMLElement ? active : null;
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
    this.restoreFocus();
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
        this.restoreFocus();
      }
    }
  }

  execute(command: CommandPaletteCommand, index: number): void {
    this.controller.executeCommand(index);
    this.closed.emit();
    this.restoreFocus();
  }

  optionId(command: CommandPaletteCommand, index: number): string {
    return this.controller.optionId(command, index);
  }

  private restoreFocus(): void {
    this.lastFocus?.focus();
    this.lastFocus = null;
  }
}
