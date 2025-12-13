import { ComponentFixture, TestBed } from '@angular/core/testing';

import { CommandPalette } from './command-palette';

describe('CommandPalette', () => {
  let component: CommandPalette;
  let fixture: ComponentFixture<CommandPalette>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CommandPalette]
    })
    .compileComponents();

    fixture = TestBed.createComponent(CommandPalette);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
