import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { Dialog } from '@angular/cdk/dialog';
import { JobsStore } from '@features/jobs/jobs.store';
import { JobsPage } from './jobs-page';

class JobsStoreStub {
  readonly manualTriggers = signal([]);
  readonly pendingCount = signal(0);
  readonly lastError = signal<string | null>(null);

  readonly jobRegistry = signal([]);
  readonly jobRegistryLoading = signal(false);
  readonly jobRegistryError = signal<string | null>(null);

  triggerJob = vi.fn();
  refreshJobRegistry = vi.fn();
  upsertJob = vi.fn();
}

const dialogStub = {
  open: vi.fn()
};

describe('JobsPage', () => {
  let component: JobsPage;
  let fixture: ComponentFixture<JobsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [JobsPage],
      providers: [
        provideZonelessChangeDetection(),
        { provide: Dialog, useValue: dialogStub }
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
