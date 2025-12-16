import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { JobsStore } from '../jobs.store';
import { JobsPage } from './jobs-page';

class JobsStoreStub {
  readonly manualTriggers = signal([]);
  readonly pendingCount = signal(0);
  readonly lastError = signal<string | null>(null);

  triggerJob = vi.fn();
}

describe('JobsPage', () => {
  let component: JobsPage;
  let fixture: ComponentFixture<JobsPage>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [JobsPage],
      providers: [{ provide: JobsStore, useClass: JobsStoreStub }],
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
