import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';import { Telemetry } from './telemetry';

describe('Telemetry', () => {
  let component: Telemetry;
  let fixture: ComponentFixture<Telemetry>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Telemetry],
      providers: [provideZonelessChangeDetection()],
    })
    .compileComponents();

    fixture = TestBed.createComponent(Telemetry);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
