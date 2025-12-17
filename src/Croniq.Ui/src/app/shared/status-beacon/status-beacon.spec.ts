import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';import { StatusBeacon } from './status-beacon';

describe('StatusBeacon', () => {
  let component: StatusBeacon;
  let fixture: ComponentFixture<StatusBeacon>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [StatusBeacon],
      providers: [provideZonelessChangeDetection()],
    })
    .compileComponents();

    fixture = TestBed.createComponent(StatusBeacon);
    fixture.componentRef.setInput('label', 'Status');
    fixture.componentRef.setInput('value', 'OK');
    fixture.detectChanges();
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
