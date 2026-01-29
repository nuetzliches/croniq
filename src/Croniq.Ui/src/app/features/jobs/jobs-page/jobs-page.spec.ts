import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, convertToParamMap } from '@angular/router';
import { JobsStore } from '@features/jobs/jobs.store';
import { CqDialogService } from 'ui-kit';
import { of } from 'rxjs';
import { JobsPage } from './jobs-page';

class JobsStoreStub {
  readonly manualTriggers = signal([]);
  readonly pendingCount = signal(0);
  readonly lastError = signal<string | null>(null);

  readonly jobRegistry = signal([]);
  readonly jobRegistryLoading = signal(false);
  readonly jobRegistryError = signal<string | null>(null);
  readonly jobDetail = signal(null);
  readonly jobDetailLoading = signal(false);
  readonly jobDetailError = signal<string | null>(null);
  readonly deleteJobLoading = signal(false);
  readonly deleteJobError = signal<string | null>(null);
  readonly toggleSchedulesLoading = signal(false);
  readonly toggleSchedulesError = signal<string | null>(null);
  readonly executions = signal([]);
  readonly executionsLoading = signal(false);
  readonly executionsError = signal<string | null>(null);
  readonly activateJobLoading = signal(false);
  readonly activateJobError = signal<string | null>(null);

  triggerJob = vi.fn();
  refreshJobRegistry = vi.fn();
  refreshJobDetail = vi.fn();
  refreshExecutions = vi.fn();
  upsertJob = vi.fn();
  deleteJob = vi.fn();
  setJobSchedulesEnabled = vi.fn();
  activateJob = vi.fn();
  deactivateJob = vi.fn();
}

const dialogStub = {
  open: vi.fn()
};
const routerStub = {
  navigate: vi.fn(() => Promise.resolve(true)),
};
const activatedRouteStub = {
  queryParamMap: of(convertToParamMap({})),
};

describe('JobsPage', () => {
  let component: JobsPage;
  let fixture: ComponentFixture<JobsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [JobsPage],
      providers: [
        provideZonelessChangeDetection(),
        { provide: CqDialogService, useValue: dialogStub },
        { provide: Router, useValue: routerStub },
        { provide: ActivatedRoute, useValue: activatedRouteStub },
      ],
    })
      .overrideComponent(JobsPage, {
        set: {
          providers: [{ provide: JobsStore, useClass: JobsStoreStub }]
        }
      })
      .compileComponents();

    fixture = TestBed.createComponent(JobsPage);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
