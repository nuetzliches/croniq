import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { nowIso } from '@core/time/clock';
import { SchedulesPage } from './schedules-page';
import { ScheduleRow, SchedulesStore } from './schedules.store';

class SchedulesStoreStub {
  readonly schedules = signal<ReadonlyArray<ScheduleRow>>([]);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);
  readonly lastUpdated = signal(nowIso());
  readonly calendarOptions = signal<ReadonlyArray<{ calendarId: string; label: string }>>([]);
  readonly calendarOptionsLoading = signal(false);
  readonly calendarOptionsError = signal<string | null>(null);
  readonly calendarOptionsPermissionDenied = signal(false);

  readonly scheduleDetail = signal<unknown | null>(null);
  readonly scheduleDetailLoading = signal(false);
  readonly scheduleDetailError = signal<string | null>(null);

  readonly deleteScheduleLoading = signal(false);
  readonly deleteScheduleError = signal<string | null>(null);

  readonly upsertScheduleLoading = signal(false);
  readonly upsertScheduleError = signal<string | null>(null);

  readonly scheduleDeadLetters = signal<ReadonlyArray<{ id: number }>>([]);
  readonly scheduleDeadLettersLoading = signal(false);
  readonly scheduleDeadLettersError = signal<string | null>(null);
  readonly scheduleDeadLetterCount = signal(0);

  readonly executions = signal<ReadonlyArray<{ id: string }>>([]);
  readonly executionsLoading = signal(false);
  readonly executionsError = signal<string | null>(null);

  refresh = vi.fn();

  refreshScheduleDetail = vi.fn();
  deleteSchedule = vi.fn();
  upsertSchedule = vi.fn();
  refreshScheduleDeadLetters = vi.fn();
  replayScheduleDeadLetter = vi.fn();
}

describe('SchedulesPage', () => {
  let component: SchedulesPage;
  let fixture: ComponentFixture<SchedulesPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [SchedulesPage],
      providers: [provideZonelessChangeDetection()],
    })
      .overrideComponent(SchedulesPage, {
        set: {
          providers: [{ provide: SchedulesStore, useClass: SchedulesStoreStub }],
        },
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
