import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { JobsPage } from './jobs-page';
import { JobsStore } from '../jobs.store';

class JobsStoreStub {
  readonly manualTriggers = signal([]);
  readonly pendingCount = signal(0);
  readonly lastError = signal<string | null>(null);

  triggerJob = jasmine.createSpy('triggerJob');
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
