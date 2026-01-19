import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { CqIconComponent } from './icon';

describe('CqIconComponent', () => {
  let fixture: ComponentFixture<CqIconComponent>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CqIconComponent],
      providers: [provideZonelessChangeDetection()],
    }).compileComponents();

    fixture = TestBed.createComponent(CqIconComponent);
  });

  it('renders an svg for a known icon', () => {
    fixture.componentRef.setInput('name', 'magnify');
    fixture.detectChanges();

    const svg = fixture.nativeElement.querySelector('svg');
    expect(svg).not.toBeNull();
  });

  it('marks icons as decorative when no label is provided', () => {
    fixture.componentRef.setInput('name', 'magnify');
    fixture.detectChanges();

    const svg = fixture.nativeElement.querySelector('svg');
    expect(svg?.getAttribute('aria-hidden')).toBe('true');
    expect(svg?.getAttribute('role')).toBeNull();
  });

  it('applies aria-label when provided', () => {
    fixture.componentRef.setInput('name', 'magnify');
    fixture.componentRef.setInput('ariaLabel', 'Search');
    fixture.detectChanges();

    const svg = fixture.nativeElement.querySelector('svg');
    expect(svg?.getAttribute('aria-label')).toBe('Search');
    expect(svg?.getAttribute('aria-hidden')).toBeNull();
  });
});
