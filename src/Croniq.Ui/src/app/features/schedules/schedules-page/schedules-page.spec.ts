import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { nowIso } from '@core/time/clock';
import { ScheduleSummary } from '@croniq/api-schema';
import { SchedulesPage } from './schedules-page';
import { SchedulesStore } from './schedules.store';

class SchedulesStoreStub {
  readonly schedules = signal<ReadonlyArray<ScheduleSummary>>([]);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);
  readonly lastUpdated = signal(nowIso());

  refresh = vi.fn();
}

describe('SchedulesPage', () => {
  let component: SchedulesPage;
  let fixture: ComponentFixture<SchedulesPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [SchedulesPage],
      providers: [provideZonelessChangeDetection(), { provide: SchedulesStore, useClass: SchedulesStoreStub }],
    })
      .compileComponents();

    fixture = TestBed.createComponent(SchedulesPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
